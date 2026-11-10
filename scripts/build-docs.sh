#!/usr/bin/env bash
# Builds the mdBook and copies rustdoc in beside it. No deploy.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

if ! command -v mdbook >/dev/null 2>&1; then
    echo "mdbook is not on PATH" >&2
    exit 1
fi

export RUSTDOCFLAGS="-D warnings"
rm -rf "$root/target/doc"
cargo doc --locked --no-deps -p rfc9162 -p rfc9162-tiles -p rfc9162-log
bash "$root/scripts/regen-doc-output.sh"

err=$(mktemp)
if ! mdbook build "$root/docs" 2>"$err"; then
    cat "$err" >&2
    exit 1
fi
if grep -E -i 'warning|error' "$err"; then
    cat "$err" >&2
    exit 1
fi
rm -f "$err"

rm -rf "$root/docs/book/api"
mkdir -p "$root/docs/book/api"
cp -a "$root/target/doc/." "$root/docs/book/api/"
