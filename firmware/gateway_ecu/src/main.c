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
// B2: All mutable state migrated to sim_instance_state(0x4D430001, ...) so
// the gateway can run concurrently in multiple in-process World instances
// with independent device banks (UNBLOCKING.md §B2).

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
#include "microcar_safety.h"
#include "microcar_trace.h"
#include "microcar_can.h"
#include "microcar_ota_slot.h"
#include "microcar_charging.h"
#include "sim_abi.h"
#include <string.h>
#include <stdalign.h>

// ── Instance key — allocated once per machine via sim_instance_state ──────
#define GW_KEY 0x4D430001

/// Block device ID for OTA metadata flash (2×256-byte pages: copy A, copy B).
#define MC_OTA_FLASH_BLK_ID 10

/// Reflected CRC-32 lookup table (poly 0xEDB88320) for incremental update.
static const uint32_t MC_OTA_CRC_TABLE[256] = {
    0x00000000u, 0x77073096u, 0xEE0E612Cu, 0x990951BAu,
    0x076DC419u, 0x706AF48Fu, 0xE963A535u, 0x9E6495A3u,
    0x0EDB8832u, 0x79DCB8A4u, 0xE0D5E91Eu, 0x97D2D988u,
    0x09B64C2Bu, 0x7EB17CBDu, 0xE7B82D07u, 0x90BF1D91u,
    0x1DB71064u, 0x6AB020F2u, 0xF3B97148u, 0x84BE41DEu,
    0x1ADAD47Du, 0x6DDDE4EBu, 0xF4D4B551u, 0x83D385C7u,
    0x136C9856u, 0x646BA8C0u, 0xFD62F97Au, 0x8A65C9ECu,
    0x14015C4Fu, 0x63066CD9u, 0xFA0F3D63u, 0x8D080DF5u,
    0x3B6E20C8u, 0x4C69105Eu, 0xD56041E4u, 0xA2677172u,
    0x3C03E4D1u, 0x4B04D447u, 0xD20D85FDu, 0xA50AB56Bu,
    0x35B5A8FAu, 0x42B2986Cu, 0xDBBBC9D6u, 0xACBCF940u,
    0x32D86CE3u, 0x45DF5C75u, 0xDCD60DCFu, 0xABD13D59u,
    0x26D930ACu, 0x51DE003Au, 0xC8D75180u, 0xBFD06116u,
    0x21B4F4B5u, 0x56B3C423u, 0xCFBA9599u, 0xB8BDA50Fu,
    0x2802B89Eu, 0x5F058808u, 0xC60CD9B2u, 0xB10BE924u,
    0x2F6F7C87u, 0x58684C11u, 0xC1611DABu, 0xB6662D3Du,
    0x76DC4190u, 0x01DB7106u, 0x98D220BCu, 0xEFD5102Au,
    0x71B18589u, 0x06B6B51Fu, 0x9FBFE4A5u, 0xE8B8D433u,
    0x7807C9A2u, 0x0F00F934u, 0x9609A88Eu, 0xE10E9818u,
    0x7F6A0DBBu, 0x086D3D2Du, 0x91646C97u, 0xE6635C01u,
    0x6B6B51F4u, 0x1C6C6162u, 0x856530D8u, 0xF262004Eu,
    0x6C0695EDu, 0x1B01A57Bu, 0x8208F4C1u, 0xF50FC457u,
    0x65B0D9C6u, 0x12B7E950u, 0x8BBEB8EAu, 0xFCB9887Cu,
    0x62DD1DDFu, 0x15DA2D49u, 0x8CD37CF3u, 0xFBD44C65u,
    0x4DB26158u, 0x3AB551CEu, 0xA3BC0074u, 0xD4BB30E2u,
    0x4ADFA541u, 0x3DD895D7u, 0xA4D1C46Du, 0xD3D6F4FBu,
    0x4369E96Au, 0x346ED9FCu, 0xAD678846u, 0xDA60B8D0u,
    0x44042D73u, 0x33031DE5u, 0xAA0A4C5Fu, 0xDD0D7CC9u,
    0x5005713Cu, 0x270241AAu, 0xBE0B1010u, 0xC90C2086u,
    0x5768B525u, 0x206F85B3u, 0xB966D409u, 0xCE61E49Fu,
    0x5EDEF90Eu, 0x29D9C998u, 0xB0D09822u, 0xC7D7A8B4u,
    0x59B33D17u, 0x2EB40D81u, 0xB7BD5C3Bu, 0xC0BA6CADu,
    0xEDB88320u, 0x9ABFB3B6u, 0x03B6E20Cu, 0x74B1D29Au,
    0xEAD54739u, 0x9DD277AFu, 0x04DB2615u, 0x73DC1683u,
    0xE3630B12u, 0x94643B84u, 0x0D6D6A3Eu, 0x7A6A5AA8u,
    0xE40ECF0Bu, 0x9309FF9Du, 0x0A00AE27u, 0x7D079EB1u,
    0xF00F9344u, 0x8708A3D2u, 0x1E01F268u, 0x6906C2FEu,
    0xF762575Du, 0x806567CBu, 0x196C3671u, 0x6E6B06E7u,
    0xFED41B76u, 0x89D32BE0u, 0x10DA7A5Au, 0x67DD4ACCu,
    0xF9B9DF6Fu, 0x8EBEEFF9u, 0x17B7BE43u, 0x60B08ED5u,
    0xD6D6A3E8u, 0xA1D1937Eu, 0x38D8C2C4u, 0x4FDFF252u,
    0xD1BB67F1u, 0xA6BC5767u, 0x3FB506DDu, 0x48B2364Bu,
    0xD80D2BDAu, 0xAF0A1B4Cu, 0x36034AF6u, 0x41047A60u,
    0xDF60EFC3u, 0xA867DF55u, 0x316E8EEFu, 0x4669BE79u,
    0xCB61B38Cu, 0xBC66831Au, 0x256FD2A0u, 0x5268E236u,
    0xCC0C7795u, 0xBB0B4703u, 0x220216B9u, 0x5505262Fu,
    0xC5BA3BBEu, 0xB2BD0B28u, 0x2BB45A92u, 0x5CB30A04u,
    0xC2D7FFA7u, 0xB5D0CF31u, 0x2CD99E8Bu, 0x5BDEAE1Du,
    0x9B64C2B0u, 0xEC63F226u, 0x756AA39Cu, 0x026D930Au,
    0x9C0906A9u, 0xEB0E363Fu, 0x72076785u, 0x05005713u,
    0x95BF4A82u, 0xE2B87A14u, 0x7BB12BAEu, 0x0CB61B38u,
    0x92D28E9Bu, 0xE5D5BE0Du, 0x7CDCEFB7u, 0x0BDBDF21u,
    0x86D3D2D4u, 0xF1D4E242u, 0x68DDB3F8u, 0x1FDA836Eu,
    0x81BE16CDu, 0xF6B9265Bu, 0x6FB077E1u, 0x18B74777u,
    0x88085AE6u, 0xFF0F6A70u, 0x66063BCAu, 0x11010B5Cu,
    0x8F659EFFu, 0xF862AE69u, 0x616BFFD3u, 0x166CCF45u,
    0xA00AE278u, 0xD70DD2EEu, 0x4E048354u, 0x3903B3C2u,
    0xA7672661u, 0xD06016F7u, 0x4969474Du, 0x3E6E77DBu,
    0xAED16A4Au, 0xD9D65ADCu, 0x40DF0B66u, 0x37D83BF0u,
    0xA9BCAE53u, 0xDEBB9EC5u, 0x47B2CF7Fu, 0x30B5FFE9u,
    0xBDBDF21Cu, 0xCABAC28Au, 0x53B39330u, 0x24B4A3A6u,
    0xBAD03605u, 0xCDD70693u, 0x54DE5729u, 0x23D967BFu,
    0xB3667A2Eu, 0xC4614AB8u, 0x5D681B02u, 0x2A6F2B94u,
    0xB40BBE37u, 0xC30C8EA1u, 0x5A05DF1Bu, 0x2D02EF8Du,
};
// ── Heartbeat queue item ───────────────────────────────────────────────────

/// A single heartbeat event pushed from heartbeat_rx onto the queue.
typedef struct {
    uint8_t  sender;
    uint32_t uptime_ms;
} hb_event_t;

// ── OTA frame queue item ────────────────────────────────────────────────────

/// OTA frame types forwarded from heartbeat_rx to gateway_main.
typedef enum {
    OTA_EVT_REQUEST = 0,
    OTA_EVT_CHUNK   = 1,
    OTA_EVT_FINISH  = 2,
} ota_evt_type_t;

/// A single OTA event pushed from heartbeat_rx onto the ota_req_queue.
typedef struct {
    ota_evt_type_t type;
    union {
        mc_ota_request_msg_t request;
        mc_ota_chunk_msg_t   chunk;
        mc_ota_finish_msg_t  finish;
    } payload;
} ota_evt_t;

// ── Charging event queue item (Stage E2) ─────────────────────────────────

/// A charging event pushed from heartbeat_rx onto the charge_event_queue,
/// either from an EVSE_EVENT frame or a BMS_CHARGE_LIMIT update.
typedef struct {
    mc_charging_event_t event;
    uint8_t             request_id;   // EVSE request_id to echo in CHARGE_COMMAND
} charge_evt_t;

// ── Per-instance context (replaces all file-scope statics) ────────────────

typedef struct {
    // Core state
    gateway_state_t     gs;
    heartbeat_monitor_t hm;
    fault_manager_t     fm;

    // FreeRTOS primitives
    SemaphoreHandle_t   fm_mutex;
    EventGroupHandle_t  mode_events;
    QueueHandle_t       hb_queue;
    SemaphoreHandle_t   can_frame_sem;

    // BMS status snapshot cache (Stage D: diagnostics live data)
    mc_bms_status_msg_t bms_snapshot;
    uint8_t             bms_snapshot_seq;
    uint32_t            bms_snapshot_time_ms;
    uint8_t             bms_snapshot_valid;

    // Diagnostics request queue (heartbeat_rx → gateway_main)
    QueueHandle_t       diag_req_queue;

    // ── OTA update state (Stage F2) ────────────────────────────
    mc_ota_slot_state_t ota_slot;          // A/B slot state machine
    uint8_t             ota_active;        // 1 = update in progress
    uint8_t             ota_expected_chunks; // total chunks from OTA_REQUEST
    uint8_t             ota_chunks_received; // count of chunks written
    uint8_t             ota_next_chunk_idx;  // expected next chunk index
    uint8_t             ota_request_id;      // active request id
    uint32_t            ota_image_crc;       // running CRC32 of accumulated image data
    uint32_t            ota_image_length;     // expected image length in bytes
    uint8_t             ota_reboot_pending;  // 1 = reboot requested
    QueueHandle_t       ota_req_queue;       // OTA frame queue (heartbeat_rx → gateway_main)
    // ── Charging FSM state (Stage E2) ───────────────────────────
    uint8_t             charge_state;        // mc_charge_state_t, via mc_charging_step()
    uint8_t             bms_charge_limit;     // cached BMS current limit (0.5A units)
    uint8_t             bms_charge_limit_valid; // 1 = fresh limit received
    uint8_t             bms_charge_limit_soc; // SOC from BMS_CHARGE_LIMIT (0-100)
    uint8_t             last_evse_request_id; // EVSE request_id for CHARGE_COMMAND echo
    QueueHandle_t       charge_event_queue;   // charging event queue

    // Task handle for gateway_main (receives task notifications)
    TaskHandle_t        gateway_task_handle;
 } gateway_ctx_t;

/// Return the per-machine gateway context, allocating it on first call.
static gateway_ctx_t *gateway_ctx(void)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)sim_instance_state(
        GW_KEY, sizeof(gateway_ctx_t), alignof(gateway_ctx_t));
    return ctx;
}

// ── Boot ──────────────────────────────────────────────────────────────────

static void gateway_primitives_init(gateway_ctx_t *ctx)
{
    ctx->fm_mutex           = xSemaphoreCreateMutex();
    ctx->mode_events        = xEventGroupCreate();
    ctx->diag_req_queue     = xQueueCreate(8, sizeof(mc_diag_request_msg_t));
    ctx->ota_req_queue      = xQueueCreate(8, sizeof(ota_evt_t));
    ctx->charge_event_queue = xQueueCreate(8, sizeof(charge_evt_t));
    ctx->can_frame_sem      = xSemaphoreCreateCounting(64, 0);

    sim_trace_u32("gateway_mutex", ctx->fm_mutex != NULL ? 1 : 0);
    sim_trace_u32("gateway_event_group", ctx->mode_events != NULL ? 1 : 0);
    sim_trace_u32("gateway_queue", ctx->hb_queue != NULL ? 1 : 0);
    sim_trace_u32("gateway_can_sem", ctx->can_frame_sem != NULL ? 1 : 0);
}

void gateway_init(void)
{
    gateway_ctx_t *ctx = gateway_ctx();

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

    // ── OTA boot recovery (Stage F2) ────────────────────────────
    mc_ota_init(&ctx->ota_slot);
    ctx->ota_active          = 0;
    ctx->ota_reboot_pending  = 0;
    mc_charging_init((mc_charge_state_t *)&ctx->charge_state);
    ctx->bms_charge_limit       = 0;
    ctx->bms_charge_limit_valid = 0;
    ctx->bms_charge_limit_soc   = 0;
    ctx->last_evse_request_id   = 0;

    // Try to read persistent metadata from flash.
    // Block device 10: 2 pages for metadata copies (page 0 = copy A, page 1 = copy B).
    uint32_t blk_result = sim_block_create(MC_OTA_FLASH_BLK_ID, 256, 2, 0xFF);
    if (blk_result == 0) {
        mc_ota_meta_record_t rec_a, rec_b;
        uint32_t r = sim_block_read(MC_OTA_FLASH_BLK_ID, 0,
                                    (uint8_t *)&rec_a, sizeof(rec_a));
        uint32_t r2 = sim_block_read(MC_OTA_FLASH_BLK_ID, 256,
                                     (uint8_t *)&rec_b, sizeof(rec_b));
        if (r == sizeof(rec_a) && r2 == sizeof(rec_b)) {
            const mc_ota_meta_record_t *chosen = mc_ota_select_record(&rec_a, &rec_b);
            if (chosen) {
                mc_ota_recover_after_reset(&ctx->ota_slot, chosen);
                // If we were in the middle of a reboot cycle, finish it.
                if (ctx->ota_slot.state == MC_OTA_REBOOTING) {
                    // Assume healthy boot for now; a real bootloader would run self-test.
                    mc_ota_health_check(&ctx->ota_slot, 1);
                    sim_trace_u32("ota_boot_recovered", ctx->ota_slot.state);
                }
            }
        }
    }
}

// ── Message handlers ──────────────────────────────────────────────────────

/// Process a heartbeat frame (0x001) from any node.
/// Called from gateway_main after dequeuing an hb_event_t.
static void handle_heartbeat(gateway_ctx_t *ctx, uint32_t now_ms,
                             const hb_event_t *ev)
{
    heartbeat_monitor_beat(&ctx->hm, ev->sender, now_ms);
    (void)ev->uptime_ms;
}

/// Process a BMS fault frame (0x202).
/// Protected by mutex.
static void handle_bms_fault(gateway_ctx_t *ctx, const mc_can_frame_t *frame)
{
    uint8_t fault_code = frame->data[0];
    uint8_t severity   = fault_manager_bms_severity(fault_code);

    if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
        fault_manager_report(&ctx->fm, MC_NODE_BMS, fault_code, severity);

        // If critical fault, notify gateway_main immediately.
        if (severity == 2 && ctx->gateway_task_handle != NULL) {
            xTaskNotify(ctx->gateway_task_handle,
                        (uint32_t)fault_code,
                        eSetValueWithoutOverwrite);
        }

        xSemaphoreGive(ctx->fm_mutex);
    }
}

// Dispatch a received CAN frame to the appropriate handler.
// Called from heartbeat_rx task.
static void dispatch_frame_in_rx(gateway_ctx_t *ctx,
                                 const mc_can_frame_t *frame)
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
            xQueueSend(ctx->hb_queue, &ev, 0);
        }
        break;
    case MC_MSG_BMS_FAULT:
        handle_bms_fault(ctx, frame);
        break;
    case MC_MSG_BMS_STATUS:
        // Stage D: cache BMS status snapshot with timestamp.
        memcpy(&ctx->bms_snapshot, frame->data, sizeof(mc_bms_status_msg_t));
        ctx->bms_snapshot_seq      = frame->data[7];
        ctx->bms_snapshot_time_ms  = xTaskGetTickCount() * portTICK_PERIOD_MS;
        ctx->bms_snapshot_valid    = 1;
        break;
    case MC_MSG_DIAG_REQUEST:
        // Forward to gateway_main via queue.
        if (frame->len >= sizeof(mc_diag_request_msg_t)) {
            mc_diag_request_msg_t req;
            memcpy(&req, frame->data, sizeof(mc_diag_request_msg_t));
            xQueueSend(ctx->diag_req_queue, &req, 0);
        }
        break;
    case MC_MSG_OTA_REQUEST:
        if (frame->len >= sizeof(mc_ota_request_msg_t)) {
            ota_evt_t evt;
            evt.type = OTA_EVT_REQUEST;
            memcpy(&evt.payload.request, frame->data, sizeof(mc_ota_request_msg_t));
            xQueueSend(ctx->ota_req_queue, &evt, 0);
        }
        break;
    case MC_MSG_OTA_CHUNK:
        if (frame->len >= sizeof(mc_ota_chunk_msg_t)) {
            ota_evt_t evt;
            evt.type = OTA_EVT_CHUNK;
            memcpy(&evt.payload.chunk, frame->data, sizeof(mc_ota_chunk_msg_t));
            xQueueSend(ctx->ota_req_queue, &evt, 0);
        }
        break;
    case MC_MSG_OTA_FINISH:
        if (frame->len >= sizeof(mc_ota_finish_msg_t)) {
            ota_evt_t evt;
            evt.type = OTA_EVT_FINISH;
            memcpy(&evt.payload.finish, frame->data, sizeof(mc_ota_finish_msg_t));
            xQueueSend(ctx->ota_req_queue, &evt, 0);
        }
        break;
    case MC_MSG_EVSE_EVENT:
        // Stage E2: queue a charging event for gateway_main to process.
        if (frame->len >= sizeof(mc_evse_event_msg_t)) {
            mc_evse_event_msg_t evse;
            memcpy(&evse, frame->data, sizeof(evse));
            charge_evt_t cevt;
            cevt.event.kind            = (mc_evse_event_t)evse.event;
            cevt.event.offered_current = evse.offered_current_a_x2;
            cevt.event.target_soc      = evse.target_soc;
            cevt.event.fresh_bms_limit = ctx->bms_charge_limit_valid
                                           ? ctx->bms_charge_limit : 0;
            cevt.event.soc_percent     = ctx->bms_charge_limit_soc;
            cevt.request_id            = evse.request_id;
            cevt.event.critical_fault  = 0;
            if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
                cevt.event.critical_fault =
                    fault_manager_has_critical(&ctx->fm) ? 1 : 0;
                xSemaphoreGive(ctx->fm_mutex);
            }
            ctx->last_evse_request_id = evse.request_id;
            xQueueSend(ctx->charge_event_queue, &cevt, 0);
        }
        break;
    case MC_MSG_BMS_CHARGE_LIMIT:
        // Stage E2: cache the limit and queue a step event.
        if (frame->len >= sizeof(mc_bms_charge_limit_msg_t)) {
            mc_bms_charge_limit_msg_t blim;
            memcpy(&blim, frame->data, sizeof(blim));
            ctx->bms_charge_limit       = blim.max_current_a_x2;
            ctx->bms_charge_limit_soc   = blim.soc_percent;
            ctx->bms_charge_limit_valid = 1;
            charge_evt_t cevt;
            cevt.event.kind            = MC_EVSE_STOP; // sentinel: BMS update only
            cevt.event.offered_current = 0;
            cevt.event.target_soc      = 0;
            cevt.event.fresh_bms_limit = blim.max_current_a_x2;
            cevt.event.soc_percent     = blim.soc_percent;
            cevt.event.critical_fault  = 0;
            cevt.request_id            = 0;
            if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
                cevt.event.critical_fault =
                    fault_manager_has_critical(&ctx->fm) ? 1 : 0;
                xSemaphoreGive(ctx->fm_mutex);
            }
            xQueueSend(ctx->charge_event_queue, &cevt, 0);
        }
        break;
     default:
         break;
    }
}

// ── Mode determination ────────────────────────────────────────────────────

/// Re-evaluate vehicle mode based on current state.
/// Protected by mutex for fault_manager access.
static mc_vehicle_mode_t update_vehicle_mode(gateway_ctx_t *ctx,
                                             uint32_t now_ms)
{
    // Check timeouts → update online/offline status.
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

    return gateway_state_update(&ctx->gs, all_online, bms_fault,
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

/// Build and send the charge command frame (0x611).
static void send_charge_command(gateway_ctx_t *ctx,
                                const mc_charging_output_t *out,
                                uint8_t target_soc,
                                mc_can_frame_t *tx)
{
    mc_charge_command_msg_t msg = {
        .source_node    = MC_NODE_GATEWAY,
        .state          = (uint8_t)out->next_state,
        .request_id     = ctx->last_evse_request_id,
        .current_a_x2   = out->command_current_a_x2,
        .target_soc     = target_soc,
        .reason         = out->reject_reason,
    };
    mc_frame_init(tx, MC_MSG_CHARGE_COMMAND, MC_NODE_GATEWAY,
                  sizeof(mc_charge_command_msg_t));
    memcpy(tx->data, &msg, sizeof(msg));
    sim_can_send(0, tx->id, tx->data, tx->len, 0, 0);
}

// Handle a diagnostics request from the diagnostics tool.
// Responds with MC_DIAG_LIVE_BMS selector-based encoding using the
// cached BMS status snapshot.  If the snapshot is missing or older
// than 500ms, responds with MC_DIAG_STALE.
static void handle_diag_request(gateway_ctx_t *ctx, uint32_t now_ms,
                                const mc_diag_request_msg_t *req,
                                mc_can_frame_t *tx)
{
    mc_diag_response_msg_t resp = {
        .source_node = MC_NODE_GATEWAY,
        .service     = req->service,
        .request_id  = req->request_id,
        .status      = MC_DIAG_OK,
        .value0      = 0,
        .value1      = 0,
    };

    if (req->service != MC_DIAG_LIVE_BMS) {
        resp.status = MC_DIAG_UNSUPPORTED;
    } else if (!ctx->bms_snapshot_valid ||
               (now_ms - ctx->bms_snapshot_time_ms) > 500) {
        resp.status = MC_DIAG_STALE;
    } else {
        switch (req->param) {
        case 0:
            // selector 0: soc_percent, temp_c + 40
            resp.value0 = ctx->bms_snapshot.soc_percent;
            resp.value1 = (uint8_t)((ctx->bms_snapshot.pack_temp_c_x10 / 10) + 40);
            break;
        case 1: {
            // selector 1: pack voltage in 100 mV, little-endian u16
            uint16_t volts_cmv = ctx->bms_snapshot.pack_voltage_mv / 100;
            resp.value0 = (uint8_t)(volts_cmv & 0xFF);
            resp.value1 = (uint8_t)(volts_cmv >> 8);
            break;
        }
        case 2: {
            // selector 2: pack current in 100 mA, little-endian i16
            int16_t current_cma = ctx->bms_snapshot.pack_current_ma / 100;
            resp.value0 = (uint8_t)(current_cma & 0xFF);
            resp.value1 = (uint8_t)(current_cma >> 8);
            break;
        }
        default:
            resp.status = MC_DIAG_UNSUPPORTED;
            break;
        }
    }

    mc_frame_init(tx, MC_MSG_DIAG_RESPONSE, MC_NODE_GATEWAY,
                  MC_DIAG_RESPONSE_MSG_SIZE);
    memcpy(tx->data, &resp, sizeof(resp));
    sim_can_send(0, tx->id, tx->data, tx->len, 0, 0);
}

// ── OTA message handlers (Stage F2) ─────────────────────────────────────────

/// Build and send an OTA_STATUS response frame.
static void send_ota_status(gateway_ctx_t *ctx, uint8_t request_id,
                            uint8_t state, uint8_t status,
                            uint8_t reason, mc_can_frame_t *tx)
{
    mc_ota_status_msg_t msg = {
        .source_node = MC_NODE_GATEWAY,
        .request_id  = request_id,
        .state       = state,
        .status      = status,
        .active_slot = ctx->ota_slot.active_slot,
        .target_slot = ctx->ota_slot.target_slot,
        .reason      = reason,
        .seq         = ctx->ota_chunks_received,
    };
    mc_frame_init(tx, MC_MSG_OTA_STATUS, MC_NODE_GATEWAY,
                  sizeof(mc_ota_status_msg_t));
    memcpy(tx->data, &msg, sizeof(msg));
    sim_can_send(0, tx->id, tx->data, tx->len, 0, 0);
}

/// Handle OTA_REQUEST: admission check, begin download, or reject.
static void handle_ota_request(gateway_ctx_t *ctx, uint32_t now_ms,
                               const mc_ota_request_msg_t *req,
                               mc_can_frame_t *tx)
{
    uint8_t reject_reason = 0;
    uint8_t admitted = 0;

    // Admission checks:
    // 1. mode != DRIVE
    // 2. charging == DISCONNECTED
    // 3. no critical BMS fault
    // 4. BMS status ≤500ms old
    // 5. no active update
    if (ctx->gs.mode == VEHICLE_DRIVE) {
        reject_reason = 1; // vehicle in drive
    } else if (ctx->charge_state != MC_CHARGE_DISCONNECTED) {
        reject_reason = 2; // charger connected
    } else {
        uint8_t has_critical = 0;
        if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
            has_critical = fault_manager_has_critical(&ctx->fm);
            xSemaphoreGive(ctx->fm_mutex);
        }
        if (has_critical) {
            reject_reason = 3; // critical BMS fault
        } else if (!ctx->bms_snapshot_valid ||
                   (now_ms - ctx->bms_snapshot_time_ms) > 500) {
            reject_reason = 4; // BMS data stale
        } else if (ctx->ota_active) {
            reject_reason = 5; // update already in progress
        } else {
            admitted = 1;
        }
    }

    if (admitted) {
        mc_ota_begin_download(&ctx->ota_slot);
        ctx->ota_active           = 1;
        ctx->ota_expected_chunks  = req->total_chunks;
        ctx->ota_chunks_received  = 0;
        ctx->ota_next_chunk_idx   = 0;
        ctx->ota_request_id       = req->request_id;
        ctx->ota_image_crc        = 0xFFFFFFFFu;
        ctx->ota_image_length     = (uint32_t)req->total_chunks * 5;

        // Enter OTA_UPDATE mode.
        ctx->gs.mode = VEHICLE_OTA_UPDATE;
        sim_trace_u32("ota_begin", req->total_chunks);
        sim_trace_u32("vehicle_mode", (uint32_t)VEHICLE_OTA_UPDATE);

        send_ota_status(ctx, req->request_id, ctx->ota_slot.state,
                        0 /* accepted */, reject_reason, tx);
    } else {
        sim_trace_u32("ota_rejected", reject_reason);
        send_ota_status(ctx, req->request_id, MC_OTA_IDLE,
                        1 /* rejected */, reject_reason, tx);
    }
}

/// Handle OTA_CHUNK: sequential index check, accumulate data CRC, track progress.
static void handle_ota_chunk(gateway_ctx_t *ctx,
                             const mc_ota_chunk_msg_t *chunk,
                             mc_can_frame_t *tx)
{
    if (!ctx->ota_active || ctx->ota_slot.state != MC_OTA_DOWNLOADING) {
        send_ota_status(ctx, chunk->request_id, ctx->ota_slot.state,
                        2 /* not in download */, 1, tx);
        return;
    }

    if (chunk->request_id != ctx->ota_request_id) {
        send_ota_status(ctx, chunk->request_id, ctx->ota_slot.state,
                        3 /* wrong request */, 2, tx);
        return;
    }

    if (chunk->chunk_index != ctx->ota_next_chunk_idx) {
        sim_trace_u32("ota_chunk_seq_err",
                      ((uint32_t)chunk->chunk_index << 8) | ctx->ota_next_chunk_idx);
        send_ota_status(ctx, chunk->request_id, ctx->ota_slot.state,
                        4 /* seq error */, 3, tx);
        return;
    }

    // Accumulate incremental reflected CRC-32 (poly 0xEDB88320).
    // ctx->ota_image_crc holds the running CRC; finalised by XOR with 0xFFFFFFFF.
    for (int i = 0; i < 5; i++) {
        uint32_t byte_val = chunk->data[i];
        uint32_t idx = ((ctx->ota_image_crc ^ byte_val) & 0xFFu);
        ctx->ota_image_crc = (ctx->ota_image_crc >> 8) ^ MC_OTA_CRC_TABLE[idx];
    }

    ctx->ota_chunks_received++;
    ctx->ota_next_chunk_idx++;

    sim_trace_u32("ota_chunk", ((uint32_t)chunk->chunk_index << 16) | ctx->ota_chunks_received);

    send_ota_status(ctx, chunk->request_id, ctx->ota_slot.state,
                    0 /* progress */, 0, tx);
}

/// Handle OTA_FINISH: verify total chunks, verify CRC32, commit, persist, reboot.
static void handle_ota_finish(gateway_ctx_t *ctx,
                              const mc_ota_finish_msg_t *finish,
                              mc_can_frame_t *tx)
{
    if (!ctx->ota_active || ctx->ota_slot.state != MC_OTA_DOWNLOADING) {
        send_ota_status(ctx, finish->request_id, ctx->ota_slot.state,
                        2 /* not in download */, 1, tx);
        return;
    }

    if (finish->request_id != ctx->ota_request_id) {
        send_ota_status(ctx, finish->request_id, ctx->ota_slot.state,
                        3 /* wrong request */, 2, tx);
        return;
    }

    // Verify total chunks.
    if (ctx->ota_chunks_received != finish->total_chunks ||
        ctx->ota_chunks_received != ctx->ota_expected_chunks) {
        mc_ota_abort(&ctx->ota_slot, 10);
        ctx->ota_active = 0;
        sim_trace_u32("ota_chunk_count_mismatch",
                      ((uint32_t)ctx->ota_chunks_received << 16) | finish->total_chunks);
        send_ota_status(ctx, finish->request_id, ctx->ota_slot.state,
                        5 /* chunk count mismatch */, 10, tx);
        return;
    }

    // Finalise running CRC32.
    uint32_t computed_crc = ctx->ota_image_crc ^ 0xFFFFFFFFu;
    int crc_ok = (computed_crc == finish->crc32) ? 1 : 0;

    sim_trace_u32("ota_crc_computed", computed_crc);
    sim_trace_u32("ota_crc_expected", finish->crc32);

    // Phase 1: finish download into the slot model.
    mc_ota_finish_download(&ctx->ota_slot, 1);

    // Phase 2: verify CRC.
    mc_ota_verify(&ctx->ota_slot, crc_ok);
    if (!crc_ok) {
        ctx->ota_active = 0;
        sim_trace_u32("ota_crc_fail", 1);
        send_ota_status(ctx, finish->request_id, ctx->ota_slot.state,
                        6 /* CRC mismatch */, 11, tx);
        return;
    }

    // Phase 3: commit.
    mc_ota_commit(&ctx->ota_slot);

    // Phase 4: persist metadata record to flash (both copies).
    mc_ota_meta_record_t rec;
    memset(&rec, 0, sizeof(rec));
    rec.magic          = MC_OTA_META_MAGIC;
    rec.format_version = MC_OTA_META_FORMAT_VERSION;
    rec.active_slot    = ctx->ota_slot.active_slot;
    rec.target_slot    = ctx->ota_slot.target_slot;
    rec.state          = (uint8_t)ctx->ota_slot.state;
    rec.committed      = ctx->ota_slot.committed ? 1 : 0;
    rec.healthy        = ctx->ota_slot.boot_healthy ? 1 : 0;
    rec.abort_reason   = ctx->ota_slot.abort_reason;
    rec.image_length   = ctx->ota_image_length;
    rec.image_crc32    = computed_crc;
    rec.generation++;
    rec.record_crc32   = mc_ota_record_crc32(&rec);

    sim_block_erase_page(MC_OTA_FLASH_BLK_ID, 0);
    sim_block_write(MC_OTA_FLASH_BLK_ID, 0, (const uint8_t *)&rec, sizeof(rec));
    sim_block_erase_page(MC_OTA_FLASH_BLK_ID, 1);
    sim_block_write(MC_OTA_FLASH_BLK_ID, 256, (const uint8_t *)&rec, sizeof(rec));

    // Phase 5: reboot.
    mc_ota_reboot(&ctx->ota_slot);
    ctx->ota_active         = 0;
    ctx->ota_reboot_pending = 1;

    sim_trace_u32("ota_commit_ok", 1);
    sim_trace_u32("ota_reboot_requested", 10); // 10ms downtime

    send_ota_status(ctx, finish->request_id, ctx->ota_slot.state,
                    0 /* accepted */, 0, tx);
}

// ── Heartbeat RX task ─────────────────────────────────────────────────────

/// High-priority task that drains CAN RX and pushes heartbeat events
/// onto the heartbeat queue.  Also handles BMS fault frames inline.
/// Receives gateway_ctx_t * as pvParameters.
void heartbeat_rx(void *pvParameters)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)pvParameters;

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

            // Give semaphore to preempt: can_frame_processor (prio 5)
            // will wake and process this frame.
            xSemaphoreGive(ctx->can_frame_sem);
        }
    }
}

// ── Fault aggregator task ──────────────────────────────────────────────────

/// Low-rate task that aggregates fault statistics and traces them.
/// Accesses fault_manager under mutex protection.
/// Receives gateway_ctx_t * as pvParameters.
void fault_aggregator(void *pvParameters)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)pvParameters;

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

        // Trace aggregated fault stats.
        uint32_t agg = ((uint32_t)critical << 16) | ((uint32_t)warning << 8) | active;
        sim_trace_u32("fault_aggregate", agg);

        // If critical fault exists, set event group bit.
        if (critical > 0) {
            xEventGroupSetBits(ctx->mode_events, 0x02);
        }
    }
}

// ── CAN frame processor task ──────────────────────────────────────────

/// Highest-priority task (5) that processes CAN frames via counting
/// semaphore.  heartbeat_rx (prio 4) gives the semaphore for each
/// received frame, which preempts lower-priority tasks so this
/// processor runs immediately.
/// Each activation traces a counter showing preemption count.
/// Receives gateway_ctx_t * as pvParameters.
void can_frame_processor(void *pvParameters)
{
    gateway_ctx_t *ctx = (gateway_ctx_t *)pvParameters;

    uint32_t proc_count = 0;

    while (1) {
        // Block until heartbeat_rx gives the semaphore.
        xSemaphoreTake(ctx->can_frame_sem, portMAX_DELAY);

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

    gateway_ctx_t *ctx = gateway_ctx();
    gateway_init();
    gateway_primitives_init(ctx);
    ctx->gateway_task_handle = xTaskGetCurrentTaskHandle();

    // Create subordinate tasks, passing the context pointer.
    xTaskCreate(heartbeat_rx, "hb_rx", 768, ctx, 4, NULL);
    xTaskCreate(fault_aggregator, "fault_agg", 512, ctx, 2, NULL);
    xTaskCreate(can_frame_processor, "can_proc", 768, ctx, 5, NULL);

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
                // BMS lost → report fault.
                if (xSemaphoreTake(ctx->fm_mutex, pdMS_TO_TICKS(5)) == pdTRUE) {
                    fault_manager_report(&ctx->fm, MC_NODE_BMS,
                                         MC_BMS_FAULT_COMM_ERROR, 2);
                    xSemaphoreGive(ctx->fm_mutex);
                }
            }
        }

        // ── Process charging events (Stage E2) ──────────────────
        charge_evt_t cevt;
        while (xQueueReceive(ctx->charge_event_queue, &cevt, 0) == pdTRUE) {
            mc_charging_output_t chg_out;
            mc_charge_state_t old_cs =
                (mc_charge_state_t)ctx->charge_state;
            int accepted = mc_charging_step(&old_cs, cevt.event, &chg_out);
            if (accepted == 0) {
                ctx->charge_state = (uint8_t)chg_out.next_state;
                if (chg_out.next_state != old_cs) {
                    // Update vehicle mode to reflect new charge state.
                    ctx->gs.mode = mc_charging_vehicle_mode(&chg_out.next_state);
                    sim_trace_u32("charge_state",
                                  (uint32_t)chg_out.next_state);
                }
                // Send CHARGE_COMMAND with the output current.
                send_charge_command(ctx, &chg_out,
                                    cevt.event.target_soc, &tx);
            }
        }

        // ── Mode update ─────────────────────────────────────────
        mc_vehicle_mode_t old_mode = ctx->gs.mode;
        mc_vehicle_mode_t new_mode = update_vehicle_mode(ctx, now_ms);

        if (new_mode != old_mode) {
            sim_trace_u32("vehicle_mode", (uint32_t)new_mode);
            // Signal mode change to event group.
            xEventGroupSetBits(ctx->mode_events, 0x01);
        }

        // ── Check event group for mode transitions ──────────────
        // (this lets other tasks or tests wait for mode changes)
        EventBits_t mode_bits = xEventGroupGetBits(ctx->mode_events);
        if (mode_bits & 0x01) {
            sim_trace_u32("mode_event_group", mode_bits);
            xEventGroupClearBits(ctx->mode_events, 0x01);
        }

        // ── Process diagnostics requests ────────────────────────
        mc_diag_request_msg_t diag_req;
        while (xQueueReceive(ctx->diag_req_queue, &diag_req, 0) == pdTRUE) {
            handle_diag_request(ctx, now_ms, &diag_req, &tx);
        }

        // ── Process OTA events ──────────────────────────────────
        ota_evt_t ota_evt;
        while (xQueueReceive(ctx->ota_req_queue, &ota_evt, 0) == pdTRUE) {
            switch (ota_evt.type) {
            case OTA_EVT_REQUEST:
                handle_ota_request(ctx, now_ms, &ota_evt.payload.request, &tx);
                break;
            case OTA_EVT_CHUNK:
                handle_ota_chunk(ctx, &ota_evt.payload.chunk, &tx);
                break;
            case OTA_EVT_FINISH:
                handle_ota_finish(ctx, &ota_evt.payload.finish, &tx);
                break;
            default:
                break;
            }
        }

        // ── Check for pending reboot ────────────────────────────
        if (ctx->ota_reboot_pending) {
            // Signal reboot with 10ms downtime via trace fault marker.
            sim_trace_u32("ota_reboot_execute", 10);
            // Reset the pending flag so we don't loop.
            ctx->ota_reboot_pending = 0;
            // Gateway reboot resets state to OFF.
            gateway_state_reboot(&ctx->gs);
        }

        // ── Broadcast phase ─────────────────────────────────────
        // Send heartbeat every 100ms.
        if (now_ms % 100 == 0) {
            send_heartbeat(now_ms, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }

        // Send vehicle mode on change or every 50ms.
        if (new_mode != old_mode || now_ms % 50 == 0) {
            send_vehicle_mode(ctx, &tx);
            sim_can_send(0, tx.id, tx.data, tx.len, 0, 0);
        }
    }
}
