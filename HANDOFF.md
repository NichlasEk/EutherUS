# EutherUS Lab milestone 1

The repository is independent at `EutherUS/`, with origin
`https://github.com/NichlasEk/EutherUS.git`. The initially inherited parent
repository belonged to EutherScryer; its remote was restored and it is not the
EutherUS source tree. No push is part of this milestone.

## Current scope

The user has approved completing the lab as the starting point. Further OS
architecture and kernel functionality wait for a discussion of premises.
The Rust kernel is a minimal executable test payload for the lab. It does not
settle process, application, security, driver or user-interface architecture.
The current host controller uses Python's standard library.

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
or kernel GUI yet. The kernel keeps firmware-provided mappings and stack and
halts with maskable interrupts disabled. See docs/HANDOFF-V1.md before adding
memory reclamation or interrupts. GDB register/memory/disassembly access works;
relocated Rust source-symbol loading is future work.

The EutherBoot sibling may contain unrelated uncommitted work. Builds record
its commit, dirty flag, content hash and source archive. They do not modify the
sibling checkout or installed EutherBoot. No host boot configuration, disk or
firmware settings are changed. All acceptance here is virtual TCG execution.
