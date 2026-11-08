#!/usr/bin/env python3
"""The one hand-written version, and the copies cog writes from it.

The version is written by hand in ``[workspace.package]`` in the workspace
Cargo.toml. Every other copy is a cogapp block listed in cogfiles.txt.

    cog -r @cogfiles.txt        rewrites those copies from this module
    cog --check @cogfiles.txt   fails when a copy has drifted
"""

from __future__ import annotations

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "Cargo.toml"


def version() -> str:
    """The workspace package version, the only hand-written copy."""
    with MANIFEST.open("rb") as handle:
        return tomllib.load(handle)["workspace"]["package"]["version"]


def pins() -> str:
    """``[workspace.dependencies]`` lines for this workspace's own crates.

    A published crate that depends on another by path also has to require
    the version that crate publishes, or the published package does not
    resolve. ``rfc9162-tiles`` depends on ``rfc9162`` this way and
    ``rfc9162-log`` on both; ``rfc9162-log`` is required by nothing in the
    workspace, so it has no pin of its own.
    """
    current = version()
    return (
        f'rfc9162 = {{ path = "crates/rfc9162", version = "{current}" }}\n'
        f'rfc9162-tiles = {{ path = "crates/rfc9162-tiles", version = "{current}" }}\n'
    )


def main() -> None:
    print(version())


if __name__ == "__main__":
    main()
