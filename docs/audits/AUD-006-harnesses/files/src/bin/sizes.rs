//! What a pile holds per file, against the bytes of the file.
#![allow(clippy::all)]

use reed_solomon_core::{FileError, FileSet, Header, Member, Pile, SetFacts, ShardFile, Field};
use std::time::Instant;

fn rss_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse().ok())
        .unwrap_or(0)
}

fn main() {
    println!(
        "size_of ShardFile {} Result<ShardFile,FileError> {} Member {} FileSet {} Header {} SetFacts {}",
        size_of::<ShardFile<'static>>(),
        size_of::<Result<ShardFile<'static>, FileError>>(),
        size_of::<Member>(),
        size_of::<FileSet<'static>>(),
        size_of::<Header>(),
        size_of::<SetFacts>()
    );
    let count: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(200_000);
    // Header-only files: each its own shard of one of several sets of GF(2^16), threshold 2.
    let mut headers = Vec::new();
    for at in 0..count {
        let id = [(at / 65_536) as u8 + 1];
        let facts = SetFacts::new(Field::Gf65536, &id, 2, 1 << 20, 2, None).unwrap();
        headers.push(Header::new(facts, (at % 65_536) as u16).unwrap());
    }
    let files: Vec<&[u8]> = headers.iter().map(Header::as_bytes).collect();
    let bytes: usize = files.iter().map(|f| f.len()).sum();
    let before = rss_kib();
    let t = Instant::now();
    let pile = Pile::new(&files).unwrap();
    let took = t.elapsed();
    let after = rss_kib();
    println!(
        "{count} files, {bytes} bytes of files: pile {took:?}, {} sets, RSS +{} KiB ({:.0} bytes per file, {:.1}x the file bytes)",
        pile.sets().len(),
        after - before,
        (after - before) as f64 * 1024.0 / count as f64,
        (after - before) as f64 * 1024.0 / bytes as f64
    );
    let t = Instant::now();
    for set in pile.sets() {
        let _ = set.vote_work();
        let _ = set.rebuild_work();
    }
    println!("vote_work + rebuild_work over all sets {:?}", t.elapsed());
}
