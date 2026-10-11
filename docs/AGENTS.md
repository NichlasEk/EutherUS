# Agent workflows

## Separate source trees

Start from a committed known-good milestone, then use short sibling worktree
paths so QMP and GDB UNIX socket paths remain within Linux's length limit:

```sh
git worktree add -b experiment/memory ../eus-memory main
git worktree add -b experiment/interrupts ../eus-irq main
```

In **each** worktree:

```sh
./scripts/setup.sh
./eutherlab build --eutherboot ../EutherBoot
./eutherlab boot --vm experiment
./eutherlab status --vm experiment
./eutherlab logs --vm experiment
./eutherlab debug --vm experiment --registers
./eutherlab stop --vm experiment
./eutherlab clean --vm experiment
```

The identical VM name is safe across these worktrees: runtime resources,
compiler output, toolchain storage and build manifests are local to each tree.
Git objects are shared by Git; working files and branches are independent.
Use project-scoped agent permissions. Do not give an agent host disk or firmware
access merely because QEMU guests are disposable.

## A single checkout with concurrent VMs

```sh
./eutherlab boot --vm memory-test
./eutherlab boot --vm driver-test
./eutherlab status --vm memory-test
./eutherlab status --vm driver-test
./eutherlab stop --vm memory-test
./eutherlab stop --vm driver-test
./eutherlab clean --vm memory-test
./eutherlab clean --vm driver-test
```

Compilation in a single checkout is serialized; concurrently editing its source
is not an isolated experiment. Use worktrees for source changes. The automated
suite starts both QEMUs before observing either and verifies that stopping one
does not stop the other.

## Regression and baseline policy

1. Develop and run `python3 scripts/validate.py` in the experiment worktree.
2. Inspect `artifacts/reports/*.json`; distinguish an expected negative outcome
   from a harness error, and check post-test cleanup.
3. Review source changes and commit the candidate.
4. Re-run `python3 scripts/validate.py --promote` from that clean commit.
5. Merge only through the project's normal reviewed Git workflow. The controller
   never merges branches, pushes commits or grants agents execution privileges.

All four variants must have identical EutherUS source identity. The normal
baseline pointer changes only after the complete suite succeeds. Stop/clean
never deletes immutable build artifacts or the known-good manifest.

## Next layer: user agent

The current observation interface already supplies screenshots, logs, crash
records and lifecycle control. Keyboard/mouse automation and a user-facing
kernel UI are future work. Do not mistake the retained EutherBoot framebuffer
for an EutherUS desktop or QEMU acceptance for physical hardware acceptance.
