# Cooperative task bootstrap

The optional `tasks` variant adds two kernel tasks after owned paging and before
`boot_success`. It uses the existing EutherBoot entry and BootInfo v3. The
`graphics` variant now reuses the same scheduler for long-lived renderer/input
tasks and real tower telemetry; the Linux preview still uses simulated data.

## Execution contract

This is a bounded single-CPU experiment: two statically allocated tasks share
the kernel address space and run round-robin. Each has a page-aligned 128 KiB
usable stack and two unmapped 4 KiB guards, reserved within the kernel image.
Paging removes all four task guards before any task runs, using the same owned
hierarchy, neighbor audit and TLB invalidation as the kernel/emergency guards.
Stack storage is not dynamically allocated or reclaimed.

A task transitions `Ready -> Running -> Ready` when yielding,
`Running -> Sleeping(deadline) -> Ready` for a timed sleep, and
`Running -> Finished` when its body returns. Finished tasks are skipped. When
both finite probe tasks finish, the scheduler returns to boot and the kernel
enters interrupt-driven idle. The first probe task yields eight times, the second five: there are 15 dispatches,
including each task's initial entry and final return. IRQ handlers never schedule.
A task that does not yield can starve its peer; there is no preemption, blocking
queue, cancellation, spawn API, userspace or process isolation.

`switch` is a naked function with an explicit SysV64 ABI, even though the outer
UEFI kernel uses Win64. The compiler owns transitions between these calling
conventions. A saved context represents a suspended function call and preserves:

- RSP and the return instruction pointer;
- SysV nonvolatile GPRs RBX, RBP and R12–R15;
- RFLAGS, including each context's interrupt-enable state;
- MXCSR and the x87 control word.

Caller-saved registers are the caller's responsibility. This is not a complete
asynchronous CPU/FPU context and must not be reused as an interrupt/preemption
switch. FS/GS bases, CR3, descriptor tables and privilege remain shared. Rust's
Win64-to-SysV call boundary preserves Win64 nonvolatile registers as required.
The no-red-zone build setting remains mandatory.

The saved stack layout is `MXCSR:u32, FCW:u16, padding:u16`, followed by u64
slots `r15, r14, r13, r12, rbx, rbp, rflags, return_rip`. Initial entry is built
with RSP aligned to 8 modulo 16 after RET; entry never returns through the dummy
caller slot. IRQs are disabled only during stack transfer, then restored from
the destination context. The timer never accesses scheduler state. Shared
context/state storage is accessed without retaining Rust references over a
switch; this relies on single-CPU, non-reentrant execution.

## Acceptance in actual QEMU TCG

```sh
python3 scripts/test-tasks-qemu.py --build
```

The suite builds immutable, source-identical variants and captures manifests,
ordered events, serial logs, QEMU configuration and cleanup in
`artifacts/reports/tasks-*.json`, with `artifacts/tasks-latest.json` as a pointer.

The normal case checks exact progress ordering, unequal task lifetimes and
return to kernel idle. An assembly probe keeps distinct sentinels in all six
nonvolatile GPRs across every yield. Volatile stack words contain per-task and
per-round patterns. Tasks use distinct FP rounding control values, and one yields
with interrupts disabled; task and scheduler assertions check preservation.
Real PIT ticks advance while tasks execute. Two concurrent VMs, warm and fresh
reset, and stopping one without stopping its peer exercise the lifecycle.

`task0-low`, `task0-high`, `task1-low` and `task1-high` write to each task guard
from the selected task after both have made real progress. Each must produce
#PF, error code 2 and the exact CR2 guard address, with the handler on that task's
usable stack. `task-overflow` positions task 1's RSP at its lower usable boundary
and executes PUSH. Failed #PF delivery escalates to #DF on the existing emergency
IST stack; the test checks vector, CR2 and handler RSP. It is a forced boundary
crossing, not a recursive workload. Faults halt the guest without boot success;
there is no recovery and no triple-fault termination is accepted.

`eutherus.tasks` version 1 emits `task_ready` (stack/guard ranges),
`scheduler_ready`, `task_progress`, `task_finished` and `tasks_complete`.
Graphics also uses `scheduler_idle` and the first `task_woke` per task;
`task_ready` includes the task name.
The controller exposes these as `task_events` in `status`. The existing
`eutherus.boot` event order remains unchanged.

Run paging, graphics, memory, CPU and original lab regression before treating
this as a verified milestone. This experiment does not automatically promote
`.lab/known-good.json`. KVM and physical hardware remain separate gates.

## Long-lived graphics tasks

`run_with` accepts two static task descriptors (name and body function).
The graphics variant uses `graphics` and `input` instead of the finite probes.
`tasks::sleep` takes a nonzero u32 tick count; the scheduler compares wrapping
u64 deadlines with signed differences (well within the half-range bound).
It never resumes a sleeper before its deadline. If no task is runnable but a
sleeper remains, it waits one IRQ tick with HLT and rescans. IRQ handlers do not
access scheduler state. Unexpected return of either essential graphics task is
fatal; finite probes retain their normal completion behavior.

Per-task dispatch/yield/sleep/work counters are updated at the real operation.
Callers receive copied activity snapshots; graphics owns its arena across sleeps,
and input never accesses that arena. Shared renderer configuration is initialized
once before scheduler startup and copied into the renderer's own stack.
See [graphics bootstrap](GRAPHICS-BOOTSTRAP.md) for UI semantics, sleep cadence,
post-boot fault tests and limitations. This does not add a dynamic spawn API or
an asynchronous/preemptive context switch.

## Read-only inspection

`inspect(id)` copies the registered name, current state, wait reason/deadline,
observation tick, reserved stack/guard bounds and activity counters. It returns
`None` for an unknown or uninitialized task. It does not yield, allocate, change
state or expose references to scheduler storage. On the single CPU, only the
timer tick can advance during the copy. The graphics observer sees itself as
running and its input peer normally sleeping; a due timer is not relabelled
ready until the scheduler actually transitions it. See [inspector](INSPECTOR.md).

## Counter extension in graphics builds

The serial probes still use two slots. Graphics now reserves a third guarded
slot for a built-in restartable counter. Only the first two tasks are essential;
normal return from the counter is retained as Finished and allows a fresh start.
See [counter lifecycle](COUNTER.md) for initialization, reuse, generation and
real QEMU tests of both additional guards.
