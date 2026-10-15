# System Regis in the kernel

The opt-in `graphics` build boots **OVMF → existing EutherBoot → EutherUS**,
exits UEFI boot services, then renders a rotating ASCII tower directly into
GOP-provided framebuffer memory. This is actual Rust kernel execution in QEMU
TCG. It uses the same `regis-scene` geometry, perspective and shading helpers as
the native Linux prototype. The kernel screen is a smaller bootstrap interface;
two task branches now show real kernel activity and a keyboard-controlled
inspector displays real task state. The general process graph still
belongs to the simulated host prototype.

```sh
./eutherlab build --variant graphics
python3 scripts/test-regis-qemu.py
python3 scripts/test-inspector-qemu.py
python3 scripts/test-graphics-faults-qemu.py --build
./eutherlab boot --vm regis --variant graphics
./eutherlab screenshot --vm regis
./eutherlab logs --vm regis
./eutherlab stop --vm regis
./eutherlab clean --vm regis
```

The lab runs headless. `screenshot` captures the actual QEMU display as PPM.
The acceptance script sends real emulated PS/2 keyboard input through QMP:
Space pauses/resumes rotation. It checks changing tower pixels, identical
paused tower pixels with changing task branches, resume, ordered boot/task events,
two overlapping graphics VMs, input isolation, warm/fresh reset and cleanup. Reports and original PPM captures are retained in
`artifacts/regis/qemu/`; `qemu-latest.json` points to the last report. Reports
embed the immutable build manifest, source digest, firmware/toolchain identity,
VM arguments and serial/EutherBoot logs. Run `scripts/validate.py` separately
for the original normal/panic/timeout/crash regression suite. Graphics does not
participate in known-good promotion yet.

## Framebuffer handoff (current v3; historical v2)

The standard UEFI PE entry contract from EutherBoot is unchanged. The internal
`BootInfo` v3 is 112 bytes: the 48-byte prefix, this 48-byte `repr(C)`
framebuffer descriptor, and image bounds. See [memory bootstrap](MEMORY-BOOTSTRAP.md)
for flags and the complete contract. Historical v2 had size 96.

| Offset in BootInfo | Type | Meaning |
| --- | --- | --- |
| 48 | u64 | Framebuffer physical/identity-mapped address |
| 56 | u64 | Framebuffer byte capacity |
| 64 | u64 | Render arena address; zero at v3 handoff |
| 72 | u64 | Render arena bytes; zero at v3 handoff |
| 80 | u32 | Width in pixels |
| 84 | u32 | Height in pixels |
| 88 | u32 | Hardware stride in pixels |
| 92 | u32 | 0 = RGB byte order, 1 = BGR byte order |

The entry selects a supported 1024×768 RGB/BGR mode, copies the descriptor,
closes the GOP protocol borrow, then exits boot services. The kernel allocates
a 6 MiB arena (two tightly packed 32-bit software frames) from its physical
page allocator after validating and reserving the memory map. Unsupported modes/formats panic
explicitly; bitmask/BltOnly modes are not silently reinterpreted. The kernel
checks descriptor size, dimensions, format, stride and capacities. Like v1,
all firmware mappings, the image, memory map, stack and arena remain reserved;
no memory is reclaimed. This is not a separately loadable ELF ABI.

Rendering uses no heap and makes no firmware calls. A depth buffer resolves
ASCII cells; a build-time rasterized Liberation Mono atlas supplies glyphs.
Each frame is drawn into RAM and compared against a shadow frame, so only
changed pixels are written through volatile stores to framebuffer memory.
There is no GPU driver, page flip or vblank synchronization; tearing remains
possible. `eutherus.graphics` v1 `frame_presented` diagnostics every 20 frames
report the frame number, pause state, angle in milliradians and the exact
scheduler activity snapshot rendered in the task branches. They are
separate from the unchanged `eutherus.boot` v1 event protocol.

## Graphics and input tasks

`graphics` includes `tasks -> paging -> memory -> interrupts`. A single scheduler
owns two statically allocated, separately guarded 128 KiB stacks. Task 0 owns the
framebuffer arena and renders; task 1 exclusively reads the PS/2 controller.
Boot success precedes scheduler startup; the graphics suite requires both tasks
to make actual progress afterward. No firmware calls or heap allocations occur.

The renderer sleeps for five PIT ticks after presenting. The input task drains
at most 32 bytes per dispatch and sleeps for one tick. Sleep records an absolute
wake deadline and yields to the scheduler; it does not HLT on the task stack.
When all live tasks sleep, the scheduler waits on its own stack using IRQ-driven
HLT. IRQ0 advances time but never schedules. Deadlines are lower bounds: a long
render can delay input and wakeups. This is cooperative scheduling, not a 20 FPS
or maximum-latency guarantee. A non-yielding task can starve the other.

Space changes only rotation. Both tasks, their live counters and the framebuffer
continue updating while the tower is paused. Input uses atomic pause/toggle
state; the renderer copies it before each frame. It owns the framebuffer alone,
and tasks never hold references to mutable scheduler state across a yield.
The legacy input driver recognizes translated Space, Tab, F2, Esc and extended
left/right make/break codes. It suppresses repeated makes until release, ignores
AUX/parity/timeout bytes, and consumes the six-byte Pause sequence without actions.
The bounded read-only text console is documented in [PROMPT.md](PROMPT.md).
There is no mouse, power management or physical-hardware acceptance.

Each branch displays real scheduler dispatches (`RUNS`), completed work
(`FRAMES` or bounded input `POLLS`), yields, and its reserved stack size. A pulse
marker is indexed by dispatch count. Counts are not CPU usage percentages;
input polls are not keystrokes. The renderer snapshots before drawing, so its
current frame is not counted as completed until after framebuffer presentation.
Serial fields `gfx_dispatches`, `gfx_yields`, `gfx_frames`, `input_dispatches`,
`input_yields`, `input_polls` carry that same snapshot; `toggles` records actual
Space transitions. The shared native preview remains explicitly simulated.

`renderer_ready` and `input_ready` record the real task RSP. `input_live` records
progress at poll 20. `rotation_toggled` reports actual input transitions.
The scheduler emits `task_ready` with names and stack bounds, `scheduler_ready`,
its first `scheduler_idle`, and the first timed `task_woke` for each task.
These events remain separate from the existing boot protocol.

Both tasks are essential and long-lived. Unexpected task return emits
`task_finished`, then panics on the scheduler stack; it cannot silently leave a
frozen/partly functioning UI. The `graphics-input-exit` fault variant proves this
in actual TCG. `graphics-input-guard` writes to task 1's lower guard after poll 20,
proving #PF on the active input stack with exact CR2/error code. Both tests require
prior boot success, rendered frames and input progress, then verify that the
controller reports the later fatal outcome and that the guest is halted. There
is no fault recovery or task restart. Reports are under `artifacts/reports/`,
with `artifacts/graphics-faults-latest.json` as the fault-suite pointer.

See [task context contract](TASKS-BOOTSTRAP.md), [owned page tables and guards](PAGING-BOOTSTRAP.md)
and [CPU bootstrap](CPU-BOOTSTRAP.md). Graphics has eight guards in total: two
each for the kernel, emergency, renderer and input stacks. Full process
isolation and dynamic task lifetimes remain future work.

The [task inspector](INSPECTOR.md) adds read-only selection and a live sidebar.
Tab or left/right selects a task and opens the sidebar; F2 toggles it, Esc closes
it, and Space continues to control rotation independently.
