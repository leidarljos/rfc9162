# rfc9162

The Merkle tree of RFC 9162 (Certificate Transparency Version 2.0, doi:10.17487/RFC9162) as functions over 32-byte leaf hashes. Leaf `SHA-256(0x00 || entry)`, node `SHA-256(0x01 || left || right)`. The crate returns the root, inclusion proofs, consistency proofs, and the checks for both.

Callers compute each leaf from their own record bytes and sign [`Head::material`](https://docs.rs/rfc9162/latest/rfc9162/struct.Head.html#method.material) with a domain that names the log. The longer guide is the repository's [`docs/`](../../docs/src/howto/rfc9162.md): a getting-started program, the tree shape, and these functions exercised against proofs that fail.

```rust
use rfc9162::{inclusion_proof, leaf_hash, verify_inclusion, Head};

let leaves: Vec<[u8; 32]> = ["alpha", "beta", "gamma"]
    .iter()
    .map(|record| leaf_hash(record.as_bytes()))
    .collect();
let head = Head::over(&leaves);
let path = inclusion_proof(&leaves, 1).unwrap();
assert!(verify_inclusion(&leaves[1], 1, head.size, &path, &head.root));
```

The crate tests include the RFC 6962 vectors that RFC 9162 keeps. MIT.
