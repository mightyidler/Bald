# Implementation notes

## Reference validation

`ihateborders` 1.1.1 was inspected and built successfully on 2026-10-03.
Its border operation reads `GWL_STYLE`, removes `WS_BORDER`, `WS_CAPTION`,
`WS_THICKFRAME`, and `WS_DLGFRAME`, writes the style, and requests
`SWP_FRAMECHANGED` through `SetWindowPos`.

Bald preserves that proven operation but deliberately differs in two ways:

1. `make_borderless` and `restore_borders` are explicit, separate operations.
   Automatic application can therefore be repeated safely without toggling a
   border back on.
2. Before removing the frame, Bald records both the outer rectangle and the
   client rectangle. The borderless window is fitted to the previous client
   rectangle so games that cache their render/input surface keep matching mouse
   coordinates. Restoring a rule puts back the original style and outer rectangle.
   Bald never stretches the target to a monitor.

Original live window styles and rectangles are kept only in memory and keyed by
HWND. They are never serialized because an HWND is meaningful only for the
current window instance. Turning off either the global automatic switch or a
per-application switch restores all matching windows changed in this session.

## Current watcher

The first implementation uses a four-second reconciliation scan. This is a
bounded, low-frequency pass with no busy loop and naturally catches recreated
HWNDs. A WinEvent hook can be added as a latency optimization after behavior is
validated against real games; reconciliation should remain as the fallback.

## Configuration

Configuration is JSON in the per-user application config directory. Writes go
to a temporary file, are flushed, and are then renamed over the active file.
Rules prefer a normalized executable path, fall back to the executable filename,
and use the selected window class as a discriminator.
