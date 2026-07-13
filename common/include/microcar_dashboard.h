// microcar_dashboard.h — dashboard framebuffer renderer (Stage G)
//
// 320x240 RGB565 little-endian framebuffer. Six screen states.
// Per costar_microcar_dogfood_plan.md Stage G.

#ifndef MICROCAR_DASHBOARD_H
#define MICROCAR_DASHBOARD_H

#include "microcar_protocol.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ── Dashboard state ──────────────────────────────────────────────────────

typedef struct {
    mc_vehicle_mode_t mode;
    uint8_t           page;           // 0 or 1
    uint16_t          bg_color;       // RGB565 background
    uint8_t           speed_kmh;      // 0-255 km/h
    int8_t            torque_percent; // -100 to 100
    uint8_t           soc_percent;    // 0-100
    uint8_t           current_a_x2;   // 0.5A units
    uint8_t           ota_progress;   // 0-100 percent
} mc_dash_state_t;

// ── RGB565 colors ────────────────────────────────────────────────────────

#define MC_DASH_BLACK   0x0000
#define MC_DASH_WHITE   0xFFFF
#define MC_DASH_GREEN   0x07E0
#define MC_DASH_RED     0xF800
#define MC_DASH_AMBER   0xFD20
#define MC_DASH_BLUE    0x001F

// ── Display dimensions ───────────────────────────────────────────────────

#define MC_DASH_WIDTH   320
#define MC_DASH_HEIGHT  240

// ── Functions ────────────────────────────────────────────────────────────

/// Initialise dashboard state.
void mc_dash_init(mc_dash_state_t *ds);

/// Render the current dashboard state into a framebuffer.
/// framebuffer must be at least 320*240 uint16_t values.
void mc_dash_render(mc_dash_state_t *ds, uint16_t *framebuffer);

/// Update the vehicle mode and set the appropriate background color.
void mc_dash_set_mode(mc_dash_state_t *ds, mc_vehicle_mode_t mode);

#ifdef __cplusplus
}
#endif

#endif // MICROCAR_DASHBOARD_H
