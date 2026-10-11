# System Regis in the kernel

The opt-in `graphics` build boots **OVMF → existing EutherBoot → EutherUS**,
exits UEFI boot services, then renders a rotating ASCII tower directly into
GOP-provided framebuffer memory. This is actual Rust kernel execution in QEMU
TCG. It uses the same `regis-scene` geometry, perspective and shading helpers as
the native Linux prototype. The kernel screen is a smaller bootstrap interface;
process branches, the prompt and inspector still belong to the host prototype.
There is no scheduler or real process graph yet.

```sh
./eutherlab build --variant graphics
python3 scripts/test-regis-qemu.py
./eutherlab boot --vm regis --variant graphics
./eutherlab screenshot --vm regis
./eutherlab logs --vm regis
./eutherlab stop --vm regis
./eutherlab clean --vm regis
```

The lab runs headless. `screenshot` captures the actual QEMU display as PPM.
The acceptance script sends real emulated PS/2 keyboard input through QMP:
Space pauses/resumes rotation. It checks changing tower pixels, identical
paused screenshots, resume, boot event ordering, two overlapping graphics VMs,
input isolation and cleanup. Reports and original PPM captures are retained in
`artifacts/regis/qemu/`; `qemu-latest.json` points to the last report. Reports
embed the immutable build manifest, source digest, firmware/toolchain identity,
VM arguments and serial/EutherBoot logs. Run `scripts/validate.py` separately
for the original normal/panic/timeout/crash regression suite. Graphics does not
participate in known-good promotion yet.

## Internal handoff v2

The standard UEFI PE entry contract from EutherBoot is unchanged. The internal
`BootInfo` has the existing 48-byte v1 prefix, `version=2`, `size=96`, followed
by this 48-byte `repr(C)` framebuffer descriptor (little endian x86_64):

| Offset in BootInfo | Type | Meaning |
| --- | --- | --- |
| 48 | u64 | Framebuffer physical/identity-mapped address |
| 56 | u64 | Framebuffer byte capacity |
| 64 | u64 | Retained LOADER_DATA render arena address |
| 72 | u64 | Render arena allocation size in bytes |
| 80 | u32 | Width in pixels |
| 84 | u32 | Height in pixels |
| 88 | u32 | Hardware stride in pixels |
| 92 | u32 | 0 = RGB byte order, 1 = BGR byte order |

The entry selects a supported 1024×768 RGB/BGR mode, allocates a 6 MiB arena
(two tightly packed 32-bit software frames), copies the descriptor, closes the
GOP protocol borrow, then exits boot services. Unsupported modes/formats panic
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
report the frame number, pause state and angle in milliradians. They are
separate from the unchanged `eutherus.boot` v1 event protocol.

A polled PIT channel 2 provides a nominal 50 ms wait **in addition to render
time**, with the speaker disabled. This is not a measured 20 FPS claim. Polling
consumes guest CPU; it is temporary until we own the IDT and timer interrupts.
Keyboard handling is deliberately limited to Q35's translated PS/2 Space
make/break codes, with typematic suppression. There is no general input driver,
mouse, Unicode text input, power management or physical-hardware acceptance.

Next foundation: owned stack/exception handling, memory reservations and timer
interrupts before a scheduler and genuine process telemetry feed the tower.
