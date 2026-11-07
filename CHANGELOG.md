# Changelog

Versions follow semver at 0.x: a minor bump is a feature, a patch is a fix.
Both crates carry the one workspace version and are released together.

<!-- towncrier release notes start -->

## 0.6.1 (2026-11-05)

### Features

- `rfc9162`: the Merkle tree of RFC 9162 (doi:10.17487/RFC9162) as pure functions over 32-byte leaf hashes.
  Leaf `SHA-256(0x00 || entry)`, node `SHA-256(0x01 || left || right)`, the root over a leaf slice, inclusion and consistency proofs, the checks for both, and a `Head` whose `material(domain)` is what a log signs.
  The crate stores nothing and serialises nothing.
  This release was published from the deedar repository, whose deed log builds on it; the version number is deedar's at the time.
