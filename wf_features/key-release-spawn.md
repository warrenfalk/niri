# Spawn with key release

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

EOF without a complete release line means cancellation, never permission to
send. Locking, explicit VT switching/suspending, removal of the input source,
or compositor exit cancels outstanding holds. Physical keyboards share one
source; unplugging any physical keyboard cancels all physical holds. The child
must also enforce its own destination/focus policy and treat stream errors as
cancellation. Niri does not terminate the child on cancellation.

This action is deliberately binding-only, has no IPC action, cannot be used
with pointer/scroll triggers, and cannot be allowed while locked. It is not a
general keyboard-event monitor.

Validation must cover config parsing, repeat suppression, modifier-first
release, source identity, release duration framing, cancellation EOF, and a
real child receiving both quick and delayed releases in a nested compositor.
