# Reserve modified mouse clicks for an application

A focused application may need mouse clicks while the compositor modifier is
held, such as clicking a push-to-talk dictation editor to keep listening.
Niri normally uses these clicks to move or resize windows or drag workspaces.

`disable-modified-mouse-gestures true` is an opt-in, dynamic window-rule property.
While pointing at a matching window in the normal layout, Mod+left, Mod+right
and Mod+middle do not start Niri's built-in move, resize or workspace drag.
The button event and its original modifiers are delivered normally to the
client. Unmodified clicks, application-requested moves/resizes, wheel/scroll
bindings, explicit configured mouse-button bindings, and compositor overlays
(overview, screenshot, window switcher and lock) retain their existing behavior.

The default is false. Normal window-rule matching, exclusions, dynamic updates
and last-matching-value precedence apply, including an explicit false override.
No application IDs are built into the compositor. For the dictation editor:

```kdl
window-rule {
    match app-id="^org[.]wsprflow[.]Draft$"
    disable-modified-mouse-gestures true
}
```

Validate both default move/resize/workspace-drag behavior and the opted-in
window using actual pointer/keyboard events. Cover unmatched windows, explicit
false overrides, dynamic matching, and explicit mouse binding precedence.
Application-side acceptance must also verify that the click still places a
caret or activates a button while its hold shortcut is down.
