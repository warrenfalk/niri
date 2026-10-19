# Window marker (fork extension)

An integration can mark one window independently of keyboard focus by sending
`{"WindowMarker":{"id":12}}` on a persistent Niri IPC connection. Clear with
`{"WindowMarker":{"id":null}}` on the same connection. The marker clears when
the connection or window closes. Use another connection for the event stream.

Configure the inset outline with `layout { window-marker { width 3; color
"#ffb43c"; }; }`. It works for tiled, floating and fullscreen windows without
resizing them, and leaves the ordinary focus decoration separate. Its edge
pixels overlay the window contents. `off` disables drawing; width zero hides it.

See [the complete fork contract](../../wf_features/window-marker.md) for ownership,
cleanup, stacking, defaults and validation expectations.
