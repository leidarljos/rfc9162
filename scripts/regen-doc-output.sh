#!/usr/bin/env bash
# Regenerates the transcripts included by the book. Run from anywhere.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
out=$root/docs/src/output
mkdir -p "$out"

cargo build --locked -p rfc9162-docs-examples -p rfc9162-log --bins

bins=(
    getting_started
    howto_tree
    tutorial
    howto_tiles
    howto_pack
    howto_log
    howto_checkpoint
    howto_sidecar
)
for bin in "${bins[@]}"; do
    cargo run --locked --quiet -p rfc9162-docs-examples --bin "$bin" >"$out/$bin.txt"
done
bash "$root/docs/examples/sidecar-demo.sh" >"$out/sidecar.txt"
