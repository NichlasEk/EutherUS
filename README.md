# EutherUS

**The machine belongs to its owner.**

EutherUS is a new Rust operating-system project. Its first deliverable is
EutherLab: a real automated x86-64 hardware laboratory, with an intentionally
minimal kernel. No Linux kernel, GRUB, Unix userspace, or LLM is needed to run
the guest or its tests. The current controller runs on Linux.

```text
EutherLab → QEMU / OVMF → existing EutherBoot → EutherUS UEFI entry
                                                    ↓ ExitBootServices
                                             Rust no_std kernel
                                                    ↓ COM1 JSON
                                             test report + cleanup
```

The kernel initializes COM1, retains the final UEFI memory map, reports success
**after exiting boot services**, and halts with interrupts disabled. Memory
allocation, interrupt handling, scheduling and a graphical desktop are future
milestones. The firmware screen remains visible; screenshot support does not
imply a kernel graphics driver.

## System Regis graphical form study

A separate native Rust prototype explores the living process tree, contextual
prompt, activity traces and memory ownership. It runs on the Linux host with
**clearly labelled simulated data**, not inside the QEMU kernel.

```sh
./scripts/regis-preview.sh
```

Click a node, type `start Aurora 16`, try `alloc 8`, then `stop` and watch the
allocation return. See [System Regis prototype](docs/SYSTEM-REGIS.md) for controls,
headless rendering, measured budgets and native interaction tests.

## Run it

Required host tools: Python 3.11+, Git, QEMU system x86 and `qemu-img`, OVMF,
`dosfstools`, `mtools`, GDB, curl and a Rust toolchain. On Arch the relevant
packages include `qemu-system-x86`, `qemu-img`, `edk2-ovmf`, `dosfstools`,
`mtools`, `gdb`, `python`, `git`, and `curl`.

```sh
./scripts/setup.sh
./eutherlab build --eutherboot ../EutherBoot
./eutherlab test
```

Setup installs a project-local, pinned `nightly-2026-10-01` toolchain and does
not edit shell profiles. Build uses `Cargo.lock`. The existing EutherBoot
source is copied into an isolated build directory and compiled; its checkout
is not changed. Set `EUTHERBOOT_SOURCE` or pass `--eutherboot` when it is not
next to this checkout. The tested EutherBoot source is version 0.1.2 with the
existing EFI chainloader and `qemu-debug` feature.

OVMF defaults to `/usr/share/edk2/x64/OVMF_CODE.4m.fd` and
`OVMF_VARS.4m.fd`. Override with `EUTHERLAB_OVMF_CODE` and
`EUTHERLAB_OVMF_VARS` as a matching pair. Build snapshots and hashes both files.

For the complete suite (including three real fault-injection kernels):

```sh
python3 scripts/validate.py --eutherboot ../EutherBoot
```

This builds, packages and boots the normal kernel, proves two overlapping VM
lifetimes, tests isolated shutdown, screenshots, QMP register inspection, an
actual GDB connection, warm reset and clean reset, then tests panic, boot timeout,
triple fault and cleanup. It also exercises a real VM through the JSON API,
including duplicate-name rejection and preservation of a paused debug session. JSON reports and captured logs are under
`artifacts/reports/`. The expected timeout deliberately waits 30 seconds.
Failures return nonzero. No model is involved in orchestration.

## Drive a VM

```sh
./eutherlab boot --vm kernel-test
./eutherlab status --vm kernel-test
./eutherlab logs --vm kernel-test
./eutherlab screenshot --vm kernel-test
./eutherlab debug --vm kernel-test --registers --probe
./eutherlab crash --vm kernel-test
./eutherlab reset --vm kernel-test
./eutherlab reset --vm kernel-test --fresh
./eutherlab stop --vm kernel-test
./eutherlab clean --vm kernel-test
```

`boot` returns when QMP is ready; `status`/`logs` show subsequent kernel progress.
`test` waits for the boot event. Stop preserves logs. Clean refuses a live VM.
Warm reset keeps its writable disk; fresh reset discards disk and firmware
changes and boots the **same build**. Names and lifecycle operations are locked
per VM. Build artifacts are immutable snapshots; a build cannot replace the
image of an existing VM.

TCG is the default. `boot --accel kvm` is an optional explicit choice when
`/dev/kvm` is accessible. The milestone acceptance suite uses TCG. GDB and QMP
use private UNIX sockets instead of TCP ports, eliminating port allocation
races and network-exposed debug listeners.

For debugging from the first instruction, `boot --paused` supplies QEMU `-S`.
Use the GDB command returned by `debug`; issue `continue` in GDB when ready.
The current probe supports registers, memory and disassembly. Source-level
Rust debugging with relocated PE/CodeView symbols is not yet automated.

## Known-good builds and agent work

A successful full suite can promote an immutable build only when it was built
from a clean Git commit:

```sh
python3 scripts/validate.py --promote
./eutherlab boot --vm baseline --known-good
```

Promotion records the manifest and report hash in `.lab/known-good.json`.
Ordinary builds never replace that pointer. Preserve the referenced
`artifacts/builds/` and report when archiving a baseline. An experiment does
not merge or become the Git main branch automatically.

See [architecture](docs/ARCHITECTURE.md), [handoff v1](docs/HANDOFF-V1.md),
[JSON API](docs/API.md), [agent workflows](docs/AGENTS.md), and
[current handoff](HANDOFF.md).
