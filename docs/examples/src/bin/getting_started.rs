//! The program the getting-started page copies into a new crate.

fn main() {
    // ANCHOR: records
    let records = ["alpha", "beta", "gamma"];
    let leaves: Vec<[u8; 32]> = records
        .iter()
        .map(|record| rfc9162::leaf_hash(record.as_bytes()))
        .collect();
    let head = rfc9162::Head::over(&leaves);
    println!("size {}", head.size);
    println!("root {}", head.root);
    // ANCHOR_END: records

    // ANCHOR: inclusion
    for (index, record) in records.iter().enumerate() {
        let path = rfc9162::inclusion_proof(&leaves, index).expect("index is in range");
        let ok = rfc9162::verify_inclusion(&leaves[index], index, head.size, &path, &head.root);
        println!("inclusion {index} {record} steps {} ok {ok}", path.len());
    }
    // ANCHOR_END: inclusion

    // ANCHOR: consistency
    let grown: Vec<[u8; 32]> = ["alpha", "beta", "gamma", "delta"]
        .iter()
        .map(|record| rfc9162::leaf_hash(record.as_bytes()))
        .collect();
    let later = rfc9162::Head::over(&grown);
    let proof = rfc9162::consistency_proof(&grown, head.size).expect("3 is a prefix of 4");
    let ok = rfc9162::verify_consistency(head.size, &head.root, later.size, &later.root, &proof);
    println!("later size {} root {}", later.size, later.root);
    println!(
        "consistency {} -> {} steps {} ok {ok}",
        head.size,
        later.size,
        proof.len()
    );
    // ANCHOR_END: consistency

    // ANCHOR: material
    let material = String::from_utf8(head.material("example.org/log")).expect("utf-8");
    println!("material {material}");
    // ANCHOR_END: material
}
