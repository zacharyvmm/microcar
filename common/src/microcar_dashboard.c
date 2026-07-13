// microcar_dashboard.c — dashboard framebuffer renderer (Stage G)
//
// 320x240 RGB565 little-endian framebuffer. Six screen states plus boot screen.
// Per costar_microcar_dogfood_plan.md Stage G.
//
// Region borders are white and one pixel wide. Bar interiors fill green
// left-to-right. Speed and torque use white seven-segment digits with
// 8-pixel stroke width.

#include "microcar_dashboard.h"
#include "microcar_ota_slot.h"
#include <string.h>

// ── Sentinel for uninitialised mode (boot screen) ─────────────────────────

#define MC_DASH_MODE_UNSET 0xFF

// ── Internal helpers ──────────────────────────────────────────────────────

/// Fill a rectangle in the framebuffer with a solid colour.
static void fill_rect(uint16_t *fb, int x, int y, int w, int h, uint16_t color)
{
    if (x < 0 || y < 0 || x + w > MC_DASH_WIDTH || y + h > MC_DASH_HEIGHT) return;
    for (int row = 0; row < h; row++) {
        uint16_t *line = fb + (y + row) * MC_DASH_WIDTH + x;
        for (int col = 0; col < w; col++) { line[col] = color; }
    }
}

/// Draw a one-pixel rectangle border in the framebuffer.
static void draw_border(uint16_t *fb, int x, int y, int w, int h, uint16_t color)
{
    if (w <= 0 || h <= 0) return;
    // top + bottom
    for (int col = 0; col < w; col++) {
        int i_top = (y) * MC_DASH_WIDTH + (x + col);
        int i_bot = (y + h - 1) * MC_DASH_WIDTH + (x + col);
        if (i_top < MC_DASH_WIDTH * MC_DASH_HEIGHT) fb[i_top] = color;
        if (i_bot < MC_DASH_WIDTH * MC_DASH_HEIGHT) fb[i_bot] = color;
    }
    // left + right (excluding corners)
    for (int row = 1; row < h - 1; row++) {
        int i_left  = (y + row) * MC_DASH_WIDTH + x;
        int i_right = (y + row) * MC_DASH_WIDTH + (x + w - 1);
        if (i_left  < MC_DASH_WIDTH * MC_DASH_HEIGHT) fb[i_left]  = color;
        if (i_right < MC_DASH_WIDTH * MC_DASH_HEIGHT) fb[i_right] = color;
    }
}

/// Draw a horizontal bar with border and green fill (left-to-right, 0–100).
static void draw_bar(uint16_t *fb, int x, int y, int w, int h,
                     uint8_t value, uint8_t max_val, uint16_t bg)
{
    draw_border(fb, x, y, w, h, MC_DASH_WHITE);
    if (max_val == 0) return;
    int inner_w = w - 2;
    int inner_h = h - 2;
    if (inner_w <= 0 || inner_h <= 0) return;
    int fill_w = (inner_w * value) / max_val;
    if (fill_w > inner_w) fill_w = inner_w;
    // Fill the interior background first (erases previous bar state)
    fill_rect(fb, x + 1, y + 1, inner_w, inner_h, bg);
    // Green fill
    fill_rect(fb, x + 1, y + 1, fill_w, inner_h, MC_DASH_GREEN);
}

// ── Seven-segment digit drawing (8 px stroke, 30×46 px cell) ─────────────

#define DIGIT_W  30
#define DIGIT_H  46
#define STROKE   8

// Segment origin offsets within a digit cell: (sx, sy, sw, sh)
typedef struct { int x, y, w, h; } seg_t;

static const seg_t SEG_A = { 1,  0, 28,  8}; // top
static const seg_t SEG_B = {22,  1,  8, 18}; // top-right
static const seg_t SEG_C = {22, 20,  8, 18}; // bottom-right
static const seg_t SEG_D = { 1, 38, 28,  8}; // bottom
static const seg_t SEG_E = { 0, 20,  8, 18}; // bottom-left
static const seg_t SEG_F = { 0,  1,  8, 18}; // top-left
static const seg_t SEG_G = { 1, 19, 28,  8}; // middle

// Segment bitmask: bit 6=A 5=B 4=C 3=D 2=E 1=F 0=G  (LSB=G)
static const uint8_t DIGIT_SEGS[10] = {
    0x7E, // 0: A B C D E F
    0x30, // 1: B C
    0x6D, // 2: A B G E D
    0x79, // 3: A B G C D
    0x33, // 4: F G B C
    0x5B, // 5: A F G C D
    0x5F, // 6: A F G E C D
    0x70, // 7: A B C
    0x7F, // 8: A B C D E F G
    0x7B, // 9: A F G B C D
};

static void draw_seg(uint16_t *fb, int base_x, int base_y,
                     const seg_t *s, uint16_t color)
{
    fill_rect(fb, base_x + s->x, base_y + s->y, s->w, s->h, color);
}

static void draw_digit(uint16_t *fb, int x, int y, uint8_t d, uint16_t color)
{
    if (d > 9) return;
    uint8_t mask = DIGIT_SEGS[d];
    if (mask & 0x40) draw_seg(fb, x, y, &SEG_A, color);
    if (mask & 0x20) draw_seg(fb, x, y, &SEG_B, color);
    if (mask & 0x10) draw_seg(fb, x, y, &SEG_C, color);
    if (mask & 0x08) draw_seg(fb, x, y, &SEG_D, color);
    if (mask & 0x04) draw_seg(fb, x, y, &SEG_E, color);
    if (mask & 0x02) draw_seg(fb, x, y, &SEG_F, color);
    if (mask & 0x01) draw_seg(fb, x, y, &SEG_G, color);
}

/// Draw an unsigned integer right-aligned within (rx, ry, rw, rh).
/// Digits are drawn at the vertical centre of the region.
static void draw_number(uint16_t *fb, int rx, int ry, int rw, int rh,
                        int value, uint16_t color, uint16_t bg)
{
    // Build digit string
    char buf[8];
    int nd = 0;
    if (value == 0) {
        buf[0] = '0'; nd = 1;
    } else {
        int v = value;
        while (v > 0 && nd < 7) { buf[nd++] = '0' + (v % 10); v /= 10; }
        // Reverse in-place
        for (int i = 0; i < nd / 2; i++) {
            char t = buf[i]; buf[i] = buf[nd - 1 - i]; buf[nd - 1 - i] = t;
        }
    }

    int total_w = nd * DIGIT_W;
    int start_x = rx + rw - total_w;
    int start_y = ry + (rh - DIGIT_H) / 2;

    // Clear region to background
    fill_rect(fb, rx, ry, rw, rh, bg);

    for (int i = 0; i < nd; i++) {
        draw_digit(fb, start_x + i * DIGIT_W, start_y, buf[i] - '0', color);
    }
}

/// Draw a minus sign (horizontal bar) at centre-left of the digit region.
static void draw_minus(uint16_t *fb, int rx, int ry, int rw, int rh,
                       uint16_t color, uint16_t bg)
{
    int bar_w = 18;
    int bar_h = STROKE;
    int bar_x = rx + 2;
    int bar_y = ry + (rh - bar_h) / 2;
    fill_rect(fb, bar_x, bar_y, bar_w, bar_h, color);
    // Fill rest of sign area with bg
    if (bar_x + bar_w < rx + rw) {
        fill_rect(fb, bar_x + bar_w, ry, rx + rw - bar_x - bar_w, rh, bg);
    }
}

// ── Public API ────────────────────────────────────────────────────────────

void mc_dash_init(mc_dash_state_t *ds)
{
    memset(ds, 0, sizeof(*ds));
    ds->page = 0;
    ds->mode = MC_DASH_MODE_UNSET;
    ds->prev_mode = MC_DASH_MODE_UNSET;
}

void mc_dash_set_mode(mc_dash_state_t *ds, mc_vehicle_mode_t mode)
{
    ds->mode = mode;
    switch (mode) {
    case VEHICLE_OFF:            ds->bg_color = 0x0000; break;
    case VEHICLE_READY:          ds->bg_color = 0x0010; break;
    case VEHICLE_DRIVE:          ds->bg_color = 0x0200; break;
    case VEHICLE_LIMP:           ds->bg_color = 0xFD20; break;
    case VEHICLE_FAULT:          ds->bg_color = 0x7800; break;
    case VEHICLE_CHARGING:       ds->bg_color = 0x4010; break;
    case VEHICLE_SERVICE:        ds->bg_color = 0x0000; break;
    case VEHICLE_OTA_UPDATE:     ds->bg_color = 0x0008; break;
    case VEHICLE_TRANSPORT_MODE: ds->bg_color = 0x0000; break;
    default:                     ds->bg_color = 0x0000; break;
    }
}

void mc_dash_render(mc_dash_state_t *ds, uint16_t *framebuffer)
{
    uint16_t bg = ds->bg_color;

    // Clear full framebuffer on screen-state change (mode transition).
    if (ds->mode != ds->prev_mode) {
        fill_rect(framebuffer, 0, 0, MC_DASH_WIDTH, MC_DASH_HEIGHT, bg);
        ds->prev_mode = ds->mode;
    }

    // Boot screen — handled outside the switch to avoid -Wswitch.
    if (ds->mode == MC_DASH_MODE_UNSET) {
        fill_rect(framebuffer, 0, 0, MC_DASH_WIDTH, MC_DASH_HEIGHT, MC_DASH_BLACK);
        fill_rect(framebuffer, 40, 108, 240, 24, MC_DASH_WHITE);
        return;
    }

    switch (ds->mode) {
    // ── OFF ────────────────────────────────────────────────────────────
    case VEHICLE_OFF:

    // ── READY ──────────────────────────────────────────────────────────
    case VEHICLE_READY:
        // Green status bar (0,0,320,40) — one-pixel white border
        draw_border(framebuffer, 0, 0, 320, 40, MC_DASH_WHITE);
        fill_rect(framebuffer, 1, 1, 318, 38, MC_DASH_GREEN);
        // Speed region (20,70,120,80)
        if (ds->page == 0) {
            draw_number(framebuffer, 20, 70, 120, 80,
                        ds->speed_kmh, MC_DASH_WHITE, bg);
        }
        break;

    // ── DRIVE ──────────────────────────────────────────────────────────
    case VEHICLE_DRIVE:
        // Green status bar
        draw_border(framebuffer, 0, 0, 320, 40, MC_DASH_WHITE);
        fill_rect(framebuffer, 1, 1, 318, 38, MC_DASH_GREEN);
        if (ds->page == 0) {
            // Speed region (20,70,120,80)
            draw_number(framebuffer, 20, 70, 120, 80,
                        ds->speed_kmh, MC_DASH_WHITE, bg);
            // Torque region (180,70,120,80)
            if (ds->torque_percent < 0) {
                draw_minus(framebuffer, 180, 70, 120, 80, MC_DASH_WHITE, bg);
                draw_number(framebuffer, 200, 70, 100, 80,
                            -ds->torque_percent, MC_DASH_WHITE, bg);
            } else {
                fill_rect(framebuffer, 180, 70, 120, 80, bg);
                draw_number(framebuffer, 180, 70, 120, 80,
                            ds->torque_percent, MC_DASH_WHITE, bg);
            }
        }
        break;

    // ── LIMP ───────────────────────────────────────────────────────────
    case VEHICLE_LIMP:
        // Amber status bar
        draw_border(framebuffer, 0, 0, 320, 40, MC_DASH_WHITE);
        fill_rect(framebuffer, 1, 1, 318, 38, MC_DASH_AMBER);
        if (ds->page == 0) {
            draw_number(framebuffer, 20, 70, 120, 80,
                        ds->speed_kmh, MC_DASH_WHITE, bg);
            if (ds->torque_percent < 0) {
                draw_minus(framebuffer, 180, 70, 120, 80, MC_DASH_WHITE, bg);
                draw_number(framebuffer, 200, 70, 100, 80,
                            -ds->torque_percent, MC_DASH_WHITE, bg);
            } else {
                fill_rect(framebuffer, 180, 70, 120, 80, bg);
                draw_number(framebuffer, 180, 70, 120, 80,
                            ds->torque_percent, MC_DASH_WHITE, bg);
            }
        }
        break;

    // ── FAULT ──────────────────────────────────────────────────────────
    case VEHICLE_FAULT:
        // Red warning (0,170,320,70)
        draw_border(framebuffer, 0, 170, 320, 70, MC_DASH_WHITE);
        fill_rect(framebuffer, 1, 171, 318, 68, MC_DASH_RED);
        break;

    // ── CHARGING ───────────────────────────────────────────────────────
    case VEHICLE_CHARGING:
        if (ds->page == 0) {
            // SOC bar (20,90,280,24)
            draw_bar(framebuffer, 20, 90, 280, 24,
                     ds->soc_percent, 100, bg);
            // Current bar (20,140,280,24)
            draw_bar(framebuffer, 20, 140, 280, 24,
                     ds->current_a_x2, 100, bg);
        }
        break;

    // ── OTA_UPDATE ─────────────────────────────────────────────────────
    case VEHICLE_OTA_UPDATE:
        // Progress bar (20,110,280,24)
        draw_bar(framebuffer, 20, 110, 280, 24,
                 ds->ota_progress, 100, bg);
        break;

    // ── SERVICE / TRANSPORT_MODE / default ─────────────────────────────
    default:
        // background only
        break;
    }
}
