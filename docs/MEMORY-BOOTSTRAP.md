# Physical page allocation bootstrap

The `memory` feature supplies a no-heap physical frame allocator after
ExitBootServices. `graphics` now depends on it and obtains its 6 MiB render arena
from kernel-owned frame bookkeeping, rather than calling UEFI AllocatePages.
The display shows real **allocator-pool** free KiB and arena KiB. These are not
process telemetry or total system free-memory figures.

## Eligibility, reservations and mappings

`regis-memory` is a no_std library with a stride-aware UEFI descriptor parser and
contiguous 4 KiB allocation. It never dereferences physical addresses. The kernel
adapter owns that responsibility and requires all of the following:

- Descriptor version 1; stride at least 40 and a multiple of eight; a nonempty,
  exact-length map with at most 4096 entries. Misalignment, zero-page descriptors,
  address/count overflow and overlapping descriptors fail before exposing pages.
  Descriptor order need not be sorted.
- Only type 7 (`ConventionalMemory`) is considered. Runtime, nonvolatile and
  special-purpose descriptors are excluded. Every other UEFI memory type remains
  reserved, including loader/boot-service memory, ACPI, runtime services and MMIO.
- The bounded bootstrap tracks physical addresses below 1 GiB with 256 KiB of
  static metadata in the retained kernel image. Memory above that limit is not
  managed. The first MiB is always reserved.
- Explicit reservations additionally cover the complete loaded image (including
  static stacks/GDT/IDT/TSS and allocator metadata), the full descriptor containing
  retained BootInfo/firmware stack, the full descriptor containing the final
  memory-map buffer, the GOP framebuffer, and every reachable active page-table
  frame. Reservations include intersecting partial pages.
- The adapter audits the current four-level CR3 tree, bounded to 4096 unique table
  frames, and reserves them before allocating. It accepts 4 KiB, 2 MiB and 1 GiB
  leaf mappings. Table frames must be in mapped RAM; read-only tables are valid
  because they are only inspected. In the tested OVMF build the tables are
  read-only. No page-table entry is modified and CR3 is not replaced.
- Candidate pages must translate to the same physical address and be present and
  writable through every paging level. Others are removed from the pool before
  any returned frame is written. This adapter relies on the existing Q35/OVMF
  identity mapping for reading page-table frames, then checks that mapping; it
  is not an arbitrary firmware virtual-address translator.

The pool uses byte states for reserved/free and allocation boundaries. Contiguous
allocations never bridge holes. Zero-sized requests, double frees, misaligned or
partial frees, and reservations overlapping live blocks are rejected without
changing accounting. Freed neighboring runs can be allocated together. Exhaustion
returns an error and leaves state unchanged. The low-level release API requires
caller ownership of the complete live allocation; it does not protect against a
stale address reused for another allocation (ABA), supply reference counting or
zero pages on release. Exclusive mutable access serializes operations; interrupt
handlers do not call the allocator. There is no global heap or SMP allocator.

## Internal handoff v3

The UEFI EutherBoot entry contract is unchanged. `memory` and `graphics` now use
an explicitly versioned 112-byte internal BootInfo; other variants retain v1.
The first 48 bytes preserve the v1 field offsets. Offset 8 is version 3, offset
12 is size 112, and the former reserved u32 at offset 44 is flags: bit 0 means
framebuffer present, all other bits zero. Memory-only builds use zero flags.

Offsets 48..95 retain the framebuffer layout documented in
[graphics bootstrap](GRAPHICS-BOOTSTRAP.md). At handoff, back_buffer and
back_buffer_bytes are **zero**, even with graphics. Memory-only builds zero the
entire framebuffer descriptor. After memory initialization, the kernel allocates
and fills a local copy of those arena fields before reporting boot success.

| Offset | Type | v3 addition |
| --- | --- | --- |
| 96 | u64 | LoadedImage image base address |
| 104 | u64 | LoadedImage image size in bytes |

The entry reads LoadedImage while boot services are still active and closes the
protocol borrow before exit. BootInfo and map allocations remain retained. The
image bounds cover the static kernel/emergency stacks and CPU/allocator tables.
The historical v2 graphics build used a UEFI-allocated arena and size 96; v3 is an
incompatible extension and does not mislabel itself as v2.

## Validation and diagnostics

```sh
# With the project-local Rust environment (see scripts/setup.sh):
cargo test --locked -p regis-memory
python3 scripts/test-memory-qemu.py --build
./eutherlab build --variant graphics
python3 scripts/test-regis-qemu.py
python3 scripts/test-interrupts-qemu.py --build
python3 scripts/validate.py
```

Host tests cover padded descriptors, unsupported versions, overlaps, overflow,
excluded types/attributes, bounded capacity, reservations, invalid frees,
fragmentation/exhaustion/coalescence and 5000 mixed operations with pairwise
non-overlap and exact accounting checks.

Actual QEMU memory tests write and read different patterns across two disjoint
allocations totaling five pages, reject bad frees, exhaust the entire eligible
pool's **bookkeeping** with single-page allocations, and restore exactly its
original free count. Exhaustion does not write every RAM page. Two live VM pools,
warm reset and peer survival are checked. Separate builds exercise an empty pool
(controlled OOM halt without boot success) and a deliberately unsupported map
version (rejection and panic before allocator initialization).

Graphics tests additionally require the 1536-page arena to consume exactly that
many free pages, avoid every explicit reservation, and still pass real pixel
rotation, keyboard pause/resume, timer progress and dual-VM input isolation.

`eutherus.memory` version 1 events are `reserved`, `allocator_ready`,
`self_test_pass`, `graphics_allocated`, `out_of_memory` and `map_rejected`.
The controller exposes `memory_events`; OOM cannot be hidden by an earlier
boot-success marker. Panics now also include a plain-text Rust location/reason
before the existing structured panic event. Reports retain source, toolchain,
firmware, VM configuration, logs and cleanup; latest memory evidence is pointed
to by `artifacts/memory-latest.json`. No experimental baseline is auto-promoted.

## Remaining work

No firmware memory is reclaimed. ACPI/runtime/boot-service regions remain intact.
There are no owned page tables, guard pages, per-process address spaces, scheduler
or userspace isolation. Next is constructing owned mappings and adding guard-page
fault tests before reclaiming anything or creating real tasks. The host process
tree remains simulated.

Reference: the pinned uefi-raw 0.16.0 `MemoryDescriptor` layout and
[UEFI memory-map specification](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html).
