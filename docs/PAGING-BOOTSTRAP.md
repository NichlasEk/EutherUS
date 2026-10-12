# Owned page tables and guarded stacks

The `paging` feature (included by `graphics`) switches CR3 to a hierarchy built
from frames owned by EutherUS. Four unmapped 4 KiB pages protect the lower and
upper boundaries of the 128 KiB kernel stack and 128 KiB double-fault IST stack.
The standard EutherBoot UEFI entry and internal BootInfo v3 are unchanged.

## Transition and ownership

1. The memory bootstrap audits/reserves firmware page tables and supplies only
   conventional, writable, identity-mapped RAM to the frame allocator.
2. The paging builder recursively copies every reachable table, allocating one
   physical frame per unique table. Non-leaf entries point only to new frames.
   Aliases at the same level reuse the copied table; recursive/cross-level table
   reuse is rejected. The bounded builder tracks at most 4096 frames, including
   tables added when splitting huge leaves. Its metadata is static kernel-image
   storage, not a large temporary on the stack being protected.
3. Leaf translations and rights remain inherited from the firmware policy. For
   each guard, the builder splits any 1 GiB/2 MiB leaf on its path, then clears
   only the target 4 KiB PTE. Splitting preserves physical continuity, rights and
   cache attributes, including the different PAT bit position for 4 KiB leaves.
4. An audit verifies that all non-leaf references use owned frames, that no new
   frame overlaps an original table, and that allocations are unique. Neighboring
   non-guard translations retain their physical address and effective RW/user/NX,
   PAT/cache/global/protection-key attributes. The four original mappings remain
   present in the untouched firmware hierarchy.
5. With interrupts disabled, the kernel clears CR4.PGE, installs the new CR3 and
   restores CR4. This invalidates global TLB entries as well as ordinary entries;
   changing CR3 alone would not suffice for guards previously cached as global.
   The current path requires four-level paging and PCID disabled, as in the
   accepted Q35/OVMF TCG configuration.
6. CR3 is read back and checked. Three real PIT ticks must arrive after the switch
   before initialization continues. The graphics arena is allocated afterward,
   so its free-page accounting includes the owned table frames.

The stack objects are page-aligned structures with separate lower guard, usable
storage and upper guard fields. Entry RSP and TSS.RSP0/IST1 reference only the
usable portions. Guard pages stay reserved within the kernel image; they are not
returned to the allocator. CPU-only variants use the same layout, but their
padding pages are not unmapped until the paging feature is active.

This is **owned table storage with a conservative inherited mapping policy**.
It is not yet a minimal kernel address space, per-process isolation, a higher-half
kernel or a public dynamic mapping API. Existing firmware mappings and old table
frames remain retained; no firmware memory is reclaimed. Owned tables remain
writable through their existing identity mapping. Five-level paging, PCID/SMP
TLB shootdowns and arbitrary physical platforms are outside this milestone.

## Actual fault acceptance

```sh
python3 scripts/test-paging-qemu.py --build
./eutherlab build --variant graphics
python3 scripts/test-regis-qemu.py
```

The paging suite builds source-identical variants and verifies in QEMU TCG:

- `paging`: live CR3 differs from firmware CR3 and matches the declared owned
  root; timer and boot succeed; two concurrent instances and warm reset work.
- `guard-low`, `guard-high`, `emergency-low`, `emergency-high`: a real write to
  each guard raises #PF (vector 14), with the exact CR2 address and error code 2
  (non-present supervisor write). The handler runs on the usable kernel stack
  and halts after reporting; the VM does not terminate in a triple fault.
- `stack-overflow`: the test positions RSP at the usable lower boundary and
  executes a push into the guard. #PF frame delivery cannot use that exhausted
  stack, so the CPU escalates to #DF (vector 8, error code zero). The existing
  TSS IST1 delivers the report from the protected emergency stack. CR2 lies in
  the kernel's lower guard. This is a forced boundary crossing, not a measured
  recursive application workload.

The emergency guards are tested by direct writes from the normal stack. Recovery
from exhaustion of the emergency stack itself is not promised. All fatal faults
are reported and halted; there is no recovery or instruction skipping.

Host tests additionally check all 512 entries of 2 MiB splits, rights/PAT
preservation, a two-stage 1 GiB→2 MiB→4 KiB split and invalid split requests.
The actual OVMF stack mapping exercised the large-leaf split in QEMU.

`eutherus.paging` version 1 emits `paging_ready` with old/new CR3, allocated table
pages, pool count and all guard addresses, followed by `timer_after_switch`.
The controller exposes `paging_events` in status. Reports contain immutable build
identity, firmware/toolchain/configuration, register evidence, exceptions and
cleanup; the latest pointer is `artifacts/paging-latest.json`. The original
known-good baseline is not automatically promoted.

Full regression:

```sh
python3 scripts/test-memory-qemu.py --build
python3 scripts/test-interrupts-qemu.py --build
python3 scripts/validate.py
```

Next: explicit task contexts and a small cooperative scheduler, with a separate
protected stack per task, before real task telemetry feeds System Regis.

Reference: [Intel system programming manuals](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html), paging, PAT, CR3/CR4, TLB invalidation and IST/exception delivery.
