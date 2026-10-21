# Destination text context for dictation

**wsprflow purpose:** Added specifically so wsprflow can adapt new dictation to
the destination's existing text and caret, then check the field again before
sending. Existing destination text stays intact. See the
[wsprflow feature overview](wsprflow.md) for its supporting Smithay changes.

A dictation integration can read surrounding text from a native Wayland field,
adapt its new words to that context, and condition private paste on the same
field state. Existing destination text is never edited by this API. Recognition,
formatting and draft policy remain in the integration.

## Structured IPC

Keep a dedicated connection to `$NIRI_SOCKET` open and send one JSON line:

```json
{"TextInputContext":{"id":12,"focus":false}}
```

`id` is a mapped, on-output Niri window. With `focus:false` (the default), it
must already have exact keyboard focus. `focus:true` activates the destination
before reading; it refuses held keys/modifiers and keyboard grabs before changing
focus, using the same input-busy error as private paste. Both require an unlocked
layout. Focus changes while reading refuse the request.

An available field returns, for example:

```json
{"Ok":{"TextInputContext":{"id":12,"revision":7,"text":"Existing words","cursor":14,"anchor":14,"content_hint":0,"content_purpose":0}}}
```

Cursor and anchor are **UTF-8 byte offsets**, including when a selection is
present. Text is the application's surrounding-text excerpt, at most 4000 bytes;
it need not be the whole field or document. Hints/purpose are text-input-v3's
numeric values. Hidden/sensitive hints, password/PIN purposes, missing committed
surrounding text/content type and invalid UTF-8 boundaries return
`{"Ok":{"TextInputContext":null}}`. Unsupported applications also return null.
No text is read through accessibility or the clipboard. There is no new config.

There is one context lease on the seat, owned by this connection. Another
connection cannot read, retarget or release it. A null `id` releases it. Closing
the connection (including its write half), entering EventStream, closing/unmapping
the window or locking releases it and invalidates its revision. Ordinary focus
changes clear cached client field state; refocusing requires a fresh read. Each
successful read replaces the lease's previous revision.

The lease enables compositor text-input mediation without occupying or replacing
the input-method-v2 slot. A real input method retains normal activation and
acknowledgement. When none is bound, a read acknowledges committed client state
with empty `done` events so Chromium can flush pending updates. These events send
no text, preedit or delete request. Lazy-binding clients receive enter as well.
Cleanup only removes mediation enabled by this lease.

Reads sample committed state every 10 ms, require 40 ms of unchanged field state,
and return null after 300 ms if no usable settled state arrives. This is a bounded
observation, not an application round-trip acknowledgement or a guarantee that
the client has reported every change. Only the focused, enabled, live protocol
object is readable; pending requests are invisible until commit. Disable, leave,
destruction and re-enable clear or distinguish field state, even for identical
text in two fields sharing a protocol object.

## Conditional private paste

Keep the context connection alive, then send `PasteText` on a separate connection
with optional `context_revision`:

```json
{"PasteText":{"id":12,"text":" this continues.","context_revision":7}}
```

Niri compares the window, exact surface, live text-input object, field activation,
surrounding text, offsets and content type to that revision. A stale or expired
revision returns `text input context changed before paste; review the draft and
send again` **before any focus change, offer or input injection**. No automatic
retry or clipboard fallback is performed. Omitting the field preserves existing
private-paste behavior. All private-paste delivery/uncertainty rules still apply:
data served is not proof of insertion, and client changes after preflight remain
possible.

The integration must respect the application's reported capabilities. Firefox's
GTK text-input path may collapse selections to one caret offset; this API does
not recover missing selection extent or authorize surrounding-text correction.
Selection replacement is still controlled by the receiving widget.

For fields supporting text input, prefer the separate
[revision-guarded text commit](text-input-commit.md) capability. Firefox GTK can
cancel a private clipboard read when the real selection is re-offered; transfer
accounting alone does not make that delivery path reliable for Firefox.

## Privacy and validation

Surrounding text stays in compositor/client memory and the requested IPC reply;
it is redacted from Debug output. The local IPC trust boundary is unchanged:
clients with socket access already have compositor control. Integrations should
avoid saving surrounding text in transcript/status files or command arguments.

Keep focused tests for committed versus pending state, lazy binding, field
re-enable/disable, ownership and disconnect cleanup, UTF-8/sensitive filtering,
stale-revision refusal before private offer creation, JSON compatibility and
redaction. Validate native Firefox and Chromium input, textarea and contenteditable
in an isolated off-screen compositor, with actual text insertion and unchanged
binary clipboard/history. Keep application acceptance separate from transfer
accounting. The Smithay fork exposes committed context and mediation control;
both Cargo dependencies must pin its hosted revision for reproducible builds.
The support revision is `db81bd653782221673230abddeafb2195ccdb2eb` from
`https://github.com/warrenfalk/smithay.git`, extending the existing private-paste
dependency without changing ordinary clipboard ownership.
