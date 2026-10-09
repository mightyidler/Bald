# Implementation notes

## Reference validation

### Maple native-frame compatibility (2026-10-09)

Only the measured `MapleStoryClass` preserves native styles and DWM rendering,
clipping the client area with a window region. A synchronous region change expands
the real game's client to its outer size; an owner-thread asynchronous
`SWP_FRAMECHANGED | SWP_ASYNCWINDOWPOS` restores native client geometry without
minimizing, hiding, activating, or resizing it. Both application and restoration
use this refresh and confirm geometry before success. Drag interception uses the
visible client top 12 physical pixels, and centering accounts for hidden frame
offsets. Other classes retain their existing style-removal path. See
`MAPLE-DIAGNOSIS.md` for actual-game evidence and remaining input-level checks.

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
   coordinates. Restoring a rule restores the style while preserving the current
   content size and client origin, including changes made after initial removal.
   Bald never stretches the target to a monitor.

Original styles, process identity, frame insets and managed regions are kept in
memory and keyed by HWND. They are never serialized because an HWND is meaningful only for the
current window instance. Turning off either the global automatic switch or a
per-application switch restores all matching windows changed in this session.

## Current watcher

The watcher blocks in GetMessage and wakes on coalesced WinEvent/configuration
requests or a four-second fallback timer. Managed game size changes and the end
of native window movement request reconciliation; ordinary movement does not.
There is no idle 16ms polling loop. While a drag position request is outstanding,
the separate worker checks acknowledgement every 16ms, stopping after 100ms.
The mouse hook consumes only approved top-strip presses/releases; movement passes through.

## Game compatibility and verification

All new applications, including MapleStory, use the same native style operation.
Four extended frame styles are also removed; taskbar, layered and topmost flags
are preserved. Apply/restore position operations skip WM_WINDOWPOSCHANGING, preserving the
owner order, activation and visibility state. Maple-specific region cropping is
no longer used for new applications.
User dragging uses a fixed pointer/window anchor, not SC_MOVE or a timed delay.
Mouse movement is passed through and window operations run on a separate worker.
Drag position requests retain owner notifications except for the exact
`MapleStoryClass` compatibility path, which restores SWP_NOSENDCHANGING to bypass
the modeled Y=0 position-changing callback. Live Maple testing still pins Y to
zero, so this flag has not established a fix or the real cause. Other window classes keep
their owner constraints. Only one asynchronous request is outstanding; mouse bursts replace
the latest coordinate instead of queuing a trail of stale positions. An already
posted OS request cannot be cancelled.

Rules offer only Allowed and Blocked, defaulting to Blocked. Old `auto` values
deserialize as Blocked and serialize as `disabled`; no automatic movement
classification, caption hit testing, or native move confirmation is retained.
Only the top twelve physical pixels of an allowed window intercept drag clicks.
Blocked borderless windows consume presses/releases in that same strip so the
game cannot start its own drag there. The hook remains installed when all rules
are Blocked. Input below that strip still passes through, including any game-owned
native movement behavior; this is not a global prohibition of native window moves.
No automatic movement classification or executable-name exceptions are used.
Explicitly selecting Blocked centers matching windows once on their current full
monitor rectangle, without resizing, activation or z-order changes. Hidden,
minimized and maximized windows are left unchanged. Scans and default Blocked
configuration never recenter. Outstanding moves reject centering rather than
queueing conflicting positions; unacknowledged centering reports an error.
Dragging a registered, already-borderless window does not require frame restoration
ownership in the current session, without guessing restoration styles or adopting
old ownership. The same 12px strip and live HWND/process, geometry and mode guards
apply whether or not an original frame snapshot exists.
Only already-active windows can begin a custom drag. A rejected/unacknowledged request suspends that window's
custom dragging until its mode changes, without disabling other windows.
Original region-based fixtures remain test-only; legacy restoration handling is
retained without automatically rewriting an un-restored legacy window.
Win32 regression fixtures cover native APIs without touching live games; actual
game validation remains separate. Current evidence and remaining checks are in
`WINDOW-DEBUG.md`.
Source comparison and evidence limits are recorded in `BORDERLESS-RESEARCH.md`.

## Executable icons

Native icon resources are read with data/resource-only LoadLibraryEx flags, then
decoded at their stored size. Selection uses actual cropped pixel dimensions,
not Shell canvas dimensions or a 32px fallback for every app. This does not run
the target executable. See [Microsoft resource loading](https://learn.microsoft.com/en-us/windows/win32/intl/loading-a-win32-pe-resource-module).

## Configuration

Configuration is JSON in the per-user application config directory. Writes go
to a temporary file, are flushed, and are then renamed over the active file.
Rules prefer a normalized executable path, fall back to the executable filename,
and use the selected window class as a discriminator.
