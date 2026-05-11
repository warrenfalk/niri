# Recent Windows Workspace Labels

This fork-local feature makes the recent windows switcher show where each
candidate window lives. Whenever the Alt-Tab picker is visible, every displayed
window preview should have a workspace label above it, while the window title
remains below the preview.

## Desired Behavior

- The label is shown for every window preview in the recent windows picker.
- The label identifies the workspace that currently contains that window.
- Unnamed workspaces are identified by their one-based workspace position on
  their output, using the form `Workspace N`.
- Named workspaces include both the one-based position and the configured
  workspace name, using the form `Workspace N: name`.
- The same labeling behavior applies in all picker scopes and filters:
  all windows, current output, current workspace, and app-id filtering.
- Long workspace labels should not resize the preview. They may be clipped or
  faded in the same spirit as long window titles.
- Window previews remain the primary visual target. The workspace label should
  be smaller or less visually prominent than the window title and should not
  obscure the preview itself.
- Pointer hit testing should treat the label area as part of the window preview
  target, so clicking or hovering over the label selects the same window.

## Notes And Edge Cases

Workspace numbering is dynamic and output-local, matching niri's workspace
model. If a workspace moves or the workspace order changes, the label should
describe the workspace's current position in that output's workspace list.

If a window is moved to a different workspace while the picker is open, the
label should reflect the workspace that contains the window when the picker is
next refreshed. Opening the picker with correct labels is required.

Blocked-out or privacy-sensitive window previews should not leak extra
information through these labels beyond what the picker is otherwise allowed to
show.

## Validation Expectations

Keep focused coverage with this feature. At minimum, tests should lock the label
format for unnamed and named workspaces. If the picker layout or hit testing is
rewritten, add coverage or manual validation notes showing that labels remain
above each preview, that long labels do not change preview sizing, and that the
label area still selects the corresponding window.
