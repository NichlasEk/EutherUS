# EutherUS internal handoff v1

This ABI is between the UEFI entry stub and `kernel_main` within the same
EutherUS PE image. It is **not** a new interface required from EutherBoot.
EutherBoot uses the standard UEFI x86-64 image entry convention. The kernel
entry uses Rust's `extern "C"` ABI for its compiled target and never returns.
Do not use this internal entry as an external loader ABI without extending
and specifying the calling convention for that loader.

`BootInfoV1` is `repr(C)`, 48 bytes, aligned to 8 bytes, little endian:

| Offset | Type | Meaning |
| --- | --- | --- |
| 0 | `[u8; 8]` | ASCII `EUTHERUS` |
| 8 | `u32` | Version, exactly 1 |
| 12 | `u32` | Struct byte size, 48 |
| 16 | `u64` | Address of final UEFI descriptor buffer |
| 24 | `u64` | Buffer length in bytes |
| 32 | `u64` | Descriptor stride; do not assume native struct size |
| 40 | `u32` | UEFI memory descriptor version |
| 44 | `u32` | Reserved, zero |

Boot services have exited. The descriptor buffer is retained as LOADER_DATA
and is never freed. The BootInfo object lives on the retained firmware-provided
stack for the lifetime of the kernel's non-returning entry. Firmware page
tables, stack, GDT and IDT remain installed; maskable interrupts are disabled.
The kernel does not reclaim memory or invoke boot/runtime services. Addresses
use the existing identity mapping. These constraints deliberately bound the
bootstrap; they are not a general-purpose memory-manager contract.

Before reclaiming pages, add explicit image/stack/page-table/framebuffer/ACPI
reservations, install owned exception handling, and establish owned page tables
and stack. Advance the ABI version for incompatible layout or entry changes.

COM1 emits UTF-8 text and newline-delimited JSON:

```json
{"protocol":"eutherus.boot","version":1,"event":"uefi_entry"}
{"protocol":"eutherus.boot","version":1,"event":"boot_services_exited"}
{"protocol":"eutherus.boot","version":1,"event":"memory_map","bytes":5232,"descriptor_size":48}
{"protocol":"eutherus.boot","version":1,"event":"boot_success","kernel":"0.0.1","handoff":1}
```

Memory-map byte length is illustrative and varies with firmware allocations.
A panic substitutes `{"protocol":"eutherus.boot","version":1,"event":"panic","reason":"kernel_panic"}`
for success. Firmware text/ANSI sequences are not JSON and are ignored by the
parser. A valid version/protocol and full ordered sequence are required.
