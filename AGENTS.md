# EutherUS development rules

- Read HANDOFF.md and docs/ARCHITECTURE.md before extending the lab.
- Scope execution to this checkout/worktree and its allocated virtual resources.
- Never modify host boot entries, physical disks, installed bootloaders or
  firmware settings. Physical-device work requires explicit user authorization.
- Existing EutherBoot is the boot entry point. Build a source snapshot; do not
  edit its sibling working copy as a side effect of EutherUS builds.
- Keep TCG as the acceptance baseline. Report KVM and physical testing separately.
- Preserve structured event ordering and prove failure paths in actual QEMU.
- Do not promote an experimental build without the full passing regression suite.
- Keep .tools, .lab, target, artifacts and worktrees ignored. Do not push generated
  firmware, bootloader binaries, disk images or logs into Git.
- Use separate Git worktrees for parallel source changes. Do not kill unrelated
  QEMU processes; the controller owns and verifies only its recorded VM PIDs.
