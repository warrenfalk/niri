# Spawn with key release

**wsprflow purpose:** Added specifically for wsprflow's Mod+D tap/hold dictation
modes. The later `wait-for-all-keys` option prevents release-to-send from firing
while Mod is still held after D is released. See the
[wsprflow feature overview](wsprflow.md) for related fork changes.

Applications such as push-to-talk dictation need the lifetime of a compositor
shortcut, even when its press and release are consumed by the compositor or
focus changes to another application.

`spawn-with-key-release "program" "argument"` is a keyboard-binding action. It
starts one process on the initial key press, with the same environment,
activation token, workspace metadata and argument handling as `spawn`.

The process's standard input stays open while the trigger key is held. On that
key's release, Niri writes one decimal integer followed by a newline: the hold
duration in milliseconds, measured by the compositor. It then closes the
stream. A quick release before the process starts is buffered. Releasing a
modifier first does not end the hold; focus changes do not end it either.
Autorepeat never starts another process, regardless of the `repeat` property.
Keyboards with independent Wayland keyboard sources have independent holds.

For release-to-send applications, use
`spawn-with-key-release "program" "argument" wait-for-all-keys=true`.
The process starts immediately, but Niri waits for both the trigger release and
an idle seat keyboard before writing the release line. All pressed keys and
active shortcut modifiers must be released, including virtual-keyboard modifier
masks. Caps Lock and Num Lock may stay enabled. There is no timed deadline for
releasing modifiers. The reported duration stops at the trigger's first release;
holding Mod afterward must not turn a tap into a hold. If the trigger is pressed
again while waiting for idle, it does not start another process or change that
duration. Without this property, the trigger-only behavior remains unchanged.
The child continues running while waiting, so a dictation application may keep
capturing until the whole shortcut is released. This is a release notification,
not a reservation: another key press or grab after the notification can still
make a later paste fail its own preflight checks.

EOF without a complete release line means cancellation, never permission to
send. Locking, explicit VT switching/suspending, removal of the input source,
or compositor exit cancels outstanding holds, including those waiting for
modifiers after the trigger was released. Physical keyboards share one
source; unplugging any physical keyboard cancels all physical holds. The child
must also enforce its own destination/focus policy and treat stream errors as
cancellation. Niri does not terminate the child on cancellation.

This action is deliberately binding-only, has no IPC action, cannot be used
with pointer/scroll triggers, and cannot be allowed while locked. It is not a
general keyboard-event monitor.

Validation must cover config parsing, repeat suppression, modifier-first
release, source identity, release duration framing, cancellation EOF, and a
real child receiving both quick and delayed releases in a nested compositor.
The opt-in idle wait must cover both release orders, a delayed modifier release
longer than the application's paste retry deadline, unchanged tap duration,
unrelated held keys, virtual modifier masks and cancellation while waiting.
