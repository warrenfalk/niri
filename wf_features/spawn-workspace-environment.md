# Spawn Workspace Environment

This fork-local feature lets programs launched by niri know which workspace was
focused when niri launched them. The intended use case is local launchers,
terminals, shell hooks, or child processes that want a stable launch context
without trying to infer it from compositor state later.

## Environment Contract

- When niri launches a user command while the focused workspace has a name, the
  child process environment contains `NIRI_LAUNCHED_FROM_WORKSPACE`.
- The value is exactly the workspace name that was focused at launch time.
- When niri launches a user command while there is a focused workspace, the child
  process environment contains `NIRI_LAUNCHED_FROM_WORKSPACE_ID`.
- The ID value is the decimal workspace ID that niri exposes over IPC for that
  workspace in the current compositor session.
- Both values are snapshots. They must not change if the process keeps running
  after focus moves to another workspace, the workspace is renamed, or the
  workspace is reordered.
- When the focused workspace is unnamed, `NIRI_LAUNCHED_FROM_WORKSPACE` is absent
  from the child process environment, but `NIRI_LAUNCHED_FROM_WORKSPACE_ID`
  remains set when niri can identify the active workspace.
- The variable applies to user-command launch paths: configured startup commands,
  shell startup commands, key binding spawn actions, shell spawn actions, IPC
  spawn actions, and commands passed to niri at startup.
- These variables are not placement rules by themselves. Other tools may use
  them as launch context, but niri should not infer window placement from them
  unless a separate feature defines that behavior.

## Notes And Edge Cases

If a process launched from a named workspace later launches another process, the
variable naturally propagates through the normal process environment unless that
program changes it. This is expected and preserves the original niri launch
context.

The workspace ID is only durable for the current workspace object in the running
compositor session. It can be used with IPC workspace state to find the
workspace's current index or name, but callers must not treat it as stable across
niri restarts or workspace deletion and recreation.

If a launch happens before there is an active workspace, niri must remove both
variables for that child so a stale inherited value cannot masquerade as fresh
compositor context. If the active workspace exists but is unnamed, niri removes
only `NIRI_LAUNCHED_FROM_WORKSPACE`.

If a user also configures environment variables with these names, the niri
launch-context values win when available and stale inherited values are removed
when unavailable. This keeps `NIRI_LAUNCHED_FROM_WORKSPACE` and
`NIRI_LAUNCHED_FROM_WORKSPACE_ID` reserved for this feature's contract.

## Validation Expectations

Keep focused coverage for the environment mutation boundary. At minimum, tests
should verify that a named workspace sets both variables, an unnamed workspace
sets only `NIRI_LAUNCHED_FROM_WORKSPACE_ID`, and a missing workspace snapshot
removes both variables. Manual validation can launch a shell command from a
named workspace and print both variables, then repeat from an unnamed workspace
and confirm only the ID remains.
