// main.c — gateway ECU firmware
//
// The gateway is the central coordinator. It:
// 1. Monitors heartbeats from all ECUs
// 2. Determines the vehicle's operating mode
// 3. Aggregates faults from all sources
// 4. Broadcasts vehicle mode and warnings
//
// Multi-task: heartbeat_rx (prio 4) captures CAN → queue,
// gateway_main (prio 3) processes heartbeats/faults/mode,
// fault_aggregator (prio 2) low-rate fault aggregation.
//
// FreeRTOS primitives exercised: Mutex, Event groups, Task notifications,
// Queues (all created via xSemaphoreCreateMutex, xEventGroupCreate, etc.)
//
// Compiles as FreeRTOS tasks running on the costar simulator.

#include "FreeRTOS.h"
#include "task.h"
#include "queue.h"
#include "semphr.h"
#include "event_groups.h"
#include "timers.h"

#define CAN_BUS 0

#include "gateway_state.h"
#include "heartbeat_monitor.h"
#include "fault_manager.h"
#include "microcar_protocol.h"
#include "microcar_ota_slot.h"
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include "sim_abi.h"
#include <string.h>

// ── Heartbeat queue item ───────────────────────────────────────────────────

/// A single heartbeat event pushed from heartbeat_rx onto the queue.
typedef struct {
    uint8_t  sender;
    uint32_t uptime_ms;
} hb_event_t;

// ── Global state ──────────────────────────────────────────────────────────

static gateway_state_t     g_gs;
static heartbeat_monitor_t g_hm;
static fault_manager_t     g_fm;
static uint8_t             g_diag_session_active = 0;
static uint8_t             g_diag_dogfood_script = 0;
static uint8_t             g_diag_dogfood_inject_fault = 0;
static uint8_t             g_charging_dogfood_script = 0;
static uint8_t             g_ota_dogfood_script = 0;

// OTA dogfood fault-injection selector (see run_dogfood_ota_script). 0 = the
// happy-path campaign; non-zero values inject one fault-matrix case. Each fault
// gets its own gateway_enable_dogfood_ota_fault_* wrapper so a scenario selects
// it purely by firmware path (no CAN input needed).
#define OTA_FAULT_NONE    0u
#define OTA_FAULT_BAD_CRC 1u
#define OTA_FAULT_INTERRUPTED_WRITE 2u
#define OTA_FAULT_BAD_HEALTH        3u
#define OTA_FAULT_POWERCUT_PRECOMMIT 4u
static uint8_t             g_ota_fault_mode = OTA_FAULT_NONE;

// ── FreeRTOS primitives ───────────────────────────────────────────────────

/// Mutex protecting fault_manager_t (guards concurrent access from
/// gateway_main and fault_aggregator).
static SemaphoreHandle_t g_fm_mutex = NULL;

/// Event group for mode transition signalling.
/// Bits:
///   0x01 – mode changed (set by gateway_main)
///   0x02 – critical fault raised
static EventGroupHandle_t g_mode_events = NULL;

/// Queue carrying heartbeat events from heartbeat_rx → gateway_main.
/// Depth: 16 items, each sizeof(hb_event_t).
static QueueHandle_t g_hb_queue = NULL;

/// Counting semaphore for CAN frame preemption.
/// heartbeat_rx (prio 4) gives → can_frame_processor (prio 5) takes.
/// Max count 64, initial 0.
static SemaphoreHandle_t g_can_frame_sem = NULL;

/// Task handle for gateway_main (receives task notifications for
/// urgent fault alerts).
static TaskHandle_t g_gateway_task_handle = NULL;

// ── Boot ──────────────────────────────────────────────────────────────────

/// Allocate FreeRTOS primitives. Called once from gateway_main.
static void gateway_primitives_init(void)
{
    g_fm_mutex     = xSemaphoreCreateMutex();
    g_mode_events  = xEventGroupCreate();
    g_hb_queue     = xQueueCreate(16, sizeof(hb_event_t));
    g_can_frame_sem = xSemaphoreCreateCounting(64, 0);

    sim_trace_u32("gateway_mutex", g_fm_mutex != NULL ? 1 : 0);
    sim_trace_u32("gateway_event_group", g_mode_events != NULL ? 1 : 0);
    sim_trace_u32("gateway_queue", g_hb_queue != NULL ? 1 : 0);
    sim_trace_u32("gateway_can_sem", g_can_frame_sem != NULL ? 1 : 0);
}

void gateway_init(void)
{
    gateway_state_init(&g_gs);
    heartbeat_monitor_init(&g_hm, MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    fault_manager_init(&g_fm);

    // Register nodes to monitor with their heartbeat timeouts.
    heartbeat_monitor_register(&g_hm, MC_NODE_POWERTRAIN,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&g_hm, MC_NODE_BMS,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&g_hm, MC_NODE_DASHBOARD,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
}

void gateway_enable_dogfood_diag_script(uint8_t inject_fault)
{
    g_diag_dogfood_script = 1;
    g_diag_dogfood_inject_fault = inject_fault ? 1 : 0;
}

void gateway_enable_dogfood_charging_script(void)
{
    g_charging_dogfood_script = 1;
}

void gateway_enable_dogfood_ota_script(void)
{
    g_ota_dogfood_script = 1;
    g_ota_fault_mode = OTA_FAULT_NONE;
}

// OTA fault-matrix variant: a corrupt image fails CRC verification, so the slot
// model must refuse to arm slot B and roll back to the known-good slot A.
void gateway_enable_dogfood_ota_fault_bad_crc(void)
{
    g_ota_dogfood_script = 1;
    g_ota_fault_mode = OTA_FAULT_BAD_CRC;
}

// OTA fault-matrix variant: the image download is interrupted (power cut during
// write), so the partial image is discarded and the update aborts to slot A
// before it ever verifies.
void gateway_enable_dogfood_ota_fault_interrupted_write(void)
{
    g_ota_dogfood_script = 1;
    g_ota_fault_mode = OTA_FAULT_INTERRUPTED_WRITE;
}

// OTA fault-matrix variant: the new slot downloads, verifies and commits, but
// the post-reboot self-test fails (bad boot), so the model rolls back to the
// previous known-good slot A.
void gateway_enable_dogfood_ota_fault_bad_health(void)
{
    g_ota_dogfood_script = 1;
    g_ota_fault_mode = OTA_FAULT_BAD_HEALTH;
}

// OTA fault-matrix variant: a power cut strikes after the image has been written
// and verified but before the atomic commit. The verified-but-uncommitted image
// is discarded and the bootloader stays on the known-good slot A — proving the
// commit is the point of no return (a valid image still reverts if it never
// committed).
void gateway_enable_dogfood_ota_fault_powercut_precommit(void)
{
    g_ota_dogfood_script = 1;
    g_ota_fault_mode = OTA_FAULT_POWERCUT_PRECOMMIT;
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process a heartbeat frame (0x001) from any node.
/// Called from gateway_main after dequeuing an hb_event_t.
static void handle_heartbeat(uint32_t now_ms, const hb_event_t *ev)
{
    heartbeat_monitor_beat(&g_hm, ev->sender, now_ms);
    (void)ev->uptime_ms;
}

/// Process a BMS fault frame (0x202).
/// Protected by mutex.
static void handle_bms_fault(const mc_can_frame_t *frame)
{
    uint8_t fault_code = frame->data[0];
    uint8_t severity   = fault_manager_bms_severity(fault_code);

    if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_manager_report(&g_fm, MC_NODE_BMS, fault_code, severity);

        // If critical fault, notify gateway_main immediately.
        if (severity == 2 && g_gateway_task_handle != NULL) {
            xTaskNotify(g_gateway_task_handle,
                        (uint32_t)fault_code,
                        eSetValueWithoutOverwrite);
        }

        xSemaphoreGive(g_fm_mutex);
    }
}

static uint8_t first_active_dtc(uint8_t *source_node, uint8_t *fault_code)
{
    uint8_t count = 0;
    *source_node = 0;
    *fault_code = 0;

    if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        for (uint8_t i = 0; i < g_fm.fault_count; i++) {
            if (g_fm.faults[i].active) {
                if (count == 0) {
                    *source_node = g_fm.faults[i].source_node;
                    *fault_code = g_fm.faults[i].fault_code;
                }
                count++;
            }
        }
        xSemaphoreGive(g_fm_mutex);
    }

    return count;
}

static void send_diag_response(uint8_t service, uint8_t request_id,
                               uint8_t status, uint8_t value0,
                               uint8_t value1)
{
    mc_can_frame_t tx;
    mc_frame_init(&tx, MC_MSG_DIAG_RESPONSE, MC_NODE_GATEWAY,
                  MC_DIAG_RESPONSE_MSG_SIZE);
    tx.data[0] = MC_NODE_GATEWAY;
    tx.data[1] = service;
    tx.data[2] = request_id;
    tx.data[3] = status;
    tx.data[4] = value0;
    tx.data[5] = value1;
    sim_trace_u32("gateway_diag_response",
                  ((uint32_t)request_id << 24)
                | ((uint32_t)service << 16)
                | ((uint32_t)status << 8)
                | (uint32_t)value0);
    sim_trace_u32("gateway_diag_response_v1",
                  ((uint32_t)request_id << 24)
                | ((uint32_t)service << 16)
                | (uint32_t)value1);
    sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
}

static void handle_diag_request(const mc_can_frame_t *frame)
{
    if (frame->len < MC_DIAG_REQUEST_MSG_SIZE) return;

    uint8_t service = frame->data[1];
    uint8_t request_id = frame->data[2];
    uint8_t status = MC_DIAG_OK;
    uint8_t value0 = 0;
    uint8_t value1 = 0;

    switch (service) {
    case MC_DIAG_START_SESSION:
        if (g_gs.mode == VEHICLE_DRIVE) {
            status = MC_DIAG_REJECTED;
        } else {
            g_diag_session_active = 1;
            g_gs.mode = VEHICLE_SERVICE;
            value0 = (uint8_t)g_gs.mode;
            sim_trace_u32("diag_session", 1);
            sim_trace_u32("vehicle_mode", (uint32_t)g_gs.mode);
            xEventGroupSetBits(g_mode_events, 0x01);
        }
        break;

    case MC_DIAG_END_SESSION:
        g_diag_session_active = 0;
        value0 = (uint8_t)g_gs.mode;
        sim_trace_u32("diag_session", 0);
        break;

    case MC_DIAG_READ_MODE:
        value0 = (uint8_t)g_gs.mode;
        break;

    case MC_DIAG_READ_DTCS:
        {
            uint8_t source = 0;
            value0 = first_active_dtc(&source, &value1);
            (void)source;
        }
        status = MC_DIAG_OK;
        break;

    case MC_DIAG_CLEAR_DTCS:
        if (!g_diag_session_active) {
            status = MC_DIAG_REJECTED;
        } else if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
            fault_manager_clear_node(&g_fm, MC_NODE_BMS);
            xSemaphoreGive(g_fm_mutex);
            value0 = 0;
        }
        break;

    case MC_DIAG_ACTUATOR_TEST:
        status = g_diag_session_active && g_gs.mode != VEHICLE_DRIVE
            ? MC_DIAG_OK
            : MC_DIAG_REJECTED;
        value0 = (uint8_t)g_gs.mode;
        break;

    case MC_DIAG_LIVE_BMS:
    default:
        status = MC_DIAG_UNSUPPORTED;
        break;
    }

    send_diag_response(service, request_id, status, value0, value1);
}

static void synth_diag_request(uint8_t service, uint8_t request_id)
{
    mc_can_frame_t rx;
    mc_frame_init(&rx, MC_MSG_DIAG_REQUEST, MC_NODE_DIAGNOSTICS,
                  MC_DIAG_REQUEST_MSG_SIZE);
    rx.data[0] = MC_NODE_DIAGNOSTICS;
    rx.data[1] = service;
    rx.data[2] = request_id;
    rx.data[3] = 0;
    handle_diag_request(&rx);
}

static void synth_bms_fault(uint8_t fault_code)
{
    mc_can_frame_t rx;
    mc_frame_init(&rx, MC_MSG_BMS_FAULT, MC_NODE_BMS, 2);
    rx.data[0] = fault_code;
    rx.data[1] = 0;
    handle_bms_fault(&rx);
}

static void run_dogfood_diag_script(uint32_t *script_ms, uint32_t *sent_mask)
{
    if (!g_diag_dogfood_script) return;

    *script_ms += 10;

    if (*script_ms >= 100 && !(*sent_mask & 0x01)) {
        synth_diag_request(MC_DIAG_START_SESSION, 1);
        *sent_mask |= 0x01;
    }
    if (*script_ms >= 200 && !(*sent_mask & 0x02)) {
        synth_diag_request(MC_DIAG_READ_MODE, 2);
        *sent_mask |= 0x02;
    }
    if (g_diag_dogfood_inject_fault
        && *script_ms >= 300
        && !(*sent_mask & 0x04)) {
        synth_bms_fault(MC_BMS_FAULT_OVERTEMP);
        *sent_mask |= 0x04;
    }
    if (*script_ms >= 350 && !(*sent_mask & 0x08)) {
        synth_diag_request(MC_DIAG_ACTUATOR_TEST, 6);
        *sent_mask |= 0x08;
    }
    if (*script_ms >= 450 && !(*sent_mask & 0x10)) {
        synth_diag_request(MC_DIAG_READ_DTCS, 3);
        *sent_mask |= 0x10;
    }
    if (*script_ms >= 650 && !(*sent_mask & 0x20)) {
        synth_diag_request(MC_DIAG_CLEAR_DTCS, 4);
        *sent_mask |= 0x20;
    }
    if (*script_ms >= 750 && !(*sent_mask & 0x40)) {
        synth_diag_request(MC_DIAG_READ_DTCS, 5);
        *sent_mask |= 0x40;
    }
}

/// Drive the charging dogfood lane inside gateway firmware.
///
/// This exercises the charging safety contract without relying on the
/// (still unreliable) firmware CAN RX path, mirroring the diagnostics script:
///   * at 100ms a charger is "plugged in" → the gateway enters CHARGING and
///     broadcasts the mode (mc_gateway_determine_mode keeps CHARGING sticky);
///   * at 300ms a drive request arrives while plugged → gateway_state_enter_drive
///     is a no-op outside READY, so the vehicle must remain in CHARGING
///     (drive blocked while plugged).
static void run_dogfood_charging_script(uint32_t *script_ms, uint32_t *sent_mask)
{
    if (!g_charging_dogfood_script) return;

    *script_ms += 10;

    // Plug inserted → enter CHARGING.
    if (*script_ms >= 100 && !(*sent_mask & 0x01)) {
        g_gs.mode = VEHICLE_CHARGING;
        sim_trace_u32("charging_plug", 1);
        sim_trace_u32("gateway_charging_state", (uint32_t)g_gs.mode);
        sim_trace_u32("vehicle_mode", (uint32_t)g_gs.mode);
        xEventGroupSetBits(g_mode_events, 0x01);
        *sent_mask |= 0x01;
    }

    // Drive request while plugged → must stay in CHARGING.
    if (*script_ms >= 300 && !(*sent_mask & 0x02)) {
        gateway_state_enter_drive(&g_gs); // no-op unless mode == READY
        uint8_t blocked = (g_gs.mode == VEHICLE_CHARGING) ? 1 : 0;
        sim_trace_u32("charging_drive_blocked", blocked);
        sim_trace_u32("gateway_charging_state", (uint32_t)g_gs.mode);
        *sent_mask |= 0x02;
    }
}

/// Emit the trace markers for an OTA rollback: the update aborted and the
/// bootloader reverted to the previous known-good slot. Shared by every
/// fault-matrix variant so each rolls back with an identical marker set.
static void emit_ota_rollback(const mc_ota_slot_state_t *ota)
{
    sim_trace_u32("ota_state", ota->state); // MC_OTA_ROLLED_BACK
    sim_trace_u32("ota_rollback", ota->rolled_back);
    sim_trace_u32("ota_active_slot", ota->active_slot);
    sim_trace_u32("ota_boot_result", 0);
}

/// Drive the OTA (over-the-air firmware update) dogfood lane inside gateway
/// firmware.
///
/// Like the charging/diagnostics scripts this exercises the OTA state machine
/// via compact trace events without depending on the (still unreliable)
/// firmware CAN RX path. It drives the pure slot-metadata model
/// (common/src/microcar_ota_slot.c) so the lane asserts the model's real
/// commit/rollback behavior end-to-end.
///
/// Happy path (OTA_FAULT_NONE):
///   IDLE(0) → DOWNLOADING(1) → VERIFYING(2, crc ok) → COMMIT_PENDING(3, slot B)
///   → REBOOTING(4) → HEALTHY(5, boot ok).
/// Fault-matrix variants all roll back to the known-good slot A (state 6),
/// each aborting at the step its fault strikes:
///   * OTA_FAULT_BAD_CRC          — corrupt image fails verify:
///       IDLE → DOWNLOADING → VERIFYING(crc BAD) → ROLLED_BACK.
///   * OTA_FAULT_INTERRUPTED_WRITE — power cut during write (partial image):
///       IDLE → DOWNLOADING → ROLLED_BACK (never verifies).
///   * OTA_FAULT_BAD_HEALTH        — armed image boots but fails self-test:
///       IDLE → DOWNLOADING → VERIFYING → COMMIT_PENDING → REBOOTING → ROLLED_BACK.
///   * OTA_FAULT_POWERCUT_PRECOMMIT — power cut after verify, before the atomic
///     commit; the verified-but-uncommitted image is discarded, revert to A:
///       IDLE → DOWNLOADING → VERIFYING(crc ok) → ROLLED_BACK.
/// States are traced as `ota_state` at successive scheduled times, with
/// `ota_crc_ok`, `ota_slot`, `ota_boot_result`, `ota_rollback` and
/// `ota_active_slot` marker events at the relevant steps.
static void run_dogfood_ota_script(uint32_t *ota_ms, uint32_t *ota_sent)
{
    if (!g_ota_dogfood_script) return;

    // Persistent slot-metadata model for this update campaign. One gateway runs
    // per process, so a function-static instance is safe here.
    static mc_ota_slot_state_t ota;
    static uint8_t             ota_inited = 0;
    if (!ota_inited) {
        mc_ota_init(&ota);
        ota_inited = 1;
    }

    // Fault selectors: each fault flips exactly one input to the model.
    int download_complete = (g_ota_fault_mode != OTA_FAULT_INTERRUPTED_WRITE);
    int crc_ok            = (g_ota_fault_mode != OTA_FAULT_BAD_CRC);
    int boot_healthy      = (g_ota_fault_mode != OTA_FAULT_BAD_HEALTH);

    *ota_ms += 10;

    // 100ms — IDLE, update campaign accepted.
    if (*ota_ms >= 100 && !(*ota_sent & 0x01)) {
        sim_trace_u32("ota_state", ota.state); // MC_OTA_IDLE
        *ota_sent |= 0x01;
    }
    // 200ms — DOWNLOADING. An interrupted write discards the partial image and
    // rolls back here, before the image ever verifies.
    if (*ota_ms >= 200 && !(*ota_sent & 0x02)) {
        mc_ota_begin_download(&ota);
        sim_trace_u32("ota_state", ota.state); // MC_OTA_DOWNLOADING
        if (!mc_ota_finish_download(&ota, download_complete)) {
            emit_ota_rollback(&ota);
        }
        *ota_sent |= 0x02;
    }
    // 300ms — VERIFYING. A corrupt image fails CRC and rolls back here.
    if (*ota_ms >= 300 && !(*ota_sent & 0x04)) {
        if (ota.state == MC_OTA_DOWNLOADING) {
            sim_trace_u32("ota_state", MC_OTA_VERIFYING);
            int verified = mc_ota_verify(&ota, crc_ok);
            sim_trace_u32("ota_crc_ok", ota.crc_ok);
            if (!verified) {
                emit_ota_rollback(&ota);
            }
        }
        *ota_sent |= 0x04;
    }
    // 400ms — COMMIT_PENDING: arm slot B as the boot target (only if verified).
    // A power cut here (after verify, before the atomic commit) discards the
    // verified-but-uncommitted image and reverts to slot A — the commit is the
    // point of no return.
    if (*ota_ms >= 400 && !(*ota_sent & 0x08)) {
        if (ota.state == MC_OTA_VERIFYING) {
            if (g_ota_fault_mode == OTA_FAULT_POWERCUT_PRECOMMIT) {
                mc_ota_rollback(&ota);
                emit_ota_rollback(&ota);
            } else {
                mc_ota_commit(&ota);
                sim_trace_u32("ota_state", ota.state); // MC_OTA_COMMIT_PENDING
                sim_trace_u32("ota_slot", ota.target_slot);
            }
        }
        *ota_sent |= 0x08;
    }
    // 500ms — REBOOTING into the new slot (only if armed).
    if (*ota_ms >= 500 && !(*ota_sent & 0x10)) {
        if (ota.state == MC_OTA_COMMIT_PENDING) {
            mc_ota_reboot(&ota);
            sim_trace_u32("ota_state", ota.state); // MC_OTA_REBOOTING
        }
        *ota_sent |= 0x10;
    }
    // 600ms — HEALTHY on a good self-test, else roll back to slot A (bad boot).
    if (*ota_ms >= 600 && !(*ota_sent & 0x20)) {
        if (ota.state == MC_OTA_REBOOTING) {
            if (mc_ota_health_check(&ota, boot_healthy)) {
                sim_trace_u32("ota_state", ota.state); // MC_OTA_HEALTHY
                sim_trace_u32("ota_boot_result", ota.boot_healthy);
            } else {
                emit_ota_rollback(&ota);
            }
        }
        *ota_sent |= 0x20;
    }
}

/// Dispatch a received CAN frame to the appropriate handler.
/// Called from heartbeat_rx task (only processes heartbeat and BMS fault).
static void dispatch_frame_in_rx(const mc_can_frame_t *frame)
{
    switch (frame->id) {
    case MC_MSG_HEARTBEAT:
        // Forward to heartbeat monitor via queue.
        if (frame->sender != MC_NODE_GATEWAY) {
            hb_event_t ev;
            ev.sender = frame->sender;
            ev.uptime_ms = 0;
            if (frame->len >= 5) {
                ev.uptime_ms = ((uint32_t)frame->data[1] << 24)
                             | ((uint32_t)frame->data[2] << 16)
                             | ((uint32_t)frame->data[3] << 8)
                             |  (uint32_t)frame->data[4];
            }
            xQueueSend(g_hb_queue, &ev, 0);
        }
        break;
    case MC_MSG_BMS_FAULT:
        handle_bms_fault(frame);
        break;
    case MC_MSG_DIAG_REQUEST:
        handle_diag_request(frame);
        break;
    default:
        break;
    }
}

// ── Mode determination ────────────────────────────────────────────────────

/// Re-evaluate vehicle mode based on current state.
/// Protected by mutex for fault_manager access.
static mc_vehicle_mode_t update_vehicle_mode(uint32_t now_ms)
{
    // Check timeouts → update online/offline status.
    heartbeat_monitor_check(&g_hm, now_ms);

    uint8_t all_online      = heartbeat_monitor_all_online(&g_hm);
    uint8_t bms_fault       = 0;
    uint8_t bms_limp        = 0;
    uint8_t fault_count     = 0;

    if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        bms_fault   = fault_manager_has_critical(&g_fm);
        fault_count = fault_manager_active_count(&g_fm);
        xSemaphoreGive(g_fm_mutex);
    }

    if (!bms_fault && g_diag_session_active) {
        g_gs.all_nodes_online = all_online;
        g_gs.bms_fault_active = bms_fault;
        g_gs.bms_limp_requested = bms_limp;
        g_gs.active_fault_count = fault_count;
        g_gs.mode = VEHICLE_SERVICE;
        return g_gs.mode;
    }

    return gateway_state_update(&g_gs, all_online, bms_fault,
                                bms_limp, fault_count);
}

// ── CAN frame construction ────────────────────────────────────────────────

/// Build and send the heartbeat frame.
static void send_heartbeat(uint32_t now_ms, mc_can_frame_t *tx)
{
    mc_frame_init(tx, MC_MSG_HEARTBEAT, MC_NODE_GATEWAY,
                  MC_HEARTBEAT_MSG_SIZE);
    tx->data[0] = MC_NODE_GATEWAY;
    tx->data[1] = (uint8_t)(now_ms >> 24);
    tx->data[2] = (uint8_t)(now_ms >> 16);
    tx->data[3] = (uint8_t)(now_ms >> 8);
    tx->data[4] = (uint8_t)(now_ms);
}

/// Build and send the vehicle mode frame.
static void send_vehicle_mode(mc_can_frame_t *tx)
{
    mc_vehicle_mode_t mode     = g_gs.mode;
    uint8_t           fault_cd = 0;

    if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_cd = g_fm.critical_count > 0 ? 1 : 0;
        xSemaphoreGive(g_fm_mutex);
    }

    mc_frame_init(tx, MC_MSG_VEHICLE_MODE, MC_NODE_GATEWAY,
                  MC_VEHICLE_MODE_MSG_SIZE);
    tx->data[0] = (uint8_t)mode;
    tx->data[1] = fault_cd;
}

// ── Heartbeat RX task ─────────────────────────────────────────────────────

/// High-priority task that drains CAN RX and pushes heartbeat events
/// onto g_hb_queue.  Also handles BMS fault frames inline.
void heartbeat_rx(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(5));

        uint32_t can_id;
        uint32_t is_ext;
        uint32_t is_remote;
        while (1) {
            mc_can_frame_t rx;
            uint32_t dlc = sim_can_recv(0, rx.data, MC_MAX_PAYLOAD_SIZE,
                                        &can_id, &is_ext, &is_remote);
            if (dlc == 0) break;

            rx.id = can_id;
            rx.sender = rx.data[0];
            rx.len = (uint8_t)dlc;
            dispatch_frame_in_rx(&rx);

            // Give semaphore to preempt: can_frame_processor (prio 5)
            // will wake and process this frame.
            xSemaphoreGive(g_can_frame_sem);
        }
    }
}

// ── Fault aggregator task ──────────────────────────────────────────────────

/// Low-rate task that aggregates fault statistics and traces them.
/// Accesses fault_manager under mutex protection.
void fault_aggregator(void *pvParameters)
{
    (void)pvParameters;

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(100));

        uint8_t critical = 0;
        uint8_t active   = 0;
        uint8_t warning  = 0;

        if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(10)) == pdTRUE) {
            critical = g_fm.critical_count;
            warning  = g_fm.warning_count;
            active   = fault_manager_active_count(&g_fm);
            xSemaphoreGive(g_fm_mutex);
        }

        // Trace aggregated fault stats.
        uint32_t agg = ((uint32_t)critical << 16) | ((uint32_t)warning << 8) | active;
        sim_trace_u32("fault_aggregate", agg);

        // If critical fault exists, set event group bit.
        if (critical > 0) {
            xEventGroupSetBits(g_mode_events, 0x02);
        }
    }
}

// ── CAN frame processor task ──────────────────────────────────────────

/// Highest-priority task (5) that processes CAN frames via counting
/// semaphore.  heartbeat_rx (prio 4) gives the semaphore for each
/// received frame, which preempts lower-priority tasks so this
/// processor runs immediately.
/// Each activation traces a counter showing preemption count.
void can_frame_processor(void *pvParameters)
{
    (void)pvParameters;

    uint32_t proc_count = 0;

    while (1) {
        // Block until heartbeat_rx gives the semaphore.
        xSemaphoreTake(g_can_frame_sem, portMAX_DELAY);

        proc_count++;

        // Trace the processing event to show preemption occurred.
        // The value is the activation count — proves every give
        // caused a preemption wakeup.
        sim_trace_u32("can_frame_proc", proc_count);
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────

/// Gateway main task entry point.
///
/// Runs every 10ms virtual time.  Dequeues heartbeat events from
/// heartbeat_rx, processes faults, updates vehicle mode, and broadcasts
/// status.  Also checks event group for mode changes and task
/// notifications for urgent faults.
void gateway_main(void *pvParameters)
{
    (void)pvParameters;
    gateway_init();
    gateway_primitives_init();
    g_gateway_task_handle = xTaskGetCurrentTaskHandle();

    // Create subordinate tasks.
    xTaskCreate(heartbeat_rx, "hb_rx", 768, NULL, 4, NULL);
    xTaskCreate(fault_aggregator, "fault_agg", 512, NULL, 2, NULL);
    xTaskCreate(can_frame_processor, "can_proc", 768, NULL, 5, NULL);

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t tx;
    uint32_t diag_script_ms = 0;
    uint32_t diag_script_sent = 0;
    uint32_t charging_script_ms = 0;
    uint32_t charging_script_sent = 0;
    uint32_t ota_script_ms = 0;
    uint32_t ota_script_sent = 0;

    // Send initial heartbeat at boot.
    send_heartbeat(0, &tx);
    sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // ── Dequeue heartbeat events ────────────────────────────
        hb_event_t ev;
        while (xQueueReceive(g_hb_queue, &ev, 0) == pdTRUE) {
            handle_heartbeat(now_ms, &ev);
        }

        run_dogfood_diag_script(&diag_script_ms, &diag_script_sent);
        run_dogfood_charging_script(&charging_script_ms, &charging_script_sent);
        run_dogfood_ota_script(&ota_script_ms, &ota_script_sent);

        // ── Check for urgent fault notifications ────────────────
        uint32_t notify_val = 0;
        if (xTaskNotifyWait(0, 0xFFFFFFFF, &notify_val, 0) == pdTRUE) {
            sim_trace_u32("urgent_fault_notify", notify_val);
        }

        // ── Process phase: check heartbeats for timeouts ────────
        int transitions = heartbeat_monitor_check(&g_hm, now_ms);
        if (transitions > 0) {
            uint8_t lost_node = heartbeat_monitor_last_transition_node(&g_hm);
            if (lost_node == MC_NODE_BMS) {
                // BMS lost → report fault.
                if (xSemaphoreTake(g_fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
                    fault_manager_report(&g_fm, MC_NODE_BMS,
                                         MC_BMS_FAULT_COMM_ERROR, 2);
                    xSemaphoreGive(g_fm_mutex);
                }
            }
        }

        // ── Mode update ─────────────────────────────────────────
        mc_vehicle_mode_t old_mode = g_gs.mode;
        mc_vehicle_mode_t new_mode = update_vehicle_mode(now_ms);

        if (new_mode != old_mode) {
            sim_trace_u32("vehicle_mode", (uint32_t)new_mode);
            // Signal mode change to event group.
            xEventGroupSetBits(g_mode_events, 0x01);
        }

        // ── Check event group for mode transitions ──────────────
        // (this lets other tasks or tests wait for mode changes)
        EventBits_t mode_bits = xEventGroupGetBits(g_mode_events);
        if (mode_bits & 0x01) {
            sim_trace_u32("mode_event_group", mode_bits);
            xEventGroupClearBits(g_mode_events, 0x01);
        }

        // ── Broadcast phase ─────────────────────────────────────
        // Send heartbeat every 100ms.
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }

        // Send vehicle mode on change or every 50ms.
        if (new_mode != old_mode || now_ms % 50 == 0) {
            send_vehicle_mode(&tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
