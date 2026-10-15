# Kernel console

F1 opens the bottom console dock; F1 or Esc closes it and preserves the draft.
The dark background, restrained teal accent and steady gold caret complement
System Regis. This is our software framebuffer UI, not a Ghostty port.

The prompt names the selected task: `regis/graphics >` or `regis/input >`.
Use `select` to choose a branch in the console. While focused, Space inserts a
space, and Tab, arrows and F2 do not change selection or rotation. Outside the
console, existing tower/inspector controls retain their behavior.

Commands (case insensitive, strict argument counts; lifecycle extension below):

- `help`: list commands.
- `tasks`: snapshots of both real kernel tasks.
- `memory`: free page pool after arena allocation, framebuffer arena size and
  statically reserved task stack capacity. These allocation values are captured
  during graphics initialization; no later allocation occurs in this prototype.
- `inspect [graphics|input|0|1]`: task state, wait, stack bounds and counters;
  omitted argument uses the selected branch and opens its panel.

Commands run in the input task, so its own snapshot is running. This differs
from the renderer's inspector, which sees the input task sleeping. Snapshot
reads never yield or modify scheduler state.

Input uses a US QWERTY ASCII subset (letters, Shift case, digits, space, minus,
period and slash), Enter and trailing Backspace. There is no cursor navigation,
history recall, Unicode, shell execution or external program loading. Repeated
makes are suppressed until key release. The line holds 64 characters and the
output ring eight lines, with no heap allocation. Overflow rejects the entire
command on Enter; Backspace permits correction. Unknown commands, extra
arguments and invalid task names produce visible errors without changing tasks.

`eutherus.prompt` v1 records `prompt_presented` (including hex-encoded draft,
focus, revision, context and bounded row count), `command_result`, `command_task`
and `command_memory`. Task/memory events precede their command result.

Run `python scripts/test-prompt-qemu.py --build` for real TCG keyboard, command,
error, focus, isolation and reset checks. Reports live under
`artifacts/regis/prompt`, with `artifacts/prompt-latest.json` pointing to the latest.

## Prompt-driven tower navigation

`select <graphics|input|0|1>` selects a real task and smoothly brings its tower
anchor to the front. `inspect [task]` selects it, focuses the camera and opens
its live inspector. `memory` replaces that panel with the boot allocation
snapshot and highlights the present task connections. `tree` prints the present tasks,
closes detail panels and releases the camera into overview, keeping the console
open. `help` and `tasks` leave the current view unchanged.

Closing the console with Esc or F1 also releases command-driven focus and its
panel, preserving selection and draft. Camera focus is separate from Space's
manual rotation pause: navigation can turn a manually paused tower, but releasing
focus preserves that pause. Otherwise rotation resumes from the current angle.
Invalid commands, invalid task names and overflow leave the view unchanged.

Labels and panels stay in screen coordinates while branch anchors are projected
from fixed points on the shared tower geometry. Graphics and input have opposite
anchors; focused rotation follows the shortest arc with bounded steps and
an ease-out ending at an exact angle. This is presentation state, not a scheduler
operation or a new process model. Keyboard-only inspector controls retain their
existing behavior when the console is closed.

`navigation_presented` in `eutherus.graphics` v1 reports the shown revision,
selection, mode (`overview`, `task`, `memory`), focus, settled flag, actual/target
angles in milliradians, panel visibility and allocation snapshot. It is emitted
on view changes, during movement, periodically at rest and in overview. Existing
`inspector_changed` now reports the shared view revision, including focus/mode
changes. `command_result.context` remains the context at command invocation;
`prompt_presented.context` is the new selection after the command.

`python scripts/test-navigation-qemu.py --build` verifies actual TCG turns,
bounded movement, stable text and held tower pixels, live panels, invalid-command
non-mutation, overview/Esc/F1 release, manual pause, two-VM isolation and resets.

`run counter` is the first controlled lifecycle command. It starts/restarts one
built-in task; see [counter lifecycle](COUNTER.md) for state, stack ownership,
busy errors and fatal fault boundaries. It focuses the new counter branch.
All inspection commands remain read-only with respect to task execution.
