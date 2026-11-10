# rfc9162

Rust crates for an append-only Merkle log hashed as [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162) (Certificate Transparency Version 2.0). A log built with these crates gives each reader an inclusion proof for one entry and a consistency proof between two signed tree heads.

## Why

A signature on a tree head binds the signer to one size and one root. An inclusion proof shows that one entry is in that tree. A consistency proof shows that a later head extends an earlier head by append. A reader who stores a head can check both, entry by entry and head by head.

The hash tree, the stored tiles and the indexed log are separate crates. A program can hash a tree in memory, store the same tree where a tlog-tiles client can read it, or keep an index whose own manifests are entries in the log.

<div class="cards">
  <a class="card" href="getting-started.html">
    <span class="kicker">Start</span>
    <strong>Getting started</strong>
    <p>Hash three records, prove inclusion, and prove that a fourth record extends the log.</p>
  </a>
  <a class="card" href="tutorials/build-a-log.html">
    <span class="kicker">Tutorial</span>
    <strong>Build a log</strong>
    <p>Write the records to a store, sign a checkpoint, and check the proofs a reader receives.</p>
  </a>
  <a class="card" href="howto/index.html">
    <span class="kicker">How-to</span>
    <strong>Tasks by crate</strong>
    <p>Recipes for the hash tree, the tile store and the indexed log, including the sidecar.</p>
  </a>
  <a class="card" href="topics/index.html">
    <span class="kicker">Topics</span>
    <strong>Concepts</strong>
    <p>The Merkle tree, inclusion and consistency proofs, tiles, runs and signed checkpoints.</p>
  </a>
  <a class="card" href="reference/index.html">
    <span class="kicker">Reference</span>
    <strong>API reference</strong>
    <p>rustdoc for <code>rfc9162</code>, <code>rfc9162-tiles</code> and <code>rfc9162-log</code>, plus the sidecar command.</p>
  </a>
  <a class="card" href="install/index.html">
    <span class="kicker">Install</span>
    <strong>Installation</strong>
    <p>Rustup on Linux, macOS and Windows, then the three crates at one version.</p>
  </a>
</div>

The crates are version 0.10.0. [Installation](install/index.md) has the commands. The [changelog](changelog.md) records earlier releases.

<p class="copyright">Copyright (c) rfc9162 developers</p>
