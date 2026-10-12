# Owned CPU state and interrupt-driven waiting

The `interrupts` feature (also enabled by `graphics`) installs an EutherUS
stack, GDT, TSS and IDT after ExitBootServices. The original `normal`, `panic`,
`timeout` and `crash` builds remain the small serial baseline. EutherBoot and
the internal BootInfo layouts did not change in the CPU step. The later
`memory` feature uses v3, documented in MEMORY-BOOTSTRAP.md.

## Ownership and startup

- A 128 KiB, 16-byte-aligned static kernel stack belongs to the retained PE image.
  The non-returning entry switches RSP, supplies Win64 shadow space and passes
  BootInfo in RCX. BootInfo itself still lives on the retained firmware stack.
- A private GDT contains kernel code/data and a 64-bit TSS. The task register is
  loaded explicitly. A separate 128 KiB static stack is assigned to TSS IST1.
- A private 256-entry IDT installs interrupt gates with the new kernel code
  selector. Double fault uses IST1, so its handler does not depend on the normal
  stack being usable. Rust's nightly `x86-interrupt` ABI preserves the interrupted
  context and emits the appropriate return path. Red-zone use stays disabled.
- An `int3` self-check must return and increment its counter exactly once. The
  live RSP must lie within the owned stack before `tables_ready` is emitted.
- The legacy 8259 PIC is remapped to vectors 32..47; only IRQ0 is unmasked.
  PIT channel 0 uses mode 2 and divisor 11932, nominally 100 Hz. The timer handler
  performs an atomic counter increment and PIC EOI; it neither logs nor draws.
  Spurious IRQ7/15 handlers check the ISR before acknowledging the relevant PIC.
- `wait_ticks` disables interrupts around the condition and uses adjacent
  `sti; hlt; cli`. STI's interrupt shadow covers HLT, closing the lost-wakeup
  window. Inline assembly acts as a compiler memory barrier. The function
  returns with interrupts enabled. Initialization requires three actual ticks.

The graphics renderer now waits five timer ticks between frames instead of
polling PIT channel 2. Rendering adds to this interval; this is not a 20 FPS
claim. The keyboard remains polled once per frame, with its IRQ masked. The
serial-only `interrupts` variant sleeps and emits a heartbeat every 100 ticks.
The reported `halt_waits` counts HLT attempts, not elapsed idle time or CPU load.

## Diagnostic protocol

Separate newline-delimited `eutherus.cpu`, version 1 events:

| Event | Evidence |
| --- | --- |
| `tables_ready` | Stack range, GDT/IDT addresses, emergency stack base, returning breakpoint |
| `timer_ready` | Nominal rate, observed ticks and HLT waits |
| `idle` | Advancing ticks and waits in the serial-only variant |
| `exception` | Vector, error code, RIP, CR2, handler RSP and emergency-stack membership |

Fatal exceptions report once and execute `cli; hlt`; they are not recovered or
silently skipped. CR2 is meaningful for page fault; other exceptions may show
its previous value. Exception vector 255 denotes the default unclassified gate.
Reserved vectors have no promised semantics. The controller exposes `cpu_events`
in status/observations and classifies a captured exception as `exception`,
rather than allowing an earlier boot-success event to hide it.

## Reproduce actual QEMU acceptance

```sh
python3 scripts/test-interrupts-qemu.py --build
./eutherlab build --variant graphics
python3 scripts/test-regis-qemu.py
python3 scripts/validate.py
```

The CPU suite pins source-identical build manifests and checks:

1. Live GDT/IDT/RSP registers match the owned structures, ticks progress, HLT is
   observed by QMP, and warm reset reinitializes working interrupts.
2. `UD2` produces vector 6; loading invalid DS produces vector 13 with the
   expected selector error code.
3. Reading unmapped canonical address `0x400000000000` produces vector 14,
   CR2 equal to that address and a non-present supervisor read error code.
4. Making the #GP gate absent before causing #GP produces vector 8, code zero,
   with the handler RSP inside the emergency stack. This tests exception
   escalation and IST switching, not stack-overflow guard pages.
5. With IRQ0 masked, initialization stays halted after `tables_ready` and cannot
   reach `timer_ready` or boot success. This negative control rules out a software
   counter or a polling fallback masquerading as interrupt wakeups.
6. Every allocated VM is stopped and its runtime resources removed.

Graphics acceptance additionally requires both timer ticks and HLT waits to
advance while the real rotating/paused/resumed images and input isolation pass.
Reports include build/source/toolchain/firmware identity, QEMU configuration,
serial logs, register observations and cleanup. Find the latest CPU report via
`artifacts/interrupts-latest.json`. No experimental variant is auto-promoted.

## Limits and next step

This is a single-core Q35/OVMF TCG bootstrap using the legacy PIC routing supplied
by that platform. APIC/IOAPIC setup, SMP, userspace, scheduling and preemption are
not implemented. Firmware page tables and all allocations remain retained;
the optional `memory` feature now provides a conservative conventional-memory
page allocator. There is still no firmware reclamation. The optional paging feature now
unmaps dedicated guard pages around both static stacks. The reserved static
stacks and descriptor tables are part of the kernel image, not free RAM.
NMI recovery and arbitrary physical platform startup are not acceptance claims.

See [memory bootstrap](MEMORY-BOOTSTRAP.md) for reservations and allocation.
See [paging bootstrap](PAGING-BOOTSTRAP.md) for the subsequent owned CR3 and
stack-guard milestone. Task scheduling remains future work.
The System Regis process tree remains simulated in the host preview.

## Primary references

- [Rust interrupt ABI](https://doc.rust-lang.org/unstable-book/language-features/abi-x86-interrupt.html)
- [Intel software developer manuals](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html), system programming: interrupt/exception gates, TSS/IST, STI and HLT.
