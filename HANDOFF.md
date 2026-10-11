# EutherUS Lab milestone 1

The repository is independent at `EutherUS/`, with origin
`https://github.com/NichlasEk/EutherUS.git`. The initially inherited parent
repository belonged to EutherScryer; its remote was restored and it is not the
EutherUS source tree. No push is part of this milestone.

## Current scope

The lab milestone is complete. The user has now approved a first System Regis
form study: Linux-like modularity, a visually rich process/resource tree and a
living contextual prompt. The latest approved form is a rotating cylindrical
ASCII tower with process branches, stable labels and a camera that locks on selection. A separate native Rust prototype implements that
interaction with clearly labelled simulated data. It runs on the Linux host,
not in QEMU. An opt-in graphics kernel now renders the shared tower geometry after
ExitBootServices in QEMU; see docs/GRAPHICS-BOOTSTRAP.md. The full process UI
remains a host prototype.

Start `./scripts/regis-preview.sh`. Read `docs/SYSTEM-REGIS.md` for controls and
validation. The model is `crates/regis-model` (no_std + alloc), the CPU-rendered
native viewer is `crates/regis-preview`. Screenshots, host render benchmark and
native keyboard/mouse evidence are under ignored `artifacts/regis/`.

The approved baseline at `.lab/known-good.json` remains the original serial lab
milestone. Preview changes do not promote a new kernel baseline. The Rust
bootstrap and the Python host controller retain their existing responsibilities.
Further OS process, application, security and driver architecture remain open.

## Reproduce

```sh
./scripts/setup.sh
python3 scripts/validate.py --eutherboot ../EutherBoot
```

The normal path is OVMF → existing EutherBoot → Rust UEFI entry →
ExitBootServices → no_std kernel → structured COM1 success → controller stop.
Two overlapping VM instances, isolation, reset, screenshot, registers and GDB
are included in the same test suite. A separate real JSON-API lifecycle test
checks paused debug inspection, duplicate-name rejection and live-cleanup refusal. Fault variants cover panic, a kernel
boot timeout and triple fault. Results live in ignored `artifacts/reports/`;
read the JSON's `passed`, per-case outcomes and source identity.

For a clean committed baseline, run `python3 scripts/validate.py --promote`.
The known-good pointer is `.lab/known-good.json`. It references immutable build
and report artifacts; preserve them when moving this lab.

## Boundaries

This is the boot-and-observation milestone, not a complete OS. There is no
physical page allocator, scheduler, owned IDT/page tables, VirtIO kernel driver
or full kernel GUI yet. The graphics variant has a framebuffer tower and
bootstrap Space-key pause/resume, with a polled PIT timer. The kernel keeps firmware-provided mappings and stack and
halts with maskable interrupts disabled. See docs/HANDOFF-V1.md before adding
memory reclamation or interrupts. GDB register/memory/disassembly access works;
relocated Rust source-symbol loading is future work.

The EutherBoot sibling may contain unrelated uncommitted work. Builds record
its commit, dirty flag, content hash and source archive. They do not modify the
sibling checkout or installed EutherBoot. No host boot configuration, disk or
firmware settings are changed. All acceptance here is virtual TCG execution.

## Graphics continuation

Run `./eutherlab build --variant graphics` and
`python3 scripts/test-regis-qemu.py` for real dual-VM rendering/input acceptance.
`crates/regis-scene` is shared no_std geometry; `crates/kernel/src/graphics.rs`
owns the framebuffer bootstrap. Internal handoff v2 extends the unchanged
v1 prefix; normal/fault variants retain v1. No graphics baseline promotion.
