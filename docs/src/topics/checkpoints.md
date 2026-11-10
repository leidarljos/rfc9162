# Checkpoints and cosignatures

A signed tree head is the size and the root, under a name, with an Ed25519 signature. The flat crate's [`Head::material`](../reference/rfc9162.md) is the domain, the size and the hexadecimal root, separated by tabs. `rfc9162-tiles` publishes a [C2SP checkpoint](https://c2sp.org/tlog-checkpoint) inside a [signed note](https://c2sp.org/signed-note).

## The checkpoint body

Three lines, each ending in a newline:

```text
<origin>
<decimal size>
<standard base64 of the 32-byte root>
```

Further non-empty lines are extensions. The origin is a schema-less URL such as `example.org/log`, and it is the signing key's name. The size is a canonical decimal: `0` or a number without a leading zero. The [tutorial](../tutorials/build-a-log.md) prints a size-4 checkpoint whose base64 root decodes to `42fc54eeb6352f90cc81fdd5791292cca3974a168208b395b06a76240b24884d`.

## The note

A note is the checkpoint text, a blank line, and one signature line per signer:

```text
— <name> <base64(key id || signature)>
```

The dash is U+2014, an em dash, followed by a space. The key id is the first four bytes of `SHA-256(name || 0x0A || 0x01 || public key)` for a log signature. The type byte `0x01` is `ED25519_LOG`. The signature is Ed25519 over the checkpoint text, including its trailing newlines.

`Signer::verifier` prints the trust line a reader stores:

```text
<name>+<8 hex digits of the key id>+<base64(0x01 || public key)>
```

The same public key under two names has two key ids. In the documentation transcripts the seed is the byte `7` repeated, and the verifier lines are:

```text
example.org/log+5e2cef00+AepKbGPinFIKvvVQexMuxfmVR3auvr57kkIe6mkURtIs
example.org/stack+77af48a1+AepKbGPinFIKvvVQexMuxfmVR3auvr57kkIe6mkURtIs
```

`checkpoint::verify` checks lines whose name and key id match a trusted log key. A line from an unknown key is ignored. At least one trusted key has to verify. `Reader::open_checkpoint` does that check and also requires the checkpoint origin to equal the origin the caller asked for. The reader then serves the tree at the checkpoint's size and root, which is how a reader opens a log from the published head.

`Writer::checkpoint` holds `.state/publish.lock` while it writes. If a checkpoint for a larger size is already published, the new note is refused. The signer keeps that published head as the latest one it will sign.

## Cosignatures

A witness appends a cosignature/v1 line. The type byte is `0x04`. The key id is `SHA-256(name || 0x0A || 0x04 || public key)` truncated to four bytes. The signed message is the lines `cosignature/v1`, `time <unix seconds>`, and the checkpoint text. The payload after the key id is the 8-byte big-endian timestamp and the Ed25519 signature.

`Cosigner::cosign` adds the line. `verify_cosignatures` checks witness lines against trusted witness keys. The [tiles how-to](../howto/rfc9162-tiles.md) cosigns at timestamp 1700000000 and prints one verified cosignature.

## What the head covers

A checkpoint from `rfc9162-tiles` covers the entries and the hash tree at that size. Key sidecars are derived from the bundles. In `rfc9162-log`, run manifests are entries, so a checkpoint over that log covers the index those manifests name. A [sidecar](../howto/rfc9162-log.md) snapshot is this checkpoint after `seal`. A parent log can `include` a child's note; the parent's next checkpoint then covers the child's signed head as an entry.
