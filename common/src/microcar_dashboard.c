// microcar_dashboard.c — dashboard framebuffer renderer (stub)
// Full implementation per costar_microcar_dogfood_plan.md Stage G
// TODO: implement 320x240 RGB565 framebuffer renderer

#include "microcar_dashboard.h"
#include <string.h>

void mc_dash_init(mc_dash_state_t *ds) {
    memset(ds, 0, sizeof(*ds));
    ds->page = 0;
}

void mc_dash_render(mc_dash_state_t *ds, uint16_t *framebuffer) {
    // Stub: clear to black
    for (int i = 0; i < 320 * 240; i++) {
        framebuffer[i] = ds->bg_color;
    }
}

void mc_dash_set_mode(mc_dash_state_t *ds, mc_vehicle_mode_t mode) {
    ds->mode = mode;
    switch (mode) {
    case VEHICLE_OFF:     ds->bg_color = 0x0000; break;
    case VEHICLE_READY:   ds->bg_color = 0x0010; break;
    case VEHICLE_DRIVE:   ds->bg_color = 0x0200; break;
    case VEHICLE_LIMP:    ds->bg_color = 0xFD20; break;
    case VEHICLE_FAULT:   ds->bg_color = 0x7800; break;
    case VEHICLE_CHARGING: ds->bg_color = 0x4010; break;
    default:              ds->bg_color = 0x0000; break;
    }
}
