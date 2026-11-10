//! Checks around one inclusion proof and one consistency proof.

fn main() {
    // ANCHOR: inclusion
    let leaves: Vec<[u8; 32]> = ["alpha", "beta", "gamma"]
        .iter()
        .map(|record| rfc9162::leaf_hash(record.as_bytes()))
        .collect();
    let head = rfc9162::Head::over(&leaves);
    let path = rfc9162::inclusion_proof(&leaves, 1).expect("index 1");
    println!(
        "right leaf {}",
        rfc9162::verify_inclusion(&leaves[1], 1, head.size, &path, &head.root)
    );
    println!(
        "other leaf {}",
        rfc9162::verify_inclusion(&leaves[0], 1, head.size, &path, &head.root)
    );
    println!(
        "other index {}",
        rfc9162::verify_inclusion(&leaves[1], 0, head.size, &path, &head.root)
    );
    println!(
        "short path {}",
        rfc9162::verify_inclusion(
            &leaves[1],
            1,
            head.size,
            &path[..path.len() - 1],
            &head.root
        )
    );
    let other_root = "00".repeat(32);
    println!(
        "other root {}",
        rfc9162::verify_inclusion(&leaves[1], 1, head.size, &path, &other_root)
    );
    // ANCHOR_END: inclusion

    // ANCHOR: consistency
    let grown: Vec<[u8; 32]> = ["alpha", "beta", "gamma", "delta"]
        .iter()
        .map(|record| rfc9162::leaf_hash(record.as_bytes()))
        .collect();
    let later = rfc9162::Head::over(&grown);
    let proof = rfc9162::consistency_proof(&grown, head.size).expect("prefix");
    println!(
        "prefix {}",
        rfc9162::verify_consistency(head.size, &head.root, later.size, &later.root, &proof)
    );
    println!(
        "swapped sizes {}",
        rfc9162::verify_consistency(later.size, &later.root, head.size, &head.root, &proof)
    );
    // ANCHOR_END: consistency

    // ANCHOR: hex
    println!("from_hex {}", rfc9162::from_hex(&head.root).is_some());
    println!(
        "from_hex uppercase {}",
        rfc9162::from_hex(&head.root.to_ascii_uppercase()).is_some()
    );
    // ANCHOR_END: hex
}
