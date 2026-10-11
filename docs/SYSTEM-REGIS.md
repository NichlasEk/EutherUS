# System Regis — form study 01

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

The viewer rasterizes a 1440×900 RGB buffer in software, with a cached background
and cached glyph masks. Activity uses deterministic integer triangular waves.
Small glows and rings encode the selected node and simulated state. No GPU
shader or per-frame external image assets are required. Window presentation
uses a shared viewport for centered aspect fitting and pointer coordinates.
A 30 fps cap and approximately 10 Hz model updates bound normal animation.
F3 removes decorative activity glows; telemetry continues updating.

Bundled Liberation fonts retain their SIL Open Font License in
`crates/regis-preview/assets/FONT-LICENSE.txt`.

## Interact

- Click a process row or an allocated memory field to choose context.
- Up/down moves between nodes; the wheel scrolls the tree when it is full.
- Type in the always-active prompt and press Enter. Escape clears the input.
- F2 opens/closes the selected node's inspector. It is an attached detail pane,
  not an independent movable OS window.
- Inspector buttons pause/resume or stop the selected task.
- F3 toggles decorative motion; F10 saves `artifacts/regis/live.png`.
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
capacity refusal and tree scrolling. Unit tests cover model invariants and
wide/tall viewport rendering and pointer mapping.

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
The native test also passed in the visible host window before the scaling fix;
the final scaling version passed on the isolated display.

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
