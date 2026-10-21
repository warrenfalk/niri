# Connection-owned window marker

**wsprflow purpose:** Added specifically to mark wsprflow's original dictation
destination while its draft editor or another application has keyboard focus.
See the [wsprflow feature overview](wsprflow.md) for related fork changes.

Trusted local integrations can identify one destination independently of keyboard
focus. Speech recognition, destination selection and dictation policy belong to
the integration. The capability is generic and has no urgency or application-name
semantics.

## IPC and lifetime

On a persistent `$NIRI_SOCKET` connection send one JSON request per line:

```json
{"WindowMarker":{"id":12}}
```

Success is `{"Ok":"Handled"}`. Send `{"WindowMarker":{"id":null}}` on the
**same connection** to clear. Setting another valid id atomically retargets the
marker. Unknown ids return `{"Err":"window not found"}` and preserve the old
marker. One compositor-wide marker is available. Another connection cannot
retarget or clear a live owner's marker and receives an error.

Keep both socket halves open. EOF (including write-half shutdown), a socket
read/write failure, or switching the connection to `EventStream` releases the
marker. Use a separate connection for events. A one-shot `niri msg raw-request`
does not retain the marker. Process exit closes the socket, provided the
integration does not leave inherited/duplicated socket descriptors in children.
Closing/unmapping the destination also releases the marker; remapping creates a
new window id. No explicit cleanup request from a crashed integration is needed.

## Appearance

```kdl
layout {
    window-marker {
        width 3
        color "#ffb43c"
        // off
    }
}
```

Defaults are enabled, width 3 logical pixels, opaque amber `#ffb43c`. `off`
disables drawing without changing the lease; `on` re-enables it in later config
includes. Width zero hides the outline. Width rounds to physical pixels and is
limited to half the tile's shortest dimension. Config reload applies immediately.

The outline is drawn **inside** the window's decorated rectangle, above its
contents, and can cover its edge pixels. It never changes content size, tile
position, input regions or focus. Ordinary focus decoration stays outside the
marker and uses its own colors. Choose distinct colors if customizing both.
The marker remains when another window has keyboard focus. It follows tiled,
floating, maximized and fullscreen windows, including transitions and output
changes. Fullscreen draws the marker inside the output edges. Normal stacking
and visibility still apply: it does not reveal a covered window, an inactive tab,
another workspace or a window behind the lock screen. Rounded corners follow the
window's decoration shape. It appears in captures that include decorations.

## Validation contract

Keep JSON, config parsing, lease conflict/cleanup and no-resize regression tests.
In a nested compositor check tiled and floating destinations while another
window is focused, rounded corners, fullscreen edges, retargeting, config reload,
window closure and owner disconnection. Verify that the focused window retains
ordinary focus decoration and that marking causes no window configure/resize.

This capability requires only Niri changes and is independently usable without
temporary paste support.

Implementation validation: the full non-visual workspace test suite, workspace
Clippy (all targets), nightly rustfmt, focused IPC/config/serialization tests and a nested
Wayland session with a GTK text view and Kitty passed. Captures confirmed the
unfocused tiled and floating amber outline alongside Kitty's blue focus ring,
an inset fullscreen outline, and removal after clear and owner disconnect. The
test session used isolated configuration, runtime files and clipboard; the
workstation compositor was not restarted or activated. The nested launch needed
Mesa from the development flake because the installed workstation driver uses a
newer libc. Multi-monitor moves and fractional-scale rendering remain manual
acceptance checks.
