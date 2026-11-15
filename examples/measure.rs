//! Measures the main calls on this machine: the time of each, its steps as the work limit counts
//! them, and on Linux the memory the process held before the call and what the call added at its
//! peak. `cargo run --release --example measure` runs every case, each in a process of its own;
//! `-- <case>` runs one.
#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a measuring program: it prints its table and stops at a fault"
)]

use reed_solomon_core::{Code, Correction, DataDigest, Field, Pile, SetFacts};
use std::time::Instant;

const MIB: usize = 1 << 20;

/// What a case measured: the bytes of data it went through, the steps the limit counts, if the
/// call counts them, the seconds the files took to read, if there were files, and the seconds of
/// the call itself.
struct Measured {
    bytes: usize,
    steps: Option<u128>,
    read: Option<f64>,
    call: f64,
    memory: Memory,
}

/// The resident memory of the process before the call and at its peak during it, in MiB.
#[derive(Default)]
struct Memory {
    before: Option<u64>,
    peak: Option<u64>,
}

/// A line of /proc/self/status, in MiB.
fn status_mib(key: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with(key))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / 1024)
}

/// Sets the peak of the resident memory to what is resident now, and says what that is.
fn from_now() -> Option<u64> {
    std::fs::write("/proc/self/clear_refs", "5").ok()?;
    status_mib("VmRSS:")
}

/// Bytes that look random, the same on every run.
fn bytes(seed: u64, count: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    (0..count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// `Code::encode` of `parity` shards beyond `k` data shards of `length` bytes.
fn encode(field: Field, k: usize, parity: usize, length: usize) -> Measured {
    let code = Code::new(field, k).unwrap();
    let data: Vec<Vec<u8>> = (0..k).map(|i| bytes(i as u64 + 1, length)).collect();
    let slices: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let targets: Vec<u16> = (k..k + parity).map(|i| i as u16).collect();
    let before = from_now();
    let started = Instant::now();
    let made = code.encode(&slices, &targets).unwrap();
    let call = started.elapsed().as_secs_f64();
    let peak = status_mib("VmHWM:");
    assert_eq!(made.len(), parity);
    Measured {
        bytes: k * length,
        steps: None,
        read: None,
        call,
        memory: Memory { before, peak },
    }
}

/// `Code::correct` of a set with two wrong symbols in one column of every hundred.
fn correct(field: Field, k: usize, parity: usize, length: usize) -> Measured {
    let code = Code::new(field, k).unwrap();
    let mut set: Vec<Vec<u8>> = {
        let data: Vec<Vec<u8>> = (0..k).map(|i| bytes(i as u64 + 1, length)).collect();
        let slices: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let all: Vec<u16> = (0..(k + parity) as u16).collect();
        let made = code.encode(&slices, &all).unwrap();
        made.iter().map(|shard| shard.to_vec()).collect()
    };
    let width = field.symbol_bytes();
    for column in (0..length / width).step_by(100) {
        set[column % (k + parity)][column * width] ^= 1;
        set[(column + 1) % (k + parity)][column * width] ^= 1;
    }
    let mut given: Vec<(u16, &mut [u8])> =
        (0..).zip(set.iter_mut().map(Vec::as_mut_slice)).collect();
    let before = from_now();
    let started = Instant::now();
    let answer = code.correct(&mut given).unwrap();
    let call = started.elapsed().as_secs_f64();
    let peak = status_mib("VmHWM:");
    assert!(matches!(answer, Correction::Corrected(_)));
    Measured {
        bytes: k * length,
        steps: None,
        read: None,
        call,
        memory: Memory { before, peak },
    }
}

/// The files of a set of `length` bytes of data in `k` shards and `parity` more, blocks of 64 KiB,
/// without a digest, so that the time of a rebuild is that of the work the steps count.
fn files(field: Field, k: usize, parity: usize, length: usize) -> Vec<Vec<u8>> {
    let data = bytes(7, length);
    let facts = SetFacts::for_data(field, b"measure", k, &data, 65_536, DataDigest::None).unwrap();
    let all: Vec<u16> = (0..(k + parity) as u16).collect();
    let made = facts.files(&data, &all).unwrap();
    made.iter().map(|file| file.to_vec()).collect()
}

/// `Pile::new`, then `FileSet::rebuild` or, with `repair`, `FileSet::repairs` of the first `lost`
/// data files, from the parity files and the rest.
fn set(
    field: Field,
    k: usize,
    parity: usize,
    length: usize,
    lost: usize,
    repair: bool,
) -> Measured {
    let files = files(field, k, parity, length);
    let given: Vec<&[u8]> = files[lost..].iter().map(Vec::as_slice).collect();
    let indexes: Vec<u16> = (0..lost as u16).collect();
    let started = Instant::now();
    let pile = Pile::new(&given).unwrap();
    let read = started.elapsed().as_secs_f64();
    let set = &pile.sets()[0];
    let before = from_now();
    let started = Instant::now();
    let steps = if repair {
        let made = set.repairs(&indexes, None).unwrap();
        let call = started.elapsed();
        assert_eq!(made[0].as_slice(), files[0].as_slice());
        (set.repairs_work(&indexes).unwrap(), call)
    } else {
        let rebuilt = set.rebuild(None).unwrap();
        let call = started.elapsed();
        assert_eq!(rebuilt.data.len(), length);
        (rebuilt.steps, call)
    };
    let peak = status_mib("VmHWM:");
    Measured {
        bytes: length,
        steps: Some(steps.0),
        read: Some(read),
        call: steps.1.as_secs_f64(),
        memory: Memory { before, peak },
    }
}

/// A case: its name, what it does, and the call.
type Case = (&'static str, &'static str, fn() -> Measured);

/// The cases.
const CASES: [Case; 8] = [
    (
        "encode-8",
        "GF(2^8), k = 10, 4 parity shards of 8 MiB",
        || encode(Field::Gf256, 10, 4, 8 * MIB),
    ),
    (
        "encode-16",
        "GF(2^16), k = 100, 20 parity shards of 256 KiB",
        || encode(Field::Gf65536, 100, 20, MIB / 4),
    ),
    (
        "correct-8",
        "GF(2^8), k = 10, 4 spare shards of 8 MiB",
        || correct(Field::Gf256, 10, 4, 8 * MIB),
    ),
    (
        "correct-16",
        "GF(2^16), k = 100, 20 spare shards of 256 KiB",
        || correct(Field::Gf65536, 100, 20, MIB / 4),
    ),
    (
        "rebuild-8",
        "GF(2^8), 64 MiB, k = 10 + 4, 4 data files lost",
        || set(Field::Gf256, 10, 4, 64 * MIB, 4, false),
    ),
    (
        "rebuild-16",
        "GF(2^16), 32 MiB, k = 200 + 50, 50 data files lost",
        || set(Field::Gf65536, 200, 50, 32 * MIB, 50, false),
    ),
    (
        "repairs-8",
        "GF(2^8), 64 MiB, k = 10 + 4, the 4 lost data files made",
        || set(Field::Gf256, 10, 4, 64 * MIB, 4, true),
    ),
    (
        "repairs-16",
        "GF(2^16), 32 MiB, k = 200 + 50, the 50 lost data files made",
        || set(Field::Gf65536, 200, 50, 32 * MIB, 50, true),
    ),
];

/// `value` with `digits` decimals, or nothing.
fn shown(value: Option<f64>, digits: usize) -> String {
    value.map_or_else(String::new, |value| format!("{value:.digits$}"))
}

fn main() {
    let Some(name) = std::env::args().nth(1) else {
        println!(
            "| Case | What | MiB | Read, s | Call, s | MiB/s | Steps | ns/step | Held, MiB | Added, MiB |"
        );
        println!("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
        for (name, _, _) in CASES {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg(name)
                .status()
                .unwrap();
            assert!(status.success(), "{name}");
        }
        return;
    };
    let (name, what, run) = CASES
        .iter()
        .find(|(case, _, _)| *case == name)
        .expect("a case");
    let measured = run();
    let mib = measured.bytes as f64 / MIB as f64;
    let steps = measured
        .steps
        .map_or_else(String::new, |steps| steps.to_string());
    let per_step = measured
        .steps
        .map(|steps| measured.call * 1e9 / steps as f64);
    let Memory { before, peak } = measured.memory;
    let added = before
        .zip(peak)
        .map(|(before, peak)| peak.saturating_sub(before) as f64);
    println!(
        "| {name} | {what} | {mib:.0} | {} | {:.2} | {:.0} | {steps} | {} | {} | {} |",
        shown(measured.read, 2),
        measured.call,
        mib / measured.call,
        shown(per_step, 2),
        shown(before.map(|before| before as f64), 0),
        shown(added, 0),
    );
}
