//! Facts the topic pages state about tree shape, proofs and tile layout.

use rfc9162_tiles::Layout;

fn leaves(bytes: &[&[u8]]) -> Vec<[u8; 32]> {
    bytes
        .iter()
        .map(|record| rfc9162::leaf_hash(record))
        .collect()
}

#[test]
fn empty_and_single_leaf_roots() {
    assert_eq!(
        rfc9162::hex(&rfc9162::root(&[])),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        rfc9162::hex(&rfc9162::leaf_hash(b"")),
        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
    );
    let one = rfc9162::leaf_hash(b"");
    assert_eq!(rfc9162::root(&[one]), one);
}

#[test]
fn three_leaf_inclusion_and_consistency() {
    let three = leaves(&[b"alpha", b"beta", b"gamma"]);
    let path = rfc9162::inclusion_proof(&three, 1).unwrap();
    assert_eq!(path, vec![three[0], three[2]]);

    let proof = rfc9162::consistency_proof(&three, 2).unwrap();
    assert_eq!(proof, vec![three[2]]);

    let four = leaves(&[b"alpha", b"beta", b"gamma", b"delta"]);
    let extended = rfc9162::consistency_proof(&four, 3).unwrap();
    assert_eq!(extended, vec![four[2], four[3], rfc9162::root(&four[..2])]);
}

#[test]
fn seven_leaf_paths() {
    let leaves: Vec<[u8; 32]> = (0..7).map(|i| rfc9162::leaf_hash(&[i])).collect();
    let path = rfc9162::inclusion_proof(&leaves, 1).unwrap();
    assert_eq!(
        path,
        vec![
            leaves[0],
            rfc9162::root(&leaves[2..4]),
            rfc9162::root(&leaves[4..7]),
        ]
    );

    let from_four = rfc9162::consistency_proof(&leaves, 4).unwrap();
    assert_eq!(from_four, vec![rfc9162::root(&leaves[4..7])]);

    let from_five = rfc9162::consistency_proof(&leaves, 5).unwrap();
    assert_eq!(
        from_five,
        vec![leaves[4], leaves[5], leaves[6], rfc9162::root(&leaves[..4]),]
    );
}

#[test]
fn seventy_thousand_entries_tile_widths() {
    let layout = Layout::new(70_000);
    assert_eq!(layout.full_tiles_at(0), 273);
    assert_eq!(layout.width_at(0), 112);
    assert_eq!(layout.full_tiles_at(1), 1);
    assert_eq!(layout.width_at(1), 17);
    assert_eq!(layout.full_tiles_at(2), 0);
    assert_eq!(layout.width_at(2), 1);
    assert_eq!(layout.levels(), 3);
}
