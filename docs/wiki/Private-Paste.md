# Private paste (fork extension)

Integrations can send `PasteText` over structured Niri IPC to activate a known
native Wayland window, offer UTF-8 text privately and trigger `CtrlV` or
`CtrlShiftV`. The real clipboard and its history distribution remain intact.
Send text in the JSON body, never a process argument. Closing the requesting
connection cancels the operation.

The result reports transfer accounting; ordinary paste cannot acknowledge text
insertion. Offers are scoped to a Wayland client and seat, not a widget. Xwayland
clipboard bridges are unsupported and there is no clipboard-replacement fallback.

See [the complete fork contract](../../wf_features/private-paste.md) for exact
requests/replies, defaults, cleanup, failure semantics, privacy boundaries,
dependency revision and validation results.
