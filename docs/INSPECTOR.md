# System Regis task inspector

The `graphics` kernel includes a read-only inspector for its real renderer and
input tasks. It does not use the simulated `regis-model` from the Linux preview.
It adds no process model, task control, text prompt or dynamic task lifetime.

## Controls and display

| Key | Effect |
| --- | --- |
| Tab | Select the next of the two tasks (wrap around) and open the inspector. |
| Left / Right | Select graphics / input and open the inspector. |
| F2 | Show or hide the inspector without changing selection. |
| Esc | Close the inspector; retain selection. |
| Space | Pause/resume tower rotation independently of selection and inspection. |

Graphics is selected initially, with the panel hidden. Selected branch title
and connector are gold and the title has a `>` marker. The sidebar shows task
name/id, actual scheduler state, wait reason and timer deadline (when applicable),
dispatch/yield/work counts, reserved stack size, stack start/end and guard size.
Stack end is exclusive. The displayed 128 KiB is reserved usable storage, not a
measurement of current or peak stack consumption. Both guards are 4 KiB.

The input task exclusively handles keyboard bytes and updates a packed
selection/visibility/revision value. Repeated makes do not trigger another action
until the corresponding break is consumed. The decoder distinguishes E0-prefixed
left/right from unprefixed keypad keys, consumes the E1 Pause sequence and
ignores unsupported keys. It remains a bounded Q35 translated PS/2 bootstrap,
not a general Unicode keyboard driver. Input processes at most 32 bytes per
turn. IRQ handlers still never schedule or access inspector state.

## Observation semantics

`tasks::inspect(id)` returns an owned snapshot, or `None` for an invalid or
uninitialized ID. It copies names, state and counters from the same scheduler
that runs the tasks. Stack bounds come from the guarded stack objects. It does
not yield or change scheduling state. No reference to mutable scheduler storage
escapes, and the sidebar exposes no task-control action. This is an API contract
inside the shared kernel address space, not a security boundary between tasks.

The renderer captures a snapshot before drawing and displays its observation
tick. It sees itself as `running` and normally sees the input task as `sleeping`
on its one-tick timer. If a timer deadline has arrived while graphics is running,
the snapshot still says sleeping: only the scheduler can make that task ready.
The deadline is a lower bound, not a promise of immediate dispatch. These are
observed states, not synthetic animations. Both tasks and panel counters continue
advancing when rotation is paused. Reading data does not pause the selected task.

The same copied snapshot produces the pixels and `inspector_presented` evidence
after presentation. Frame/work counters keep their existing pre-render semantics:
the currently rendered frame is counted as completed only after presentation.

## Serial contract

All new events use `eutherus.graphics` version 1, separate from unchanged boot
and scheduler event ordering. Existing fields retain their meanings.

- `inspector_changed`: input task id, `selected`, `visible`, monotonic `revision`
  and input observation `ticks`. Emitted only if selection or visibility changes.
- `frame_presented`: adds `selected_task`, `inspector_visible`,
  `selection_revision`. Emitted every 20 frames and on a view revision change.
- `inspector_presented`: emitted after a visible panel is presented, at the same
  cadence as `frame_presented`. Includes `frame`, `revision`, `id`, `name`, `state`,
  `wait`, nullable `deadline`, `snapshot_ticks`, `stack_base`, `stack_end`,
  `guard_low`, `guard_high`, `dispatches`, `yields`, and `work`.

State strings are `ready`, `running`, `sleeping`, `finished`; wait strings are
`dispatch`, `none`, or `timer`. Only sleeping has a non-null deadline. The current
essential graphics tasks do not normally finish. Fault handling is unchanged.

## Actual QEMU TCG acceptance

```sh
./eutherlab build --variant graphics
python3 scripts/test-inspector-qemu.py
```

The test boots two isolated VMs, sends real QMP keyboard input and compares
inspector snapshots with kernel task-ready bounds and per-frame counters. It
verifies actual running/sleeping states, timer deadlines, visible panel/name
changes, a blank sidebar when closed, live counters during rotation pause,
Tab wrapping, extended arrows, held/repeated makes, ignored keys, F2 and Esc.
The peer VM must retain default selection, visibility and rotation. Warm and
fresh reset must restore defaults even from an open inspector and paused tower.
Kernel startup also asserts that inspecting task ID 2 returns `None`.

Reports, build identity, VM configuration, snapshots, serial logs and PPM
captures are under `artifacts/regis/inspector/`; `artifacts/inspector-latest.json`
points to the latest result. Run the graphics, live graphics-fault, finite task,
paging, memory, CPU and baseline suites as regression. TCG is the acceptance
baseline; KVM and physical hardware are not covered by these results.

The kernel now has a bounded read-only [console](PROMPT.md), opened with F1. While it has focus, Esc closes it, Space types a space, and inspector selection keys are suspended.
