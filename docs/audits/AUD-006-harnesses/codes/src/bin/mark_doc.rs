//! A marked symbol holding bytes outside the field: the `Mark` rustdoc says "whatever its bytes
//! hold, it is a gap at this place"; both plans refuse it.

use reed_solomon_core::{Code, CyclicCode, CyclicWords, Field, Mark};

fn main() {
    // Code::plan, GF(2^11), k = 2, four shards, shard 3 unreadable and read as 0xFFFF.
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let data: [&[u8]; 2] = [&[0x01, 0x23], &[0x04, 0x56]];
    let set = code.encode(&data, &[0, 1, 2, 3]).unwrap();
    let gap = [0xFF, 0xFF];
    let shards = [(0, &set[0][..]), (1, &set[1][..]), (2, &set[2][..]), (3, &gap[..])];
    let mark = [Mark { shard: 3, symbol: 0 }];
    println!("Code::plan, marked symbol 0xFFFF: {:?}", code.plan(&shards, &mark).map(|_| "Ok"));
    let zero = [0x00, 0x00];
    let shards = [(0, &set[0][..]), (1, &set[1][..]), (2, &set[2][..]), (3, &zero[..])];
    println!("Code::plan, marked symbol 0x0000: {:?}", code.plan(&shards, &mark).map(|_| "Ok"));
    // CyclicCode::plan with the same mark, and with the shard named lost; CyclicWords::correct_word.
    let cyclic = CyclicCode::new(Field::Gf2048, 3, 2, 1, 1).unwrap();
    let d: [[u8; 2]; 3] = [[0, 1], [0, 2], [0, 3]];
    let views: Vec<&[u8]> = d.iter().map(|w| w.as_slice()).collect();
    let parity = cyclic.encode(&views).unwrap();
    let word: Vec<Vec<u8>> = vec![d[0].to_vec(), vec![0xFF, 0xFF], d[2].to_vec(), parity[0].to_vec(), parity[1].to_vec()];
    let views: Vec<&[u8]> = word.iter().map(Vec::as_slice).collect();
    println!("CyclicCode::plan, marked symbol 0xFFFF: {:?}", cyclic.plan(&views, &[], &[Mark { shard: 1, symbol: 0 }]).map(|_| "Ok"));
    println!("CyclicCode::plan, the same shard named lost: {:?}", cyclic.plan(&views, &[1], &[]).map(|_| "Ok"));
    let mut w = [1u16, 0xFFFF, 3, u16::from_be_bytes([parity[0][0], parity[0][1]]), u16::from_be_bytes([parity[1][0], parity[1][1]])];
    let mut room = [0u16; 30];
    println!("CyclicWords::correct_word, erased 0xFFFF: {:?} -> {:x?}", cyclic.words().correct_word(&mut w, &[1], &mut room[..CyclicWords::room(2)]), w);
}
