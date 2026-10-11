#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="$PWD/.tools/cargo"
export RUSTUP_HOME="$PWD/.tools/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
for tool in python3 qemu-system-x86_64 qemu-img mkfs.fat mcopy mmd git curl; do
    command -v "$tool" >/dev/null || { echo "Missing dependency: $tool" >&2; exit 1; }
done
if [[ ! -x "$CARGO_HOME/bin/rustup" ]]; then
    mkdir -p .tools/bootstrap
    url=https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init
    curl --fail --location --proto '=https' --tlsv1.2 "$url" -o .tools/bootstrap/rustup-init
    curl --fail --location --proto '=https' --tlsv1.2 "$url.sha256" -o .tools/bootstrap/checksum
    python3 - <<'PY'
from pathlib import Path
import hashlib
expected = Path('.tools/bootstrap/checksum').read_text().split()[0]
actual = hashlib.sha256(Path('.tools/bootstrap/rustup-init').read_bytes()).hexdigest()
if actual != expected:
    raise SystemExit('rustup-init checksum mismatch')
PY
    chmod +x .tools/bootstrap/rustup-init
    .tools/bootstrap/rustup-init -y --no-modify-path --default-toolchain none --profile minimal
fi
rustup toolchain install nightly-2026-10-01 --profile minimal --component rustfmt --target x86_64-unknown-uefi
printf 'Local toolchain ready. Run ./eutherlab build --eutherboot /path/to/EutherBoot\n'
