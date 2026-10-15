# Read-only kernel console

F1 opens the bottom console dock; F1 or Esc closes it and preserves the draft.
The dark background, restrained teal accent and steady gold caret complement
System Regis. This is our software framebuffer UI, not a Ghostty port.

The prompt names the selected task: `regis/graphics >` or `regis/input >`.
Select the branch before opening the console. While focused, Space inserts a
space, and Tab, arrows and F2 do not change selection or rotation. Outside the
console, existing tower/inspector controls retain their behavior.

Commands (case insensitive, strict argument counts):

- `help`: list commands.
- `tasks`: snapshots of both real kernel tasks.
- `memory`: free page pool after arena allocation, framebuffer arena size and
  statically reserved task stack capacity. These allocation values are captured
  during graphics initialization; no later allocation occurs in this prototype.
- `inspect [graphics|input|0|1]`: task state, wait, stack bounds and counters;
  omitted argument uses the selected branch.

Commands run in the input task, so its own snapshot is running. This differs
from the renderer's inspector, which sees the input task sleeping. Snapshot
reads never yield or modify scheduler state.

Input uses a US QWERTY ASCII subset (letters, Shift case, digits, space, minus,
period and slash), Enter and trailing Backspace. There is no cursor navigation,
history recall, Unicode, shell execution or dynamic program launch. Repeated
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
