# Destination-specific temporary paste

**wsprflow purpose:** Added specifically so wsprflow can insert dictated text
without replacing the user's clipboard or adding dictation to clipboard history.
It remains the insertion path for destinations without usable text-input context.
See the [wsprflow feature overview](wsprflow.md) for its Smithay dependency.

An integration can send UTF-8 text to a native Wayland application's ordinary
paste shortcut without becoming the ordinary clipboard owner. Recognition,
draft policy, choosing the destination and choosing its shortcut belong to the
integration. This capability is independent of the window marker.

## Structured IPC

Send a single JSON line over `$NIRI_SOCKET` and read one reply:

```json
{"PasteText":{"id":12,"text":"Hello 🌍\nsecond line\n","shortcut":"CtrlV","timeout_ms":5000}}
```

`id` is the existing Niri window id. `text` must contain 1–1,048,576 UTF-8 bytes.
`shortcut` is `CtrlV` (default) or `CtrlShiftV`. The integration chooses; Niri has
no terminal/application-name shortcut table. `timeout_ms` defaults to 5000 and
must be 100–30000. It is a cleanup deadline, not a paste delay. There is no new
configuration setting for this operation.

An optional `context_revision` conditions preflight on a live surrounding-text
snapshot. Omission preserves this contract's original behavior. See
[destination text context](text-input-context.md) for lease lifetime and stale
state refusal before any offer or input is sent.

Use a dedicated connection for each paste and keep its write half open until the
reply. Closing it, including shutting down just the write half, cancels the
operation. A separate persistent connection can retain a window marker. A client
may also keep a paste connection open and send requests sequentially after each
reply. Only one temporary paste may be active on the seat at a time; another
start receives an error and cannot replace it.

Successful protocol handling returns accounting, for example:

```json
{"Ok":{"PasteText":{"id":12,"outcome":"DataServed","requests":1,"transfers_completed":1,"bytes_written":25}}}
```

The counters above are illustrative. `requests` counts accepted MIME receive
requests, `transfers_completed` counts fully written request pipes, and
`bytes_written` sums all writes, counting repeated MIME requests separately.
At most 32 receive requests are accepted per operation.

Possible `outcome` values:

| Outcome | Meaning |
| --- | --- |
| `DataServed` | At least one full write completed, no writers remain, and the application destroyed all its private offers. |
| `Cancelled` | The requesting IPC connection closed. |
| `FocusChanged` | Focus left the exact destination surface, the keyboard became unavailable, a keyboard grab began, or the session locked. |
| `DestinationClosed` | The destination closed or unmapped. |
| `ClipboardChanged` | A real copy/clear/source closure changed the ordinary selection. |
| `TimedOut` | The cleanup deadline expired, possibly after data was served. |
| `TransferFailed` | A pipe failed, a MIME request was invalid, or the receive limit was exceeded. |
| `OfferDismissed` | All private offers were destroyed without completing a transfer. |

**None of these outcomes acknowledges insertion.** A completed write means the
bytes entered the application's request pipe. It does not prove that the
application read them, accepted them, or inserted them into a widget. Data may
already have been used even when the outcome is cancellation, failure or timeout.
Do not automatically retry on an ambiguous result; it could duplicate text.
Data already written to a pipe cannot be recalled.

Preflight failures return the ordinary `{"Err":"description"}` reply. They
include invalid size/deadline, a missing/off-output destination, another active
paste, unsupported destinations, unavailable shortcut keys, held keys/modifiers,
keyboard grabs, a locked session or inability to focus the destination. Errors
for unsupported clients start with `unsupported destination:`. No failure falls
back to replacing the clipboard or using another delivery method.

Text belongs in the structured request body, not a process argument. Direct
socket use is preferred. `niri msg raw-request` can also consume JSON from stdin;
`--print-request` rejects paste requests. Debug formatting redacts their text,
and compositor error messages and results do not include transcript contents.
IPC decoding errors report only category, line and column, including for
malformed requests; Serde's value-bearing diagnostic is not logged or returned.

## Focus, privacy and source preservation

Niri validates the id, activates the destination, verifies its exact keyboard
surface, installs a private regular Wayland clipboard offer and sends the chosen
shortcut in one compositor turn. The seat must have no held keys/modifiers or
keyboard grab. Shortcut keys must exist at the base level of the active layout.
The operation leaves the destination focused; it does not refocus the editor or
steal focus back after cancellation. Input methods and popup grabs are not
bypassed.

The ordinary selection is never replaced, saved into another source, or
cancelled for this operation. Its source identity, ownership, MIME types and
arbitrary binary contents remain intact. Primary selection is untouched. Both
`zwlr_data_control_v1` and `ext_data_control_v1` continue to offer the real
selection, including when a history client binds while the private offer is
active. Legitimate concurrent copies still replace the real selection normally
and cancel the private operation; cleanup re-offers the **current** real
selection, rather than restoring an old snapshot.

The supplied bytes are offered as `text/plain;charset=utf-8`, `text/plain` and
`UTF8_STRING`, all carrying identical UTF-8 bytes. The offer is sent only to
regular data devices of the destination's Wayland client on this seat. After the
first valid receive request, the real selection is re-offered immediately. Any
private offer object retained by the application can still make additional MIME
requests until it is destroyed or the operation is revoked. Asynchronous writes
remain independent of offer destruction. Completion follows both offer
destruction and writer completion; restoration does not depend on a sleep.
Applications that retain old offers indefinitely reach the cleanup deadline.

Wayland's privacy boundary is **a client and seat**, not a window or text widget.
Other windows belonging to the same Wayland connection can observe the private
offer. The focused widget within the exact destination surface is controlled by
the application; Niri cannot identify or pin it. An application can itself retain
or republish received text. The guarantee here concerns compositor distribution,
not the recipient application's handling of its data.

## X11 and unsupported clients

Xwayland-satellite receives ordinary Wayland selections and bridges them into the
shared X11 `CLIPBOARD`; a private native offer is not isolated there. This version
refuses known Xwayland bridge executables (including Nix wrappers) using the
destination connection's process credentials, independently of the window's
app-id. It does not use an app-id to choose a shortcut. Unknown/unreadable process
identity, missing regular clipboard devices, missing keyboards and unsupported
keyboard layouts also produce clear failures.

X11 private insertion is unsupported. No global X11 clipboard fallback is used.
Arbitrary custom or renamed proxy clients cannot be universally identified by
Wayland; they remain subject to the client/seat boundary above. This API is for
native destinations, not a general privacy guarantee for unknown clipboard
bridges. Browser/web/native editor coverage beyond the validated clients below
requires application testing; ordinary paste is not universally acknowledged.

## Dependencies and validation

The accompanying Smithay commit is
`75056ca436e29a2ef4db2aa6b612654beba4c554`, based on the fork's original dependency
`8a270232f1bfc416c781273c98a3f52a812a0bba`. It exposes non-mutating regular-device
enumeration, a real-selection change counter and re-offering of that selection.
The normal selection setter still cancels previous client sources; it must not
be used to implement a save/replace/restore scheme.

Both Smithay workspace dependencies pin a hosted revision from
`https://github.com/warrenfalk/smithay.git` containing that commit. The later
[text context feature](text-input-context.md) extends the pin without changing
this clipboard behavior. Cargo and Nix builds require no local Smithay checkout.

The Nix package also accepts the older `libdisplay-info` override as an alias
for the explicit `libdisplay-info_0_3` dependency, so the existing workstation
package override can build this revision. The default continues using ABI 0.3.

Focused protocol tests cover a preexisting binary source with multiple MIME
types, exact byte preservation and no cancellation; both data-control protocols
before/during/after private receives, including newly connecting histories;
large asynchronous transfers and multiple MIME requests; stale offer rejection;
focus changes, destination closure, IPC owner loss, concurrent copying, timeout,
backpressure, offer dismissal and broken pipes. IPC serialization and redacted
Debug formatting are also tested.

The full non-visual workspace suite (including EGL), workspace Clippy with all
targets, nightly rustfmt and the IPC `json-schema` feature check passed with the
exact dependency pin.

A nested Niri session validated a GTK 4 text view and Kitty with Unicode and
multiline text, while cliphist ran throughout. The PNG clipboard owner stayed
alive, MIME types and bytes were unchanged, and history contained only the real
selection. A second history watcher joining during an operation also received
the real selection. Real IPC checks exercised disconnect, timeout, focus change,
concurrent copying and destination closure. An xterm through xwayland-satellite
was explicitly refused and the real clipboard remained unchanged. The session
used isolated runtime/configuration/history files; the active workstation
compositor and configuration were not restarted or activated.

Keep these regressions with the feature when rebasing. Native widget insertion
is an application acceptance check distinct from transfer accounting. Full
input-method delivery, surrounding-text editing and speech/dictation policy are
outside this capability.
