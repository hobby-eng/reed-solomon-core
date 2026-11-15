//! Every public call that takes numbers, lengths or bytes from its caller, given any of them: a
//! field of any degree and polynomial, a code of any threshold with its sources, the facts of a
//! set and the header of any shard, with a file written from them. Nothing panics, and a file
//! that is written reads back as the header that wrote it.
#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a fuzz target answers by stopping the run: its unwraps and assertions are its checks"
)]

use libfuzzer_sys::fuzz_target;
use reed_solomon_core::{Code, Field, Header, SetFacts, ShardFile};

fuzz_target!(|bytes: &[u8]| {
    let mut input = bytes.iter().copied();
    let mut next = || usize::from(input.next().unwrap_or(0));
    let degree = next() as u8 % 20;
    let polynomial = (next() << 16 | next() << 8 | next()) as u32;
    let chosen = Field::binary(degree, Some(polynomial));
    let field = match next() % 5 {
        0 => Field::Gf256,
        1 => Field::Gf65536,
        2 => Field::Gf2048,
        _ => chosen.unwrap_or(Field::Gf256),
    };
    let width = field.symbol_bytes();
    let list = |next: &mut dyn FnMut() -> usize, most: usize| -> Vec<u8> {
        (0..next() % (most + 1)).map(|_| next() as u8).collect()
    };

    // A code of any threshold, its sources from any shard numbers and stretches of any length.
    if let Ok(code) = Code::new(field, next() % 12) {
        let numbers: Vec<u16> = (0..next() % 12)
            .map(|_| (next() << 8 | next()) as u16)
            .collect();
        let mut weights = vec![0; next() % 12];
        let stretches: Vec<Vec<u8>> = (0..next() % 12).map(|_| list(&mut next, 16)).collect();
        let stripes: Vec<&[u8]> = stretches.iter().map(Vec::as_slice).collect();
        let target = (next() << 8 | next()) as u16;
        if let Ok(sources) = code.sources(&numbers, &mut weights) {
            let mut out = vec![0; next() % 17];
            let _ = sources.build(target, &stripes, &mut out);
            let _ = sources.agrees(target, &stripes, &out);
        }
        let shards: Vec<(u16, &[u8])> = numbers
            .iter()
            .copied()
            .zip(stripes.iter().copied())
            .collect();
        let _ = code.rebuild(&shards, &[target]);
        let _ = code.redundancy(&shards);
        let _ = code.encode(&stripes, &[target]);
        let _ = code.correction_memory(next());
    }

    // The facts of a set with any numbers, and the file of any shard written from them.
    let id = list(&mut next, 40);
    let threshold = next() % 300;
    let data_len = (next() << 16 | next() << 8 | next()) as u64;
    let block_len = (next() << 8 | next()) as u32 * width as u32;
    let digest = (next() % 2 == 0).then(|| [next() as u8; 32]);
    let Ok(facts) = SetFacts::new(field, &id, threshold, data_len, block_len, digest) else {
        return;
    };
    let _ = (
        facts.blocks(),
        facts.file_len(),
        facts.shard_len(),
        facts.data_len(),
    );
    let data = list(&mut next, 64);
    let _ = facts.files(&data, &[0, 1]);
    let Ok(header) = Header::new(facts, (next() << 8 | next()) as u16) else {
        return;
    };
    let shard = list(&mut next, 255);
    let mut out = vec![0; next() % 512];
    if header.file_into(&shard, &mut out).is_ok() {
        let read = ShardFile::read(&out).unwrap();
        assert_eq!(read.header(), &header);
        assert_eq!(*header.file(&shard).unwrap(), *out);
    }
});
