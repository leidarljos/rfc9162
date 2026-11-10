#!/usr/bin/env bash
# Runs rfc9162-sidecar and prints the transcript the how-to page includes.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
sidecar="$root/target/debug/rfc9162-sidecar"
demo_key="$root/target/debug/demo_key"
show="$root/target/debug/show_checkpoint"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
last=$work/last.out

side() {
    printf '$ rfc9162-sidecar'
    local arg
    for arg in "$@"; do
        printf ' %q' "$arg"
    done
    printf '\n'
    "$sidecar" "$@" | tee "$last"
}

"$demo_key" demo.pem example.org/demo >/dev/null
mkdir -p demo/ssh
printf 'Port 22\n' >demo/ssh/sshd_config
printf 'hello\n' >demo/motd

side init --dir demo
printf '\n'
side sync --dir demo --meta actor=root
printf '\n'
side snapshot --dir demo --key demo.pem --origin example.org/demo
printf '\n'

printf 'Port 2222\n' >demo/ssh/sshd_config
rm demo/motd
printf 'new\n' >demo/issue
side sync --dir demo --meta actor=root
size=$(awk '/^synced:/ { print $NF }' "$last")
printf '\n'
side history --dir demo ssh/sshd_config
hex=$(awk -F '\t' '$2 == "put" { print $4 }' "$last" | tail -n 1)
printf '\n'
side tree --dir demo
printf '\n'
side diff --dir demo 0 "$size"
printf '\n'
side blob --dir demo "$hex"
printf '\n'

mkdir -p child/modules
printf "root = '/opt/petsc'\n" >child/modules/petsc.lua
verifier=$("$demo_key" child.pem example.org/stack)
side init --dir child
printf '\n'
side sync --dir child --meta actor=stack
printf '\n'
side snapshot --dir child --key child.pem --origin example.org/stack
printf '\n'
printf '$ cargo run -q -p rfc9162-docs-examples --bin show_checkpoint -- child/.sidecar/pack > child-note.txt\n'
"$show" child/.sidecar/pack >child-note.txt
mkdir -p site
side init --dir site
printf '\n'
side include --dir site stacks/2026 --note child-note.txt --trusted "$verifier" --meta actor=site
