//! The reader against a reader written here from docs/TECHNICAL.md alone, on files mutated with
//! headers, copies and trailers whose checksums hold.
#![allow(clippy::all)]

#[path = "../raw.rs"]
mod raw;

use raw::{Raw, crc32c, trailer};
use reed_solomon_core::{Block, Condition, FileError, Length, ShardFile};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Facts {
    bytes: Vec<u8>,
    file_len: u64,
    shard: u64,
    blocks: u64,
    block_len: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum Found {
    Sound(Facts),
    Unsupported(usize),
    Version(u8),
    Damaged,
    Absent,
}

fn be(b: &[u8]) -> u64 {
    b.iter().fold(0, |a, &x| (a << 8) | u64::from(x))
}

/// The header at the start of `b`, by the table of docs/TECHNICAL.md.
fn start(b: &[u8]) -> Found {
    if b.len() < 4 || &b[..4] != b"SHRD" {
        return Found::Absent;
    }
    if b.len() < 5 {
        return Found::Damaged;
    }
    if b[4] != 1 {
        return Found::Version(b[4]);
    }
    if b.len() < 8 {
        return Found::Damaged;
    }
    let (field, flags, idl) = (b[5], b[6], b[7] as usize);
    if idl == 0 || idl > 32 {
        return Found::Damaged;
    }
    let h = 32 + idl + if flags & 1 == 1 { 32 } else { 0 };
    if b.len() < h {
        return Found::Damaged;
    }
    if crc32c(&[&b[..h - 4]]) as u64 != be(&b[h - 4..h]) {
        return Found::Damaged;
    }
    if !(field == 1 || field == 2) || flags & !1 != 0 {
        return Found::Unsupported(h);
    }
    let at = 8 + idl;
    let k = be(&b[at..at + 4]);
    let index = be(&b[at + 4..at + 8]);
    let data_len = be(&b[at + 8..at + 16]);
    let block_len = be(&b[at + 16..at + 20]);
    let (order, width) = if field == 1 { (256u64, 1u64) } else { (65_536, 2) };
    if k == 0 || k > order || index >= order || block_len == 0 || block_len % width != 0 {
        return Found::Damaged;
    }
    if flags & 1 == 1 && data_len >= 1 << 61 {
        return Found::Damaged;
    }
    let shard = (data_len as u128).div_ceil(k as u128).next_multiple_of(width as u128);
    let blocks = shard.div_ceil(block_len as u128);
    if blocks > 1 << 32 {
        return Found::Damaged;
    }
    let file = 2 * h as u128 + shard + 4 * blocks + 12;
    if file > u64::MAX as u128 {
        return Found::Damaged;
    }
    Found::Sound(Facts {
        bytes: b[..h].to_vec(),
        file_len: file as u64,
        shard: shard as u64,
        blocks: blocks as u64,
        block_len,
    })
}

fn end(b: &[u8]) -> Found {
    if b.len() < 12 {
        return Found::Absent;
    }
    let t = &b[b.len() - 12..];
    let valid = crc32c(&[&t[..8]]) as u64 == be(&t[8..]);
    let named = be(&t[4..8]) as usize;
    if &t[..4] != b"SHRT" {
        return Found::Absent;
    }
    if !valid || !(33..=96).contains(&named) {
        return Found::Damaged;
    }
    let before = b.len() - 12;
    if before < named {
        return Found::Damaged;
    }
    match start(&b[before - named..before]) {
        Found::Sound(f) if f.bytes.len() == named && f.file_len == b.len() as u64 => Found::Sound(f),
        Found::Unsupported(l) if l == named => Found::Unsupported(l),
        Found::Version(v) => Found::Version(v),
        _ => Found::Damaged,
    }
}

#[derive(Debug, PartialEq)]
enum Answer {
    Read { header: Vec<u8>, lead: Condition, tail: Condition, trailer: Condition, length: Length, intact: Vec<u32> },
    Refused(FileError),
}

fn oracle(b: &[u8]) -> Answer {
    let (lead, last) = (start(b), end(b));
    let h = match (&lead, &last) {
        (Found::Sound(x), Found::Sound(y)) if x != y => return Answer::Refused(FileError::Conflict),
        (Found::Unsupported(_), Found::Sound(_)) => return Answer::Refused(FileError::Conflict),
        (Found::Sound(x), _) | (_, Found::Sound(x)) => x.clone(),
        (Found::Unsupported(_), _) | (_, Found::Unsupported(_)) => return Answer::Refused(FileError::Unsupported),
        (Found::Version(v), _) | (_, Found::Version(v)) => return Answer::Refused(FileError::Version(*v)),
        (Found::Damaged, _) | (_, Found::Damaged) => return Answer::Refused(FileError::Damaged),
        (Found::Absent, Found::Absent) => return Answer::Refused(FileError::Unrecognized),
    };
    let hl = h.bytes.len() as u64;
    let given = b.len() as u64;
    let length = if given == h.file_len {
        Length::Exact
    } else if given > h.file_len {
        Length::Long(given - h.file_len)
    } else {
        Length::Short(h.file_len - given)
    };
    let trailer_at = h.file_len.saturating_sub(12);
    let tail_at = trailer_at.saturating_sub(hl);
    let there: &[u8] = if tail_at < given {
        let s = tail_at as usize;
        &b[s..(s + 96).min(b.len())]
    } else {
        &[]
    };
    let tail = match start(there) {
        Found::Sound(c) if c == h => Condition::Intact,
        Found::Sound(_) | Found::Unsupported(_) => return Answer::Refused(FileError::Conflict),
        _ if (there.len() as u64) < hl => Condition::Missing,
        _ => Condition::Damaged,
    };
    let trailer_c = if trailer_at + 12 > given {
        Condition::Missing
    } else if b[trailer_at as usize..trailer_at as usize + 12] == trailer(hl as u32)[..] {
        Condition::Intact
    } else {
        Condition::Damaged
    };
    let lead_c = if matches!(lead, Found::Sound(_)) { Condition::Intact } else { Condition::Damaged };
    // Blocks: the place of each, the header's checksum, the number.
    let own = &h.bytes[h.bytes.len() - 4..];
    let mut intact = Vec::new();
    for n in 0..h.blocks.min(1 << 20) {
        let s = hl + n * (h.block_len + 4);
        let len = (h.shard - n * h.block_len).min(h.block_len);
        if s + len + 4 > given {
            break;
        }
        let (s, len) = (s as usize, len as usize);
        let block = &b[s..s + len];
        if crc32c(&[own, &(n as u32).to_be_bytes(), block]) as u64 == be(&b[s + len..s + len + 4]) {
            intact.push(n as u32);
        }
    }
    Answer::Read { header: h.bytes, lead: lead_c, tail, trailer: trailer_c, length, intact }
}

fn crate_answer(b: &[u8]) -> Answer {
    match ShardFile::read(b) {
        Err(e) => Answer::Refused(e),
        Ok(f) => {
            let fr = f.frame();
            let survey = f.survey();
            let mut intact = Vec::new();
            let present = (survey.intact + survey.damaged).min(1 << 20) as u32;
            for n in 0..present {
                if let Some(Block::Intact(_)) = f.block(n) {
                    intact.push(n);
                }
            }
            Answer::Read {
                header: f.header().as_bytes().to_vec(),
                lead: fr.lead,
                tail: fr.tail,
                trailer: fr.trailer,
                length: fr.length,
                intact,
            }
        }
    }
}

fn random_raw(rng: &mut Rng) -> Raw {
    let field = [1u8, 2, 1, 2, 0, 3][rng.below(6) as usize];
    let width = if field == 2 { 2 } else { 1 };
    let digest = rng.below(2) == 0;
    Raw {
        version: if rng.below(20) == 0 { 2 } else { 1 },
        field,
        flags: if digest { 1 } else { 0 } | if rng.below(30) == 0 { 4 } else { 0 },
        id: (0..[1, 1, 2, 32, 5][rng.below(5) as usize]).map(|_| rng.below(3) as u8).collect(),
        threshold: [1, 2, 3, 0, 257][rng.below(5) as usize].min(1 + rng.below(4) as u32),
        index: rng.below(6) as u32,
        data_len: [0, 1, 7, 20, 33][rng.below(5) as usize],
        block_len: ([1, 2, 4, 8][rng.below(4) as usize]) * width,
        digest: digest.then(|| [rng.below(256) as u8; 32]),
    }
}

fn main() {
    let rounds: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(100_000);
    let seed: u64 = std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(7);
    let mut rng = Rng(seed);
    let mut kinds = std::collections::BTreeMap::<String, u64>::new();
    for round in 0..rounds {
        let base = random_raw(&mut rng);
        let shard_len = {
            let k = base.threshold.max(1) as u64;
            let w = if base.field == 2 { 2 } else { 1 };
            base.data_len.div_ceil(k).next_multiple_of(w)
        };
        let shard: Vec<u8> = (0..shard_len).map(|_| rng.next() as u8).collect();
        let mut f = base.file(&shard);
        for _ in 0..=rng.below(3) {
            match rng.below(9) {
                0 => {
                    // Another header at the start, its checksum holding.
                    let other = random_raw(&mut rng).header();
                    let n = other.len().min(f.len());
                    f[..n].copy_from_slice(&other[..n]);
                }
                1 => {
                    // Another header in the place of the copy at the end.
                    let other = random_raw(&mut rng).header();
                    let hl = base.header().len();
                    if f.len() >= hl + 12 {
                        let at = f.len() - 12 - hl;
                        let n = other.len().min(f.len() - at);
                        f[at..at + n].copy_from_slice(&other[..n]);
                    }
                }
                2 => {
                    // A trailer naming any length.
                    let l = f.len();
                    if l >= 12 {
                        let named = [33u32, 34, 65, 96, 97, 32, rng.below(100) as u32][rng.below(7) as usize];
                        f[l - 12..].copy_from_slice(&trailer(named));
                    }
                }
                3 => {
                    let l = f.len() as u64;
                    f.truncate(rng.below(l + 1) as usize);
                }
                4 => {
                    for _ in 0..rng.below(40) {
                        f.push(rng.next() as u8);
                    }
                }
                5 => {
                    // A header and trailer appended that describe the longer file, or not.
                    let other = random_raw(&mut rng);
                    let h = other.header();
                    f.extend(&h);
                    f.extend(trailer(h.len() as u32));
                }
                6 => {
                    if !f.is_empty() {
                        let p = rng.below(f.len() as u64) as usize;
                        f[p] ^= 1 << rng.below(8);
                    }
                }
                7 => {
                    // The file of another header with the same blocks.
                    let other = random_raw(&mut rng);
                    f = other.file(&shard);
                }
                _ => {
                    // Cut from the front.
                    let l = f.len() as u64;
                    let c = rng.below(l.min(40) + 1) as usize;
                    f.drain(..c);
                }
            }
        }
        let (mine, theirs) = (oracle(&f), crate_answer(&f));
        let kind = match &theirs {
            Answer::Refused(e) => format!("{e:?}"),
            Answer::Read { lead, tail, trailer, length, .. } => {
                format!("read {lead:?}/{tail:?}/{trailer:?}/{}", match length { Length::Exact => "exact", Length::Long(_) => "long", Length::Short(_) => "short" })
            }
        };
        *kinds.entry(kind).or_default() += 1;
        if mine != theirs {
            println!("round {round}: differs\n file {:02x?}\n oracle {mine:?}\n crate  {theirs:?}", f);
            std::process::exit(1);
        }
    }
    for (k, v) in kinds {
        println!("{v:>8} {k}");
    }
}
