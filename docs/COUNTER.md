# Built-in counter lifecycle

`run counter` starts a single built-in kernel task in slot 2 and focuses its new
tower branch and inspector. The task sleeps for 50 PIT ticks before each of ten
steps (500 ticks total, nominally five seconds at 100 Hz). Graphics and input
continue running. The branch shows progress and remains inspectable as
`finished` after return; it is retained as the last result, not removed.

`run counter` while active returns `task_busy`, without changing selection,
generation or scheduler state. After completion, the same command starts a new
run in the same slot. Its generation increases, work/dispatch/sleep counters
reset, and its usable stack is cleared before a fresh entry context is installed.
No saved finished context is resumed. `tasks`, `tree`, `select counter`,
`inspect counter` (or ID 2) and Tab include the counter only after its first run.
Reset returns it to an absent, unstarted state.

This is a fixed-capacity demonstration, not a general process or executable
loader. All three tasks share kernel privilege and address space. There is no
kill operation, preemption, heap allocation, task unloading, per-task error
recovery or arbitrary code execution. Graphics/input remain essential: their
return is fatal. A counter return is normal; a counter fault is still fatal to
the entire kernel. IRQ handlers never schedule.

## Stack and ownership

Graphics builds reserve three aligned 128 KiB usable stacks in the kernel image,
each between two unmapped 4 KiB guards. All six guards are removed when owned
paging is activated, before any task starts. The third stack is reserved even
while dormant; memory output therefore reports 384 KiB of usable task stack
capacity. Guards are additional reserved storage. The free page pool does not
change on launch/restart, and the counter never owns the framebuffer arena.
Serial task-probe builds retain two slots and their existing 15-dispatch test.

Only input may call `start_counter`, without yielding. It accepts only the
finished/dormant slot, clears it, builds a fresh SysV entry frame with IF enabled
and default FP controls, resets activity and publishes Ready. The round-robin
scheduler sees it on its next pass. Completion changes state and emits events
before switching back to the separate scheduler stack. No mutable shared-state
reference survives a context switch.

## Evidence

`eutherus.tasks` v1 adds `counter_started` (generation, stack bounds and tick),
`counter_progress` (generation, 1..10 step, tick, actual RSP and checked stack
sentinel) and `counter_finished` (generation and final step count). The last is
followed by the existing `task_finished` for ID 2. `eutherus.graphics` v1 adds
`counter_presented` with the shown state, generation, work, dispatches, sleeps,
selection and simultaneous renderer/input progress. Ordinary task boot readiness
still describes the two essential tasks; the counter is announced on launch.

Run `python scripts/test-counter-qemu.py --build`. Actual TCG tests cover dormant
rejection, strict command arguments, busy rejection, ten timed steps on its own
stack, both existing tasks progressing, completion without later dispatch,
restart in the same stack with a new generation, read-only inspection, keyboard
selection, unchanged free pool, two-VM isolation and warm/fresh reset while
active. `graphics-counter-low` and `graphics-counter-high` intentionally write
the counter's guard after step two. Each must raise #PF with exact CR2/error code,
use the counter's own exception stack, halt and be classified as a failure even
though boot succeeded. These are separate experimental builds, not commands in
the normal UI.
