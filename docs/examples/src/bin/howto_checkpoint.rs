//! Sign a checkpoint, check it, and add a witness cosignature.

use ed25519_dalek::SigningKey;
use rfc9162_tiles::checkpoint::{verify, verify_cosignatures};
use rfc9162_tiles::index::no_keys;
use rfc9162_tiles::{Cosigner, MemoryStore, Signer, Writer};

fn main() {
    // ANCHOR: sign
    let signer =
        Signer::new("example.org/log", SigningKey::from_bytes(&[7u8; 32])).expect("signer");
    let mut writer = Writer::open(MemoryStore::new(), no_keys()).expect("writer");
    writer.append(b"alpha").expect("append");
    writer.append(b"beta").expect("append");
    let note = writer
        .checkpoint(&signer, "example.org/log")
        .expect("checkpoint");
    print!("{}", note.to_text());
    let trusted = [signer.verifier()];
    let verified = verify(&note, &trusted).expect("verify");
    println!("verified {}", verified[0].name());
    // ANCHOR_END: sign

    // ANCHOR: cosign
    let witness =
        Cosigner::new("example.org/witness", SigningKey::from_bytes(&[9u8; 32])).expect("witness");
    let mut note = note;
    witness.cosign(&mut note, 1_700_000_000);
    let lines = verify_cosignatures(&note, &[witness.verifier()]).expect("cosignatures");
    println!(
        "cosignatures {} timestamp {}",
        lines.len(),
        lines[0].timestamp
    );
    // ANCHOR_END: cosign

    // ANCHOR: pem
    println!("verifier {}", signer.verifier().to_text());
    // ANCHOR_END: pem
}
