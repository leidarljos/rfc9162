# Getting started

This program hashes three records, checks an inclusion proof for each, then checks that a fourth record is an append. It uses only `rfc9162`. Storage and signatures are the [tutorial](tutorials/build-a-log.md).

Install Rust from the [installation](install/index.md) page, then:

```text
cargo new ct-log
cd ct-log
cargo add rfc9162@0.10.0
```

Replace `src/main.rs` with the program below and run `cargo run`.

## The head

[`leaf_hash`](reference/rfc9162.md) hashes each record. [`Head::over`](reference/rfc9162.md) is the size and the hexadecimal root.

```rust
{{#rustdoc_include ../examples/src/bin/getting_started.rs:records}}
```

## Inclusion

[`inclusion_proof`](reference/rfc9162.md) returns the sibling hashes for one index, innermost first. [`verify_inclusion`](reference/rfc9162.md) folds that path back to the root. The [inclusion](topics/inclusion.md) page draws the path for `beta`.

```rust
{{#rustdoc_include ../examples/src/bin/getting_started.rs:inclusion}}
```

## Consistency

The second head is the same three records plus `delta`. [`consistency_proof`](reference/rfc9162.md) returns the nodes that recompute both roots. [`verify_consistency`](reference/rfc9162.md) accepts the pair when the later tree is the earlier tree with entries appended. The [consistency](topics/consistency.md) page lists the three nodes in this proof.

```rust
{{#rustdoc_include ../examples/src/bin/getting_started.rs:consistency}}
```

## Bytes a signature can cover

[`Head::material`](reference/rfc9162.md) is the domain, the size and the root, separated by tabs. The domain names the log. The tile crate signs a [C2SP checkpoint](topics/checkpoints.md) over the same size and root.

```rust
{{#rustdoc_include ../examples/src/bin/getting_started.rs:material}}
```

## Output

`cargo run -q` prints:

```text
{{#include output/getting_started.txt}}
```

The root at size 3 is `385da30f3917282c8939dff851957e519ab1846b1351a14c0adb3b11632742aa`. `alpha` and `beta` each need two siblings. `gamma` needs one. The consistency proof from size 3 to size 4 has three nodes, and the check returns true.

Next, [build the same log on disk](tutorials/build-a-log.md) and read the proofs back from the store.
