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
//
// Per-instance state is allocated via sim_instance_state (key 0x4D430001)
// so multiple in-process World instances each get independent mutable state.

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
#include "microcar_trace.h"
#include "microcar_can.h"
#include "microcar_ota_slot.h"
#include "sim_abi.h"
#include <string.h>
#include <stdalign.h>

// ── Heartbeat queue item ───────────────────────────────────────────────────

/// A single heartbeat event pushed from heartbeat_rx onto the queue.
typedef struct {
    uint8_t  sender;
    uint32_t uptime_ms;
} hb_event_t;

// ── Instance key ──────────────────────────────────────────────────────────

#define GATEWAY_KEY 0x4D430001

// ── OTA fault selectors ────────────────────────────────────────────────────

#define OTA_FAULT_NONE              0u
#define OTA_FAULT_BAD_CRC           1u
#define OTA_FAULT_INTERRUPTED_WRITE 2u
#define OTA_FAULT_BAD_HEALTH        3u
#define OTA_FAULT_POWERCUT_PRECOMMIT 4u

// ── Context structure ─────────────────────────────────────────────────────

/// Context structure holding all mutable ECU state, allocated per machine.
typedef struct {
    // Core state structs.
    gateway_state_t     gs;
    heartbeat_monitor_t hm;
    fault_manager_t     fm;

    // Dogfood / debug-gym flags.
    uint8_t diag_session_active;
    uint8_t diag_dogfood_script;
    uint8_t diag_dogfood_inject_fault;
    uint8_t charging_dogfood_script;
    uint8_t ota_dogfood_script;
    uint8_t ota_fault_mode;
    uint8_t ota_crc_check_bug;
    uint8_t diag_extra_pt_fault;
    uint8_t diag_clear_all_bug;
    uint8_t diag_startdrive_script;
    uint8_t diag_startsession_drive_bug;

    // FreeRTOS primitives.
    SemaphoreHandle_t   fm_mutex;
    EventGroupHandle_t  mode_events;
    QueueHandle_t       hb_queue;
    SemaphoreHandle_t   can_frame_sem;
    TaskHandle_t        gateway_task_handle;

    // OTA persistent slot state (was function-static; now per-machine).
    mc_ota_slot_state_t ota;
    uint8_t             ota_inited;

    // Dogfood script cursors.
    uint32_t diag_script_ms;
    uint32_t diag_script_sent;
    uint32_t charging_script_ms;
    uint32_t charging_script_sent;
    uint32_t ota_script_ms;
    uint32_t ota_script_sent;
    uint32_t startdrive_script_ms;
    uint32_t startdrive_script_sent;
} gateway_ctx_t;

/// Return the per-machine gateway context, allocating it on first call.
static gateway_ctx_t *gateway_ctx(void)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)sim_instance_state(
        GATEWAY_KEY, sizeof(gateway_ctx_t), alignof(gateway_ctx_t));
    return ctx;
}

// ── Boot ──────────────────────────────────────────────────────────────────

/// Allocate FreeRTOS primitives. Called once from gateway_main.
static void gateway_primitives_init(gateway_ctx_t *ctx)
{
    ctx->fm_mutex      = xSemaphoreCreateMutex();
    ctx->mode_events   = xEventGroupCreate();
    ctx->hb_queue      = xQueueCreate(16, sizeof(hb_event_t));
    ctx->can_frame_sem = xSemaphoreCreateCounting(64, 0);

    sim_trace_u32("gateway_mutex", ctx->fm_mutex != NULL ? 1 : 0);
    sim_trace_u32("gateway_event_group", ctx->mode_events != NULL ? 1 : 0);
    sim_trace_u32("gateway_queue", ctx->hb_queue != NULL ? 1 : 0);
    sim_trace_u32("gateway_can_sem", ctx->can_frame_sem != NULL ? 1 : 0);
}

static void gateway_init(gateway_ctx_t *ctx)
{
    gateway_state_init(&ctx->gs);
    heartbeat_monitor_init(&ctx->hm, MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    fault_manager_init(&ctx->fm);

    // Register nodes to monitor with their heartbeat timeouts.
    heartbeat_monitor_register(&ctx->hm, MC_NODE_POWERTRAIN,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&ctx->hm, MC_NODE_BMS,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
    heartbeat_monitor_register(&ctx->hm, MC_NODE_DASHBOARD,
                               MC_SAFETY_BMS_HEARTBEAT_TIMEOUT_MS);
}

// ── Enable functions (called before task creation) ─────────────────────────

void gateway_enable_dogfood_diag_script(uint8_t inject_fault)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->diag_dogfood_script = 1;
        ctx->diag_dogfood_inject_fault = inject_fault ? 1 : 0;
    }
}

void gateway_enable_dogfood_diag_clear_dtcs(uint8_t buggy)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->diag_dogfood_script = 1;
        ctx->diag_dogfood_inject_fault = 1;
        ctx->diag_extra_pt_fault = 1;
        ctx->diag_clear_all_bug = buggy ? 1 : 0;
    }
}

void gateway_enable_dogfood_diag_startdrive(uint8_t buggy)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->diag_startdrive_script = 1;
        ctx->diag_startsession_drive_bug = buggy ? 1 : 0;
    }
}

void gateway_enable_dogfood_charging_script(void)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) ctx->charging_dogfood_script = 1;
}

void gateway_enable_dogfood_ota_script(void)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->ota_dogfood_script = 1;
        ctx->ota_fault_mode = OTA_FAULT_NONE;
    }
}

void gateway_enable_dogfood_ota_fault_bad_crc(void)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->ota_dogfood_script = 1;
        ctx->ota_fault_mode = OTA_FAULT_BAD_CRC;
    }
}

void gateway_enable_dogfood_ota_fault_interrupted_write(void)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->ota_dogfood_script = 1;
        ctx->ota_fault_mode = OTA_FAULT_INTERRUPTED_WRITE;
    }
}

void gateway_enable_dogfood_ota_fault_bad_health(void)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->ota_dogfood_script = 1;
        ctx->ota_fault_mode = OTA_FAULT_BAD_HEALTH;
    }
}

void gateway_enable_dogfood_ota_fault_powercut_precommit(void)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->ota_dogfood_script = 1;
        ctx->ota_fault_mode = OTA_FAULT_POWERCUT_PRECOMMIT;
    }
}

void gateway_enable_dogfood_ota_bug_bad_crc(void)
{
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx != NULL) {
        ctx->ota_dogfood_script = 1;
        ctx->ota_fault_mode = OTA_FAULT_BAD_CRC;
        ctx->ota_crc_check_bug = 1;
    }
}

// ── Message handlers ──────────────────────────────────────────────────────

static void handle_heartbeat(gateway_ctx_t *ctx, uint32_t now_ms, const hb_event_t *ev)
{
    heartbeat_monitor_beat(&ctx->hm, ev->sender, now_ms);
    (void)ev->uptime_ms;
}

static void handle_bms_fault(gateway_ctx_t *ctx, const mc_can_frame_t *frame)
{
    uint8_t fault_code = frame->data[0];
    uint8_t severity   = fault_manager_bms_severity(fault_code);

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_manager_report(&ctx->fm, MC_NODE_BMS, fault_code, severity);

        if (severity == 2 && ctx->gateway_task_handle != NULL) {
            xTaskNotify(ctx->gateway_task_handle,
                        (uint32_t)fault_code,
                        eSetValueWithoutOverwrite);
        }

        xSemaphoreGive(ctx->fm_mutex);
    }
}

static uint8_t first_active_dtc(gateway_ctx_t *ctx, uint8_t *source_node, uint8_t *fault_code)
{
    uint8_t count = 0;
    *source_node = 0;
    *fault_code = 0;

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        for (uint8_t i = 0; i < ctx->fm.fault_count; i++) {
            if (ctx->fm.faults[i].active) {
                if (count == 0) {
                    *source_node = ctx->fm.faults[i].source_node;
                    *fault_code = ctx->fm.faults[i].fault_code;
                }
                count++;
            }
        }
        xSemaphoreGive(ctx->fm_mutex);
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

static void handle_diag_request(gateway_ctx_t *ctx, const mc_can_frame_t *frame)
{
    if (frame->len < MC_DIAG_REQUEST_MSG_SIZE) return;

    uint8_t service = frame->data[1];
    uint8_t request_id = frame->data[2];
    uint8_t status = MC_DIAG_OK;
    uint8_t value0 = 0;
    uint8_t value1 = 0;

    switch (service) {
    case MC_DIAG_START_SESSION:
        if (ctx->gs.mode == VEHICLE_DRIVE && !ctx->diag_startsession_drive_bug) {
            status = MC_DIAG_REJECTED;
            value0 = (uint8_t)ctx->gs.mode;
        } else {
            ctx->diag_session_active = 1;
            ctx->gs.mode = VEHICLE_SERVICE;
            value0 = (uint8_t)ctx->gs.mode;
            sim_trace_u32("diag_session", 1);
            sim_trace_u32("vehicle_mode", (uint32_t)ctx->gs.mode);
            xEventGroupSetBits(ctx->mode_events, 0x01);
        }
        break;

    case MC_DIAG_END_SESSION:
        ctx->diag_session_active = 0;
        value0 = (uint8_t)ctx->gs.mode;
        sim_trace_u32("diag_session", 0);
        break;

    case MC_DIAG_READ_MODE:
        value0 = (uint8_t)ctx->gs.mode;
        break;

    case MC_DIAG_READ_DTCS:
        {
            uint8_t source = 0;
            value0 = first_active_dtc(ctx, &source, &value1);
            (void)source;
        }
        status = MC_DIAG_OK;
        break;

    case MC_DIAG_CLEAR_DTCS:
        if (!ctx->diag_session_active) {
            status = MC_DIAG_REJECTED;
        } else if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
            if (ctx->diag_clear_all_bug) {
                fault_manager_clear_all(&ctx->fm);
            } else {
                fault_manager_clear_node(&ctx->fm, MC_NODE_BMS);
            }
            xSemaphoreGive(ctx->fm_mutex);
            value0 = 0;
        }
        break;

    case MC_DIAG_ACTUATOR_TEST:
        status = ctx->diag_session_active && ctx->gs.mode != VEHICLE_DRIVE
            ? MC_DIAG_OK
            : MC_DIAG_REJECTED;
        value0 = (uint8_t)ctx->gs.mode;
        break;

    case MC_DIAG_LIVE_BMS:
    default:
        status = MC_DIAG_UNSUPPORTED;
        break;
    }

    send_diag_response(service, request_id, status, value0, value1);
}

static void synth_diag_request(gateway_ctx_t *ctx, uint8_t service, uint8_t request_id)
{
    mc_can_frame_t frame;
    frame.id = MC_MSG_DIAG_REQUEST;
    frame.sender = MC_NODE_DIAGNOSTICS;
    frame.len = MC_DIAG_REQUEST_MSG_SIZE;
    frame.data[0] = MC_NODE_DIAGNOSTICS;
    frame.data[1] = service;
    frame.data[2] = request_id;
    frame.data[3] = 0;
    handle_diag_request(ctx, &frame);
}

static void synth_bms_fault(gateway_ctx_t *ctx, uint8_t fault_code)
{
    mc_can_frame_t frame;
    frame.id = MC_MSG_BMS_FAULT;
    frame.sender = MC_NODE_BMS;
    frame.len = 1;
    frame.data[0] = fault_code;
    handle_bms_fault(ctx, &frame);
}

static void synth_report_fault(gateway_ctx_t *ctx, uint8_t node, uint8_t fault_code, uint8_t severity)
{
    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_manager_report(&ctx->fm, node, fault_code, severity);
        xSemaphoreGive(ctx->fm_mutex);
    }
}

static void run_dogfood_diag_script(gateway_ctx_t *ctx, uint32_t *script_ms, uint32_t *sent_mask)
{
    if (!ctx->diag_dogfood_script) return;

    *script_ms += 10;

    if (*script_ms >= 100 && !(*sent_mask & 0x01)) {
        synth_diag_request(ctx, MC_DIAG_START_SESSION, 1);
        *sent_mask |= 0x01;
    }
    if (*script_ms >= 200 && !(*sent_mask & 0x02)) {
        synth_diag_request(ctx, MC_DIAG_READ_MODE, 2);
        *sent_mask |= 0x02;
    }
    if (ctx->diag_dogfood_inject_fault) {
        if (*script_ms >= 300 && !(*sent_mask & 0x04)) {
            synth_bms_fault(ctx, MC_BMS_FAULT_OVERTEMP);
            *sent_mask |= 0x04;
        }
        if (ctx->diag_extra_pt_fault && *script_ms >= 310 && !(*sent_mask & 0x80)) {
            synth_report_fault(ctx, MC_NODE_POWERTRAIN, MC_WARN_POWERTRAIN_OFFLINE, 1);
            *sent_mask |= 0x80;
        }
    }
    if (*script_ms >= 350 && !(*sent_mask & 0x08)) {
        synth_diag_request(ctx, MC_DIAG_ACTUATOR_TEST, 6);
        *sent_mask |= 0x08;
    }
    if (*script_ms >= 450 && !(*sent_mask & 0x10)) {
        synth_diag_request(ctx, MC_DIAG_READ_DTCS, 3);
        *sent_mask |= 0x10;
    }
    if (*script_ms >= 650 && !(*sent_mask & 0x20)) {
        synth_diag_request(ctx, MC_DIAG_CLEAR_DTCS, 4);
        *sent_mask |= 0x20;
    }
    if (*script_ms >= 750 && !(*sent_mask & 0x40)) {
        synth_diag_request(ctx, MC_DIAG_READ_DTCS, 5);
        *sent_mask |= 0x40;
    }
}

static void run_dogfood_diag_startdrive_script(gateway_ctx_t *ctx, uint32_t *script_ms, uint32_t *sent_mask)
{
    if (!ctx->diag_startdrive_script) return;

    *script_ms += 10;

    if (*script_ms >= 100 && !(*sent_mask & 0x01)) {
        ctx->gs.mode = VEHICLE_DRIVE;
        sim_trace_u32("vehicle_mode", (uint32_t)ctx->gs.mode);
        xEventGroupSetBits(ctx->mode_events, 0x01);
        synth_diag_request(ctx, MC_DIAG_START_SESSION, 1);
        *sent_mask |= 0x01;
    }
    if (*script_ms >= 200 && !(*sent_mask & 0x02)) {
        synth_diag_request(ctx, MC_DIAG_READ_MODE, 2);
        *sent_mask |= 0x02;
    }
}

static void run_dogfood_charging_script(gateway_ctx_t *ctx, uint32_t *script_ms, uint32_t *sent_mask)
{
    if (!ctx->charging_dogfood_script) return;

    *script_ms += 10;

    if (*script_ms >= 100 && !(*sent_mask & 0x01)) {
        ctx->gs.mode = VEHICLE_CHARGING;
        sim_trace_u32("charging_plug", 1);
        sim_trace_u32("gateway_charging_state", (uint32_t)ctx->gs.mode);
        sim_trace_u32("vehicle_mode", (uint32_t)ctx->gs.mode);
        xEventGroupSetBits(ctx->mode_events, 0x01);
        *sent_mask |= 0x01;
    }

    if (*script_ms >= 300 && !(*sent_mask & 0x02)) {
        gateway_state_enter_drive(&ctx->gs);
        uint8_t blocked = (ctx->gs.mode == VEHICLE_CHARGING) ? 1 : 0;
        sim_trace_u32("charging_drive_blocked", blocked);
        sim_trace_u32("gateway_charging_state", (uint32_t)ctx->gs.mode);
        *sent_mask |= 0x02;
    }
}

static void emit_ota_rollback(const mc_ota_slot_state_t *ota)
{
    sim_trace_u32("ota_state", ota->state);
    sim_trace_u32("ota_rollback", ota->rolled_back);
    sim_trace_u32("ota_active_slot", ota->active_slot);
    sim_trace_u32("ota_boot_result", 0);
}

static void run_dogfood_ota_script(gateway_ctx_t *ctx, uint32_t *ota_ms, uint32_t *ota_sent)
{
    if (!ctx->ota_dogfood_script) return;

    mc_ota_slot_state_t *ota = &ctx->ota;
    if (!ctx->ota_inited) {
        mc_ota_init(ota);
        ctx->ota_inited = 1;
    }

    int download_complete = (ctx->ota_fault_mode != OTA_FAULT_INTERRUPTED_WRITE);
    int crc_ok            = (ctx->ota_fault_mode != OTA_FAULT_BAD_CRC);
    int boot_healthy      = (ctx->ota_fault_mode != OTA_FAULT_BAD_HEALTH);

    if (ctx->ota_crc_check_bug) crc_ok = 1;

    *ota_ms += 10;

    if (*ota_ms >= 100 && !(*ota_sent & 0x01)) {
        sim_trace_u32("ota_state", ota->state);
        *ota_sent |= 0x01;
    }
    if (*ota_ms >= 200 && !(*ota_sent & 0x02)) {
        mc_ota_begin_download(ota);
        sim_trace_u32("ota_state", ota->state);
        if (!mc_ota_finish_download(ota, download_complete)) {
            emit_ota_rollback(ota);
        }
        *ota_sent |= 0x02;
    }
    if (*ota_ms >= 300 && !(*ota_sent & 0x04)) {
        if (ota->state == MC_OTA_DOWNLOADING) {
            sim_trace_u32("ota_state", MC_OTA_VERIFYING);
            int verified = mc_ota_verify(ota, crc_ok);
            sim_trace_u32("ota_crc_ok", ota->crc_ok);
            if (!verified) {
                emit_ota_rollback(ota);
            }
        }
        *ota_sent |= 0x04;
    }
    if (*ota_ms >= 400 && !(*ota_sent & 0x08)) {
        if (ota->state == MC_OTA_VERIFYING) {
            if (ctx->ota_fault_mode == OTA_FAULT_POWERCUT_PRECOMMIT) {
                mc_ota_rollback(ota);
                emit_ota_rollback(ota);
            } else {
                mc_ota_commit(ota);
                sim_trace_u32("ota_state", ota->state);
                sim_trace_u32("ota_slot", ota->target_slot);
            }
        }
        *ota_sent |= 0x08;
    }
    if (*ota_ms >= 500 && !(*ota_sent & 0x10)) {
        if (ota->state == MC_OTA_COMMIT_PENDING) {
            mc_ota_reboot(ota);
            sim_trace_u32("ota_state", ota->state);
        }
        *ota_sent |= 0x10;
    }
    if (*ota_ms >= 600 && !(*ota_sent & 0x20)) {
        if (ota->state == MC_OTA_REBOOTING) {
            if (mc_ota_health_check(ota, boot_healthy)) {
                sim_trace_u32("ota_state", ota->state);
                sim_trace_u32("ota_boot_result", ota->boot_healthy);
            } else {
                emit_ota_rollback(ota);
            }
        }
        *ota_sent |= 0x20;
    }
}

static void dispatch_frame_in_rx(gateway_ctx_t *ctx, const mc_can_frame_t *frame)
{
    hb_event_t ev;

    switch (frame->id) {
    case MC_MSG_HEARTBEAT:
        ev.sender    = frame->sender;
        ev.uptime_ms = ((uint32_t)frame->data[1] << 24)
                     | ((uint32_t)frame->data[2] << 16)
                     | ((uint32_t)frame->data[3] << 8)
                     | ((uint32_t)frame->data[4]);
        xQueueSend(ctx->hb_queue, &ev, 0);
        break;
    case MC_MSG_BMS_FAULT:
        handle_bms_fault(ctx, frame);
        break;
    case MC_MSG_DIAG_REQUEST:
        handle_diag_request(ctx, frame);
        break;
    default:
        break;
    }
}

static mc_vehicle_mode_t update_vehicle_mode(gateway_ctx_t *ctx, uint32_t now_ms)
{
    heartbeat_monitor_check(&ctx->hm, now_ms);

    uint8_t all_online      = heartbeat_monitor_all_online(&ctx->hm);
    uint8_t bms_fault       = 0;
    uint8_t bms_limp        = 0;
    uint8_t fault_count     = 0;

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        bms_fault   = fault_manager_has_critical(&ctx->fm);
        fault_count = fault_manager_active_count(&ctx->fm);
        xSemaphoreGive(ctx->fm_mutex);
    }

    if (!bms_fault && ctx->diag_session_active) {
        ctx->gs.all_nodes_online = all_online;
        ctx->gs.bms_fault_active = bms_fault;
        ctx->gs.bms_limp_requested = bms_limp;
        ctx->gs.active_fault_count = fault_count;
        ctx->gs.mode = VEHICLE_SERVICE;
        return ctx->gs.mode;
    }

    return gateway_state_update(&ctx->gs, all_online, bms_fault,
                                bms_limp, fault_count);
}

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

static void send_vehicle_mode(gateway_ctx_t *ctx, mc_can_frame_t *tx)
{
    mc_vehicle_mode_t mode     = ctx->gs.mode;
    uint8_t           fault_cd = 0;

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_cd = ctx->fm.critical_count > 0 ? 1 : 0;
        xSemaphoreGive(ctx->fm_mutex);
    }

    mc_frame_init(tx, MC_MSG_VEHICLE_MODE, MC_NODE_GATEWAY,
                  MC_VEHICLE_MODE_MSG_SIZE);
    tx->data[0] = (uint8_t)mode;
    tx->data[1] = fault_cd;
}

// ── Heartbeat RX task ─────────────────────────────────────────────────────

void heartbeat_rx(void *pvParameters)
{
    (void)pvParameters;
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx == NULL) { vTaskSuspend(NULL); return; }

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
            dispatch_frame_in_rx(ctx, &rx);

            xSemaphoreGive(ctx->can_frame_sem);
        }
    }
}

// ── Fault aggregator task ──────────────────────────────────────────────────

void fault_aggregator(void *pvParameters)
{
    (void)pvParameters;
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx == NULL) { vTaskSuspend(NULL); return; }

    TickType_t last_wake = xTaskGetTickCount();

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(100));

        uint8_t critical = 0;
        uint8_t active   = 0;
        uint8_t warning  = 0;

        if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(10)) == pdTRUE) {
            critical = ctx->fm.critical_count;
            warning  = ctx->fm.warning_count;
            active   = fault_manager_active_count(&ctx->fm);
            xSemaphoreGive(ctx->fm_mutex);
        }

        uint32_t agg = ((uint32_t)critical << 16) | ((uint32_t)warning << 8) | active;
        sim_trace_u32("fault_aggregate", agg);

        if (critical > 0) {
            xEventGroupSetBits(ctx->mode_events, 0x02);
        }
    }
}

// ── CAN frame processor task ──────────────────────────────────────────

void can_frame_processor(void *pvParameters)
{
    (void)pvParameters;
    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx == NULL) { vTaskSuspend(NULL); return; }

    uint32_t proc_count = 0;

    while (1) {
        xSemaphoreTake(ctx->can_frame_sem, portMAX_DELAY);

        proc_count++;

        sim_trace_u32("can_frame_proc", proc_count);
    }
}

// ── Main loop ─────────────────────────────────────────────────────────────

void gateway_main(void *pvParameters)
{
    (void)pvParameters;

    gateway_ctx_t *ctx = gateway_ctx();
    if (ctx == NULL) {
        sim_trace_u32("gateway_fatal", 1);
        vTaskSuspend(NULL);
        return;
    }

    gateway_init(ctx);
    gateway_primitives_init(ctx);
    ctx->gateway_task_handle = xTaskGetCurrentTaskHandle();

    // Create subordinate tasks.
    xTaskCreate(heartbeat_rx, "hb_rx", 768, NULL, 4, NULL);
    xTaskCreate(fault_aggregator, "fault_agg", 512, NULL, 2, NULL);
    xTaskCreate(can_frame_processor, "can_proc", 768, NULL, 5, NULL);

    TickType_t last_wake = xTaskGetTickCount();
    mc_can_frame_t tx;

    // Send initial heartbeat at boot.
    send_heartbeat(0, &tx);
    sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);

    while (1) {
        vTaskDelayUntil(&last_wake, pdMS_TO_TICKS(10));
        uint32_t now_ms = xTaskGetTickCount() * portTICK_PERIOD_MS;

        // ── Dequeue heartbeat events ────────────────────────────
        hb_event_t ev;
        while (xQueueReceive(ctx->hb_queue, &ev, 0) == pdTRUE) {
            handle_heartbeat(ctx, now_ms, &ev);
        }

        run_dogfood_diag_script(ctx, &ctx->diag_script_ms, &ctx->diag_script_sent);
        run_dogfood_diag_startdrive_script(ctx, &ctx->startdrive_script_ms, &ctx->startdrive_script_sent);
        run_dogfood_charging_script(ctx, &ctx->charging_script_ms, &ctx->charging_script_sent);
        run_dogfood_ota_script(ctx, &ctx->ota_script_ms, &ctx->ota_script_sent);

        // ── Check for urgent fault notifications ────────────────
        uint32_t notify_val = 0;
        if (xTaskNotifyWait(0, 0xFFFFFFFF, &notify_val, 0) == pdTRUE) {
            sim_trace_u32("urgent_fault_notify", notify_val);
        }

        // ── Process phase: check heartbeats for timeouts ────────
        int transitions = heartbeat_monitor_check(&ctx->hm, now_ms);
        if (transitions > 0) {
            uint8_t lost_node = heartbeat_monitor_last_transition_node(&ctx->hm);
            if (lost_node == MC_NODE_BMS) {
                if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
                    fault_manager_report(&ctx->fm, MC_NODE_BMS,
                                         MC_BMS_FAULT_COMM_ERROR, 2);
                    xSemaphoreGive(ctx->fm_mutex);
                }
            }
        }

        // ── Mode update ─────────────────────────────────────────
        mc_vehicle_mode_t old_mode = ctx->gs.mode;
        mc_vehicle_mode_t new_mode = update_vehicle_mode(ctx, now_ms);

        if (new_mode != old_mode) {
            sim_trace_u32("vehicle_mode", (uint32_t)new_mode);
            xEventGroupSetBits(ctx->mode_events, 0x01);
        }

        // ── Check event group for mode transitions ──────────────
        EventBits_t mode_bits = xEventGroupGetBits(ctx->mode_events);
        if (mode_bits & 0x01) {
            sim_trace_u32("mode_event_group", mode_bits);
            xEventGroupClearBits(ctx->mode_events, 0x01);
        }

        // ── Broadcast phase ─────────────────────────────────────
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }

        if (new_mode != old_mode || now_ms % 50 == 0) {
            send_vehicle_mode(ctx, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
