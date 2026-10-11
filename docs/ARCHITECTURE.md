# EutherLab architecture

The Rust workspace contains the no_std kernel. The controller is Python's
standard library, keeping orchestration separate from guest code and avoiding
an HTTP service or dependency on an LLM. `eutherlab api` provides a versioned
JSON-lines request/response interface suitable for an eventual MCP adapter.

## Boot contract

1. QEMU q35, one x86-64 CPU, 256 MiB RAM, TCG and OVMF.
2. OVMF discovers a disposable 64 MiB FAT32 boot volume over VirtIO block.
3. `EFI/BOOT/BOOTX64.EFI` is the existing EutherBoot, built from its source.
4. EutherBoot reads `EFI/EutherBoot/eutherboot.toml`, waits one second and
   calls UEFI `LoadImage`/`StartImage` for `EFI/EutherUS/kernel.efi`.
5. EutherUS enters through the standard UEFI ABI, initializes COM1, disables
   the firmware watchdog, obtains the final memory map through the uefi crate's
   `ExitBootServices` wrapper, disables maskable interrupts and enters its kernel.
6. The kernel validates internal BootInfoV1, reports its initialization and
   memory-map metadata, emits `boot_success`, and loops on `cli; hlt`.

EutherBoot needs no new proprietary loader protocol for this milestone.
The versioned *internal* EutherUS handoff is specified separately. Standard
`x86_64-unknown-uefi` already supplies the correct PE/COFF ABI; inventing a
custom target is unnecessary here. Red-zone use is explicitly disabled.
A later ELF kernel, independent stack/page tables and loader split will need
an expanded handoff rather than reuse of a Linux boot protocol.

## Isolation and ownership

Each worktree has its own `.lab/`, `target/` and `artifacts/`. Each VM has a
private mode-0700 runtime directory holding a qcow2 overlay, copied writable
OVMF variables, QMP socket, GDB socket, serial log, EutherBoot debug log and
QEMU stderr. All backing artifacts are snapshots with SHA-256 verification
before launch. No host disk, filesystem passthrough, USB device, host boot
entry or firmware variable is attached. QEMU uses `-nic none`, `-nodefaults`
and its seccomp sandbox. VirtIO block is already attached; network and display
drivers remain future kernel work.

UNIX sockets replace debug TCP ports. Keep workspace paths short enough for
the UNIX socket path limit; the controller rejects overlong paths. Names are
bounded and exclude separators. Per-VM flock locks serialize competing
lifecycle commands; fresh reset retains that lock throughout stop/clean/boot.
A separate build lock protects compiler outputs and snapshot creation. PID
start ticks prevent signalling a recycled PID. Missing/stale state is not
interpreted as authority to kill unrelated processes. Never use broad `pkill`.

These are VM lifecycle safeguards, **not a sandbox for a coding agent with
arbitrary shell access**. Run agents with workspace-scoped permissions. The API
is trusted local stdio, not a multi-user security boundary; the same host user
can modify its state or source. Rust build scripts are trusted project code.
Physical devices require a separate explicit user instruction and do not have
an API method here. QEMU escape vulnerabilities are outside this controller's
isolation guarantees.

## Results and reproducibility

Every build records the EutherUS Git commit (or null before the first commit),
dirty flag and content digest, EutherBoot commit/dirty flag/content digest,
exact Rust compiler details, QEMU version, firmware package version and content
hashes, kernel/bootloader/image hashes and an EutherBoot source archive.
The archive matters when the sibling bootloader has uncommitted work.

Reports embed those manifests, complete QEMU argv, selected VM configuration,
serial/debug/stderr logs, ordered events, durations, expected and observed
outcomes, exit code when the controller owns the child, isolation resource
identities and cleanup results. JSON is the durable evidence. PIDs, UUIDs,
FAT IDs and timing vary; images are content-addressed for identification, not
claimed to be byte-reproducible across builds.

The suite pins all build IDs before starting. Later builds cannot change the
candidate under test or the candidate selected for known-good promotion.
Promotion also requires the current clean checkout to match the tested source.

A successful boot requires the exact event sequence and startup banner, with
no panic and a live QEMU process at observation. Negative tests require the
kernel to have reached the post-UEFI memory-map event, preventing a stuck
firmware from masquerading as the intentional kernel timeout. A Rust panic
emits JSON, timeout halts before success, and crash deliberately installs an
empty IDT then executes `int3`. `-no-reboot` makes the triple fault terminate
QEMU. Termination identifies VM failure, not a general physical crash cause.

Warm reset pauses execution and advances the serial read offset so old boot
success cannot satisfy a new run. QMP reset briefly changes the reboot action,
waits for RESET, then reinstates shutdown-on-reboot before resuming the CPU.
Fresh reset reconstructs variables and disk from the original immutable build.

## Primary references

- [Rust UEFI target](https://doc.rust-lang.org/rustc/platform-support/unknown-uefi.html)
- [uefi 0.40 ExitBootServices wrapper](https://docs.rs/uefi/0.40.0/uefi/boot/fn.exit_boot_services.html)
- [UEFI boot services specification](https://uefi.org/specs/UEFI/2.10/07_Services_Boot_Services.html)
- [QEMU QMP commands and events](https://www.qemu.org/docs/master/interop/qemu-qmp-ref.html)
