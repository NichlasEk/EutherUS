# System Regis — form study 02: the ASCII tower

System Regis is the proposed living control surface of EutherUS: a process tree,
visible resource ownership and a prompt whose context follows the selected node.
Modularity, responsiveness and a visually rich, coherent identity are the user’s
stated premises. This prototype explores that interaction without settling the
kernel, process, application, capability or driver architecture.

## What runs today

`regis-preview` is a real native Rust program running on the Linux host through
X11/XWayland. It is not a GIF, a browser page, a QEMU guest desktop, or a monitor
of host processes. A persistent banner identifies all system telemetry as
**simulated**. No entered text becomes a shell command. The QEMU kernel remains
the minimal serial bootstrap; the graphical prototype is separate.

```sh
./scripts/regis-preview.sh
```

The model is a separate `no_std` + `alloc` Rust crate, with no I/O or execution
authority. The preview owns a 256 MiB *accounting model*, not a real 256 MiB
allocation. Nodes own allocations, can create children, and stopping a branch
returns the sum of its owned memory. Pause affects the selected simulated task;
children are independent and are not recursively suspended. System nodes are
protected against stopping, pausing and memory release. There is a limit of
24 live nodes and five model event records.

The central view is a rotating cylindrical ASCII tower with masonry, ledges and
crenellations. Precomputed 3D surface samples are rotated, perspective-projected,
lit and depth-tested into a 118×37 character grid. The renderer writes ASCII
characters into the ordinary 1440×900 RGB framebuffer using cached font masks.
No GPU shader or per-frame external image assets are required.

Process branches use the simulation's actual parent-child relationships.
Activity pulses travel along those branches; back-facing geometry is dimmer and
occluded by nearer character cells. Labels remain in stable left/right rails
for reliable selection. At most eight labels/branches are displayed at once;
scroll or use the arrow keys to reach the rest, up to the model's 24-node cap.
The inspector always names the selected node's actual parent.

The tower slowly orbits when idle. Hovering over its viewport holds rotation.
Selecting a label or a front-facing endpoint eases its angular position toward
the viewer and locks it. Typing also holds the camera. F4 releases the lock;
move the pointer out of the tower and clear the prompt to let it orbit. F3
stops camera animation and branch pulses while keeping telemetry updates.
Stationary labels do not move during a focus turn. This is CPU-projected 3D,
not a recorded animation. The data remains simulated.

Activity samples use deterministic integer triangular waves. Window presentation
uses a shared viewport for centered aspect fitting and pointer coordinates.
A 30 fps cap and approximately 10 Hz model updates bound normal animation.


Bundled Liberation fonts retain their SIL Open Font License in
`crates/regis-preview/assets/FONT-LICENSE.txt`.

## Interact

- Click a stable process label, front-facing branch endpoint or allocated memory field to choose context.
- Up/down moves between nodes; the wheel scrolls the tree when it is full.
- Type in the always-active prompt and press Enter. Escape clears the input.
- F2 opens/closes the selected node's inspector. It is an attached detail pane,
  not an independent movable OS window.
- Inspector buttons pause/resume or stop the selected task.
- F3 toggles camera/pulse animation; F4 releases the camera lock; F10 saves `artifacts/regis/live.png`.
- Close the window or press Ctrl+Q to exit.

| Command | Effect in the simulation |
| --- | --- |
| `start Aurora 16` | Create a child under the selected node, owning 16 MiB; select it |
| `alloc 8` | Give the selected node another 8 MiB if capacity permits |
| `free 8` | Return 8 MiB owned by the selected non-system node |
| `pause` / `resume` | Stop/restart that node's simulated activity |
| `open` / `close` | Open/close the inspector |
| `stop` | Remove the selected non-system branch, return its memory and select its parent |
| `help` | Show supported commands |

Names are bounded and commands are parsed against an explicit vocabulary.
Invalid commands and exhausted capacity leave resource ownership unchanged.
There is no command chaining, script engine, filesystem access or host process
control behind this prompt. Character and key transitions share an ordered
input queue so a short Enter press is not lost between rendered frames.

## Verify and measure

Run with the project-local Rust toolchain created by `scripts/setup.sh`:

```sh
export CARGO_HOME="$PWD/.tools/cargo" RUSTUP_HOME="$PWD/.tools/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
cargo test --locked -p regis-model -p regis-preview
./scripts/regis-preview.sh --verify --benchmark --headless artifacts/regis/preview.png
```

The headless interaction scenario routes through the UI hit tests and prompt,
then validates subtree memory accounting, pause/resume, inspector behavior,
capacity refusal and tree scrolling. Unit tests cover model invariants, camera locking, depth ordering, label stability at
multiple angles and wide/tall viewport rendering and pointer mapping.

The benchmark warms glyph caches for 30 frames, measures 600 frames, and reports
median/p95/max rendering time plus host RSS in `artifacts/regis/benchmark.json`.
The initial target is p95 < 8 ms at 1440×900; this is a host measurement excluding
window presentation/compositor work, not a claim about QEMU or future hardware.
The live timing label includes UI/update work and aspect-fit scaling, but not
presentation or the frame limiter's sleep. `--frames N` bounds a native run.

For isolated native keyboard/mouse verification (recommended; requires Xvfb
and ImageMagick and does not interact with your desktop):

```sh
xvfb-run -a -s '-screen 0 1920x1200x24' python3 scripts/test-regis-native.py --spawn
```

This starts its own native app on a virtual X11 display, verifies tall and wide
window scaling with real mouse events, and closes only its own app at the end.
The tower test also checks actual orbit progression, focus settling and camera
stillness with reduced motion. Tests use real X11 input on that isolated display.

To test the window on your desktop explicitly:

```sh
./scripts/regis-preview.sh --state-file artifacts/regis/live-state.json
# In another terminal, while the preview is open:
python3 scripts/test-regis-native.py
```

The X11 test finds the uniquely named preview, takes focus, and refuses input if
focus changes. It types start/allocation/stop commands, clicks pause/resume,
toggles the inspector and checks actual exported application state. It verifies
that the 24 MiB test allocation is returned, then saves a screenshot and
`artifacts/regis/native-test.json`. The app remains open. Do not type in the
preview concurrently with this short automation sequence.

## Next discussion and next engineering step

Evaluate the interaction and visual language first. Then decide what a real
kernel/task telemetry contract should expose, who may issue actions and how
presentation remains independent from execution. A QEMU graphical milestone
will need a framebuffer handoff, input path and actual task/memory facilities.
Do not relabel the simulation as real kernel telemetry when connecting them.

Implementation references:
[Minifb](https://docs.rs/minifb/0.28.0/minifb/) and
[Fontdue](https://docs.rs/fontdue/0.9.3/fontdue/).
