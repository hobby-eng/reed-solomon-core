//! AUD-006 probes of the shard file and of sets of files, through the public API only.
#![allow(clippy::all)]

mod raw;

use raw::{Raw, crc32c, trailer};
use reed_solomon_core::{Field, FileError, Header, Pile, SetFacts, ShardFile};
use std::time::Instant;

fn read(name: &str, bytes: &[u8]) {
    let t = Instant::now();
    match ShardFile::read(bytes) {
        Ok(file) => {
            let s = file.survey();
            println!(
                "{name}: Ok blocks={} frame={:?} intact={} damaged={} missing={} ({:?})",
                file.header().set().blocks(),
                file.frame(),
                s.intact,
                s.damaged,
                s.missing,
                t.elapsed()
            );
        }
        Err(e) => println!("{name}: Err({e:?}) ({:?})", t.elapsed()),
    }
}

/// Every call on every set of a pile, timed.
fn sets(name: &str, files: &[&[u8]]) {
    let t = Instant::now();
    let pile = match Pile::new(files) {
        Ok(p) => p,
        Err(e) => {
            println!("{name}: pile Err({e:?})");
            return;
        }
    };
    println!("{name}: pile in {:?}, {} sets", t.elapsed(), pile.sets().len());
    for set in pile.sets() {
        let t = Instant::now();
        let c = set.coverage();
        let stretches: Vec<_> = set.stretches().map(|s| s.take(5).collect()).unwrap_or_default();
        println!("  coverage {c:?} stretches {stretches:?}");
        println!("  rebuild_work {:?}", set.rebuild_work());
        println!("  repair_work(0) {:?}", set.repair_work(0));
        println!("  repairs_work {:?}", set.repairs_work(&[0, 1, 65535]));
        println!("  vote_work {:?}", set.vote_work());
        println!("  vote {:?}", set.vote(Some(1 << 30)).map(|v| (v.outvoted().len(), v.undecided(), v.steps())));
        println!("  rebuild {:?}", set.rebuild(Some(1 << 30)).map(|r| (r.data.len(), r.digest, r.bare, r.steps)));
        println!("  repair(0) {:?}", set.repair(0, Some(1 << 30)).map(|f| f.len()));
        println!("  without(0) {:?}", set.without(&[0]).map(|s| s.coverage()));
        println!("  disputed {}", set.disputed_blocks(0).take(10).count());
        println!("  ({:?})", t.elapsed());
    }
}

fn main() {
    let which: Vec<String> = std::env::args().skip(1).collect();
    let run = |n: &str| which.is_empty() || which.iter().any(|w| w == n);

    if run("matrix") {
        // Section 12 matrix of the reader.
        read("empty", b"");
        read("mark", b"SHRD");
        read("mark+v2", b"SHRD\x02");
        read("mark+v1", b"SHRD\x01");
        let base = Raw::new(3, 4, 10, 4);
        let file = base.file(&[0xF4, 0, 0, 0]);
        read("whole", &file);
        read("header only", &base.header());
        read("header-1", &base.header()[..base.header().len() - 1]);
        for (n, r) in [
            ("k=0", Raw { threshold: 0, ..base.clone() }),
            ("k=257", Raw { threshold: 257, ..base.clone() }),
            ("k=u32max", Raw { threshold: u32::MAX, ..base.clone() }),
            ("index=256", Raw { index: 256, ..base.clone() }),
            ("index=u32max", Raw { index: u32::MAX, ..base.clone() }),
            ("block=0", Raw { block_len: 0, ..base.clone() }),
            ("id=0", Raw { id: vec![], ..base.clone() }),
            ("id=33", Raw { id: vec![7; 33], ..base.clone() }),
            ("field=0", Raw { field: 0, ..base.clone() }),
            ("field=3", Raw { field: 3, ..base.clone() }),
            ("flag=2", Raw { flags: 2, ..base.clone() }),
            ("flag=1 no digest bytes", Raw { flags: 1, ..base.clone() }),
            ("data=u64max k=1 bl=u32max", Raw { threshold: 1, data_len: u64::MAX, block_len: u32::MAX, ..base.clone() }),
            ("data=2^63 k=1 bl=2^31", Raw { threshold: 1, data_len: 1 << 63, block_len: 1 << 31, ..base.clone() }),
            ("data=2^61 digest", Raw { threshold: 1, data_len: 1 << 61, block_len: 1 << 30, flags: 1, digest: Some([0; 32]), ..base.clone() }),
            ("data=2^61-1 digest", Raw { threshold: 1, data_len: (1 << 61) - 1, block_len: 1 << 30, flags: 1, digest: Some([0; 32]), ..base.clone() }),
            ("data=2^32 k=1 bl=1", Raw { threshold: 1, data_len: 1 << 32, block_len: 1, ..base.clone() }),
            ("data=0", Raw { data_len: 0, ..base.clone() }),
            ("gf65536 k=65536 idx=65535", Raw { field: 2, threshold: 65536, index: 65535, block_len: 2, ..base.clone() }),
            ("gf65536 odd block", Raw { field: 2, block_len: 3, ..base.clone() }),
        ] {
            read(&format!("hdr {n}"), &r.header());
        }
        // Swapped header copies: start of shard 4, end of shard 5, same set.
        let five = Raw::new(3, 5, 10, 4).file(&[1, 2, 3, 4]);
        let mut swapped = file.clone();
        let h = base.header().len();
        let at = swapped.len() - 12 - h;
        swapped[at..].copy_from_slice(&five[at..]);
        read("swapped copies", &swapped);
        // Lead damaged, exact length; and with one trailing byte.
        let mut lead = file.clone();
        lead[9] ^= 1;
        read("lead damaged", &lead);
        let mut lead_long = lead.clone();
        lead_long.push(0);
        read("lead damaged + 1 byte", &lead_long);
        // Trailing bytes and trailing whole file.
        let mut long = file.clone();
        long.extend(&file);
        read("file+file", &long);
        // A sound header + trailer appended that describes the extended file exactly.
        let other = Raw { data_len: 10 + 0, ..Raw::new(3, 4, 10, 4) };
        let _ = other;
        // Trailer names 33 but CRC wrong.
        let mut tr = file.clone();
        let n = tr.len();
        tr[n - 1] ^= 1;
        read("trailer crc wrong", &tr);
        // Trailer names an impossible length (97) with a valid CRC.
        let mut t97 = file.clone();
        let n = t97.len();
        t97[n - 12..].copy_from_slice(&trailer(97));
        read("trailer names 97", &t97);
    }

    if run("big") {
        // A header claiming 2^32 blocks; the file holds the header and a few blocks.
        let r = Raw { threshold: 1, data_len: 1 << 32, block_len: 1, ..Raw::new(1, 0, 0, 1) };
        let mut f = r.header();
        for n in 0..3u32 {
            f.extend(r.block(n, &[7]));
        }
        read("2^32 blocks, 3 present", &f);
        sets("2^32 blocks one file", &[&f]);
        // k = 1 GF(2^16), huge data, huge blocks.
        let r = Raw { field: 2, threshold: 1, data_len: 1 << 63, block_len: 1 << 31, ..Raw::new(1, 0, 0, 2) };
        let h = r.header();
        read("2^63 data", &h);
        sets("2^63 data one header", &[&h]);
        // Two files of a set of k=1 with 2^63 data: one header-only each.
        let r2 = Raw { index: 1, ..r.clone() };
        let h2 = r2.header();
        sets("2^63 data two headers", &[&h, &h2]);
        // Empty data, k = 1: header + trailer only; digest of nothing absent.
        let e = Raw { threshold: 1, data_len: 0, ..Raw::new(1, 0, 0, 1) };
        let ef = e.file(&[]);
        read("empty data file", &ef);
        sets("empty data", &[&ef]);
        // Empty data with a wrong digest.
        let ed = Raw { flags: 1, digest: Some([9; 32]), ..e.clone() };
        let edf = ed.file(&[]);
        sets("empty data wrong digest", &[&edf]);
    }

    if run("fields") {
        for (name, field) in [
            ("binary(8, None)", Field::binary(8, None).unwrap()),
            ("binary(8, 0x11D)", Field::binary(8, Some(0x11D)).unwrap()),
            ("binary(8, 0x12B)", Field::binary(8, Some(0x12B)).unwrap()),
            ("binary(4, None)", Field::binary(4, None).unwrap()),
            ("binary(16, None)", Field::binary(16, None).unwrap()),
            ("binary(11, None)", Field::binary(11, None).unwrap()),
        ] {
            println!(
                "{name}: is Gf256 {} SetFacts::new {:?}",
                field == Field::Gf256,
                SetFacts::new(field, b"x", 2, 10, 4, None).map(|f| f.file_len())
            );
        }
    }

    if run("known") {
        let facts = SetFacts::new(Field::Gf256, &[1], 3, 6, 2, None).unwrap();
        let data = [0x01, 0x04, 0x00, 0x07, 0x09, 0x16];
        let mut files: Vec<Vec<u8>> = facts.files(&data, &[0, 1, 2, 3, 4]).unwrap().iter().map(|f| f.to_vec()).collect();
        files[0] = Header::new(facts, 0).unwrap().file(&[0x00, 0x06]).unwrap().to_vec();
        let given: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
        let pile = Pile::new(&given).unwrap();
        let set = &pile.sets()[0];
        let vote = set.vote(None).unwrap();
        println!("known vote: steps {} bound {:?} outvoted {:?}", vote.steps(), set.vote_work(), vote.outvoted());
    }

    if run("consistency") {
        // SetFacts::new vs. a header read back: same facts.
        let facts = SetFacts::new(Field::Gf256, b"x", 3, 26, 8, None).unwrap();
        let files = facts.files(b"twenty-six bytes of a text", &[0, 1, 2, 3, 4]).unwrap();
        let slices: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
        sets("five whole", &slices);
        let h = Header::new(facts, 4).unwrap();
        println!("crc of header via raw {:08x}", crc32c(&[&h.as_bytes()[..h.as_bytes().len() - 4]]));
        let _ = FileError::Memory;
    }
}
