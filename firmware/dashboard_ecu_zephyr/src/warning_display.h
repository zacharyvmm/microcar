// warning_display.h — dashboard warning display logic
//
// Manages the priority-based warning display. Only the highest-priority
// active warning is shown at any time.

#ifndef WARNING_DISPLAY_H
#define WARNING_DISPLAY_H

#include "microcar_protocol.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Warning display state.
typedef struct {
    /// Currently displayed warning code (MC_WARN_NONE if none).
    uint8_t displayed_warning;

    /// Warning severity: 0=none, 1=info, 2=warning, 3=critical.
    uint8_t severity;
} warning_display_t;

/// Initialise the warning display.
void warning_display_init(warning_display_t *wd);

/// Update the display with a warning.
/// Higher-severity warnings replace lower-severity ones.
/// Returns the new displayed warning code.
uint8_t warning_display_update(warning_display_t *wd,
                               uint8_t warning_code,
                               uint8_t severity);

/// Clear the currently displayed warning.
void warning_display_clear(warning_display_t *wd);

/// Get the warning severity for a given warning code.
uint8_t warning_display_severity_for(uint8_t warning_code);

#ifdef __cplusplus
}
#endif

#endif // WARNING_DISPLAY_H
