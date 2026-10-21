# Compositor support added for wsprflow

The features below were added to this Niri fork specifically to support
**wsprflow desktop dictation**. Their public interfaces are reusable by other
trusted local applications; wsprflow's requirements drove their design.

Wsprflow owns microphone capture, recognition, vocabulary, draft editing and
dictation modes. Niri supplies compositor capabilities for the original
destination, shortcuts, text context and insertion. Each linked feature contract
defines its behavior, edge cases and validation requirements.

## Requirements and Niri features

| wsprflow requirement | Niri feature contract | Introducing commits |
| --- | --- | --- |
| Mark the original dictation destination while the draft editor or another app has keyboard focus. | [Connection-owned window marker](window-marker.md) | `7dfa11f9` |
| Insert dictated text without replacing the real clipboard or adding it to clipboard history, including in apps without text-input context. | [Destination-specific private paste](private-paste.md) | `0cca5a99` |
| Distinguish tapping Mod+D for hands-free dictation from holding it for release-to-send; wait for Mod as well as D to be released before sending. | [Spawn with key release](key-release-spawn.md) | `60c7a2df`, `209ed151` |
| Let a click inside the draft editor promote a held shortcut to hands-free dictation and place the caret while Mod is still held. | [Reserve modified mouse clicks](modified-mouse-gestures.md) | `e91abb35` |
| Read the destination's surrounding text/caret, adapt new words to it, and refuse insertion against stale field state. | [Destination text context](text-input-context.md) | `79354aaf` |
| Commit new words directly to a supported Wayland text field, avoiding Firefox GTK's private clipboard-read cancellation when the real offer is restored. | [Revision-guarded text-input insertion](text-input-commit.md) | `79354aaf` |

Keep this group, its focused tests and its Smithay dependencies together during
fork rebases. Wsprflow's clipboard preservation, destination lifetime and
release-to-send behavior depend on these contracts.

## Supporting Smithay fork changes

The supporting repository is
[`warrenfalk/smithay`](https://github.com/warrenfalk/smithay), published on branch
[`text-input-context`](https://github.com/warrenfalk/smithay/tree/text-input-context).
That branch contains both wsprflow support commits:

| wsprflow need | Smithay commit | Changed code and contract |
| --- | --- | --- |
| Niri must enumerate the destination client's regular clipboard devices and re-offer the real selection without changing or cancelling its source. | [`75056ca4`](https://github.com/warrenfalk/smithay/commit/75056ca436e29a2ef4db2aa6b612654beba4c554) — non-mutating client offer helpers | `src/wayland/selection/data_device/mod.rs`, `src/wayland/selection/seat_data.rs`; [PRIVATE-OFFERS.md](https://github.com/warrenfalk/smithay/blob/text-input-context/PRIVATE-OFFERS.md) |
| Niri must read committed text-input context, distinguish field activations and enable compositor mediation while preserving an external input method's lifecycle. | [`db81bd65`](https://github.com/warrenfalk/smithay/commit/db81bd653782221673230abddeafb2195ccdb2eb) — committed context and mediation support | `src/wayland/text_input/mod.rs`, `src/wayland/text_input/text_input_handle.rs`; [wf_text_input_context.md](https://github.com/warrenfalk/smithay/blob/text-input-context/wf_text_input_context.md) |

Both workspace dependencies, `smithay` and `smithay-drm-extras`, currently pin
`db81bd653782221673230abddeafb2195ccdb2eb` from that GitHub fork in
[`Cargo.toml`](../Cargo.toml); [`Cargo.lock`](../Cargo.lock) records the same
hosted revision. It includes the earlier `75056ca4` commit. Builds do not require
a local Smithay checkout.

The development checkout used for these changes is
`/home/warren/source/smithay`. The hosted revision above is the build reference.
