//! Public contract samples for two documented boundaries. No real backups are used.
use reed_solomon_core::{CyclicCode, Field, Plan, SetFacts};

fn views(shards: &[Vec<u8>]) -> Vec<&[u8]> {
    shards.iter().map(Vec::as_slice).collect()
}

fn main() {
    let rs = CyclicCode::new(Field::Gf256, 2, 2, 1, 1).unwrap();
    let mut shards = vec![vec![1, 2], vec![3, 4]];
    let parity = rs.encode(&views(&shards)).unwrap();
    shards.extend(parity.iter().map(|s| s.as_slice().to_owned()));
    assert!(rs.agrees(&views(&shards)).unwrap());
    shards[0][0] ^= 1;
    let Plan::Changes(changes) = rs.plan(&views(&shards), &[], &[]).unwrap() else {
        panic!("a single changed public symbol has a correction");
    };
    shards[1][1] ^= 1;
    let mut outputs: Vec<_> = shards.iter_mut().map(Vec::as_mut_slice).collect();
    rs.apply(&mut outputs, &changes).unwrap();
    assert!(!rs.agrees(&views(&shards)).unwrap());
    println!("After another edit, apply returned success and agrees returned false.");
    for length in [(1_u64 << 61) - 1, 1_u64 << 61] {
        let shape = SetFacts::new(Field::Gf256, b"test", 1, length, u32::MAX, Some([0; 32]));
        assert!(shape.is_ok());
        println!("Digest descriptor accepted for {length} bytes (metadata only).");
    }
}
