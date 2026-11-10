//! Writes a fixed Ed25519 PKCS#8 PEM and prints the verifier line for an origin.
//!
//! The key is the 32-byte seed `7` repeated. It exists so the documentation
//! transcript is stable. It has no authority over any real log.

use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use pkcs8::LineEnding;
use rfc9162_tiles::Signer;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("pem path");
    let origin = args.next().expect("origin");
    let key = SigningKey::from_bytes(&[7u8; 32]);
    let pem = key.to_pkcs8_pem(LineEnding::LF).expect("pem");
    std::fs::write(&path, pem.as_bytes()).expect("write pem");
    let signer = Signer::new(&origin, key).expect("signer");
    println!("{}", signer.verifier().to_text());
}
