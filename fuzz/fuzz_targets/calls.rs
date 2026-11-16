//! Every public call that takes numbers, lengths or bytes from its caller, given any of them: a
//! field of any degree and polynomial, a code of any threshold with its sources, the cyclic code in
//! memory the caller gives, the facts of a set and the header of any shard, with a file written
//! from them. Nothing panics, a refusal leaves the outputs as they were, a word put right is a word
//! of the code, and a file that is written reads back as the header that wrote it.
#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a fuzz target answers by stopping the run: its unwraps and assertions are its checks"
)]

use libfuzzer_sys::fuzz_target;
use reed_solomon_core::{COMPARED, Code, CyclicWords, Field, Header, SetFacts, ShardFile};

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
            let agrees = sources.agrees(target, &stripes, &out);
            // In a room of any length: a short one is refused, a long one answers as `agrees`.
            // Two bytes, so that rooms on both sides of `COMPARED` come.
            let mut room = vec![0xEE; (next() << 8 | next()) % 512];
            let answer = sources.agrees_in(target, &stripes, &out, &mut room);
            assert!(if room.len() < COMPARED {
                answer.is_err()
            } else {
                answer == agrees
            });
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

    // The cyclic code over memory the caller gives, with any shape, lengths, places and bytes.
    let mut generator = vec![0; next() % 12];
    let (data, parity) = (next() % 10, next() % 10);
    let (first, step) = ((next() << 8 | next()) as u32, (next() << 8 | next()) as u32);
    if let Ok(words) = CyclicWords::new(field, data, parity, first, step, &mut generator) {
        let shards: Vec<Vec<u8>> = (0..next() % 20).map(|_| list(&mut next, 16)).collect();
        let shards: Vec<&[u8]> = shards.iter().map(Vec::as_slice).collect();
        let mut outs: Vec<Vec<u8>> = (0..next() % 12).map(|_| vec![0xEE; next() % 17]).collect();
        let mut buffers: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        let mut remainder = vec![0; next() % 12];
        if words.encode(&shards, &mut buffers, &mut remainder).is_err() {
            assert!(outs.iter().flatten().all(|&byte| byte == 0xEE));
        }
        let mut room = vec![0xEE; next() % 64];
        if words.agrees(&shards, &mut room).is_ok() {
            assert!(room.iter().all(|&byte| byte == 0));
        }
        let mut word: Vec<u16> = (0..next() % 20)
            .map(|_| (next() << 8 | next()) as u16)
            .collect();
        let erased: Vec<usize> = (0..next() % 6).map(|_| next() % 24).collect();
        let mut room = vec![0xEEEE; next() % (CyclicWords::room(9) + 2)];
        let before = word.clone();
        match words.correct_word(&mut word, &erased, &mut room) {
            Ok(written) => {
                assert!(room.iter().all(|&element| element == 0));
                assert!(written.is_some() || word == before);
                // What it leaves is a word of the code: shards of one symbol each agree.
                let width = field.symbol_bytes();
                let one = |&symbol: &u16| symbol.to_be_bytes()[2 - width..].to_vec();
                let shards: Vec<Vec<u8>> = word.iter().map(one).collect();
                let shards: Vec<&[u8]> = shards.iter().map(Vec::as_slice).collect();
                let mut room = vec![0; parity * width];
                let agrees = words.agrees(&shards, &mut room);
                assert!(written.is_none() || agrees == Ok(true));
            }
            Err(_) => assert_eq!(word, before),
        }
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
