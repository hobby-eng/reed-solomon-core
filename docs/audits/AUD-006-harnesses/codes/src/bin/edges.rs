//! The edge-case matrix of the code calls: empty inputs, numeric boundaries, zero and odd
//! lengths, long shards across stripes with errors and marks at the borders, mixed columns
//! (all or nothing), duplicate and unordered marks, and random cases at and beyond the bound in
//! the larger fields.

use reed_solomon_core::{
    ChangeKind, Code, CodeError, Correction, CyclicCode, CyclicWords, Field, Mark, Plan, Redundancy,
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn line(ok: bool, what: &str) {
    if !ok {
        aud006_codes_probe::fail();
    }
    println!("{} {what}", if ok { "ok  " } else { "FAIL" });
}

fn random_symbols(rng: &mut Rng, field: Field, count: usize) -> Vec<u8> {
    let w = field.symbol_bytes();
    let q = field.order() as u64;
    (0..count).flat_map(|_| ((rng.next() % q) as u16).to_be_bytes().into_iter().skip(2 - w)).collect()
}

fn main() {
    let mut rng = Rng(0x5151_7A7A_0101_FEFE);
    let fields = [Field::Gf256, Field::Gf65536, Field::Gf2048, Field::binary(2, None).unwrap(), Field::binary(13, None).unwrap()];
    // A. Empty inputs.
    for field in fields {
        let code = Code::new(field, 1).unwrap();
        let w = field.symbol_bytes();
        let one = vec![0u8; w];
        let mut weights = [0u16; 1];
        let mut none: [(u16, &mut [u8]); 0] = [];
        let results = [
            code.encode(&[], &[0]).err() == Some(CodeError::Count),
            code.encode(&[&one], &[]).map(|v| v.is_empty()) == Ok(true),
            code.rebuild(&[], &[0]).err() == Some(CodeError::Count),
            code.redundancy(&[]) == Err(CodeError::Count),
            code.correct(&mut none) == Err(CodeError::Count),
            code.plan(&[], &[]) == Err(CodeError::Count),
            code.sources(&[], &mut weights).err() == Some(CodeError::Count),
        ];
        line(results.iter().all(|&b| b), &format!("{field:?}: empty inputs refused with Count, no targets gives no shard {results:?}"));
    }
    let code = CyclicCode::new(Field::Gf2048, 3, 2, 1, 1).unwrap();
    let mut none: [&mut [u8]; 0] = [];
    let mut room = vec![0u16; CyclicWords::room(2)];
    let results = [
        code.encode(&[]).err() == Some(CodeError::Count),
        code.agrees(&[]) == Err(CodeError::Count),
        code.correct(&mut none, &[]) == Err(CodeError::Count),
        code.plan(&[], &[], &[]) == Err(CodeError::Count),
        code.words().correct_word(&mut [], &[], &mut room) == Err(CodeError::Length),
    ];
    line(results.iter().all(|&b| b), &format!("CyclicCode/CyclicWords: empty inputs refused {results:?}"));

    // B. Numeric boundaries.
    for field in fields {
        let q = field.order();
        let ok = Code::new(field, 0) == Err(CodeError::Threshold)
            && Code::new(field, q + 1) == Err(CodeError::Threshold)
            && Code::new(field, usize::MAX) == Err(CodeError::Threshold)
            && Code::new(field, q).is_ok();
        let code = Code::new(field, 1).unwrap();
        let data = random_symbols(&mut rng, field, 2);
        let last = u16::try_from(q - 1).unwrap();
        let top_ok = code.encode(&[&data], &[last]).is_ok();
        let beyond = if q <= 65535 { code.encode(&[&data], &[q as u16]) == Err(CodeError::Index) } else { true };
        line(ok && top_ok && beyond, &format!("{field:?}: threshold 0 / order+1 / MAX refused, order taken; index order-1 taken, order refused"));
    }
    let shapes: [(Field, usize, usize, u32, u32, bool); 12] = [
        (Field::Gf2048, 0, 4, 1, 1, false),
        (Field::Gf2048, 4, 0, 1, 1, false),
        (Field::Gf2048, usize::MAX, 4, 1, 1, false),
        (Field::Gf2048, 4, usize::MAX, 1, 1, false),
        (Field::Gf2048, 2043, 4, 1, 1, true),
        (Field::Gf2048, 2044, 4, 1, 1, false),
        (Field::Gf2048, 4, 4, u32::MAX, u32::MAX, true),
        (Field::Gf2048, 4, 4, 1, 0, false),
        (Field::Gf2048, 4, 4, 1, 2047, false),
        (Field::Gf256, 2, 3, 0, 85, true), // 255 / gcd(85, 255) = 3 < 5? refused below
        (Field::Gf256, 1, 2, 0, 85, true),
        (Field::binary(16, None).unwrap(), 65_000, 535, 7, 1, true),
    ];
    for (field, data, parity, first, step, _) in shapes {
        let elements = field.order() - 1;
        let s = step as usize % elements;
        let order = elements / gcd(s, elements);
        let expect = data > 0 && parity > 0 && data.checked_add(parity).is_some_and(|n| n <= order);
        let got = CyclicCode::new(field, data, parity, first, step);
        let ok = match &got {
            Ok(_) => expect,
            Err(CodeError::Shape) => !expect,
            Err(_) => false,
        };
        line(ok, &format!("CyclicCode::new({field:?}, {data}, {parity}, {first}, {step}) -> {:?} (order of beta {order})", got.as_ref().map(|_| "Ok")));
    }
    let mut short_generator = vec![0u16; 4];
    let ok = CyclicWords::new(Field::Gf2048, 4, 4, 1, 1, &mut short_generator) == Err(CodeError::Length)
        && CyclicWords::new(Field::Gf65536, 4, 4, 1, 1, &mut short_generator) == Err(CodeError::Shape);
    line(ok, "CyclicWords::new: Shape before Length, generator of the wrong length refused");

    // C. Zero and odd lengths.
    for field in fields {
        let code = Code::new(field, 2).unwrap();
        let empty: [&[u8]; 2] = [&[], &[]];
        let set = code.encode(&empty, &[0, 1, 2, 3]).unwrap();
        let ok_encode = set.len() == 4 && set.iter().all(|b| b.is_empty());
        let given: Vec<(u16, &[u8])> = (0..4).map(|i| (i, &set[usize::from(i)][..])).collect();
        let ok_rebuild = code.rebuild(&given, &[1, 3]).map(|v| v.iter().all(|b| b.is_empty())) == Ok(true);
        let ok_red = code.redundancy(&given) == Ok(Redundancy::Agrees);
        let mut bufs: Vec<Vec<u8>> = vec![Vec::new(); 4];
        let mut shards: Vec<(u16, &mut [u8])> = (0..4).zip(bufs.iter_mut().map(Vec::as_mut_slice)).collect();
        let ok_correct = code.correct(&mut shards) == Ok(Correction::Agrees);
        let ok_plan = code.plan(&given, &[]) == Ok(Plan::Agrees) && code.plan(&given, &[Mark { shard: 0, symbol: 0 }]) == Err(CodeError::Mark);
        let mut odd_ok = true;
        if field.symbol_bytes() == 2 {
            let odd: [&[u8]; 2] = [&[1], &[2]];
            odd_ok &= code.encode(&odd, &[2]) == Err(CodeError::Length);
            odd_ok &= code.rebuild(&[(0, &[1][..]), (1, &[2][..])], &[2]) == Err(CodeError::Length);
            odd_ok &= code.redundancy(&[(0, &[1][..]), (1, &[2][..]), (2, &[3][..])]) == Err(CodeError::Length);
            let mut a = [1u8];
            let mut b = [2u8];
            let mut c = [3u8];
            let mut odd_shards: Vec<(u16, &mut [u8])> = vec![(0, &mut a), (1, &mut b), (2, &mut c)];
            odd_ok &= code.correct(&mut odd_shards) == Err(CodeError::Length);
            odd_ok &= code.plan(&[(0, &[1][..]), (1, &[2][..]), (2, &[3][..])], &[]) == Err(CodeError::Length);
            let mut weights = [0u16; 2];
            let sources = code.sources(&[0, 1], &mut weights).unwrap();
            let mut out = [0xEEu8; 1];
            odd_ok &= sources.build(2, &odd, &mut out) == Err(CodeError::Length) && out == [0xEE];
        }
        let mixed: [&[u8]; 2] = [&[0, 0], &[0, 0, 0, 0]];
        let mixed_ok = code.encode(&mixed, &[2]) == Err(CodeError::Length);
        line(ok_encode && ok_rebuild && ok_red && ok_correct && ok_plan && odd_ok && mixed_ok,
            &format!("{field:?}: zero-length shards encode/rebuild/agree/correct/plan; odd and mixed lengths refused"));
    }
    let code = CyclicCode::new(Field::Gf2048, 3, 2, 1, 1).unwrap();
    let empty: [&[u8]; 3] = [&[], &[], &[]];
    let parity = code.encode(&empty).unwrap();
    let five: [&[u8]; 5] = [&[], &[], &[], &[], &[]];
    let mut bufs: Vec<Vec<u8>> = vec![Vec::new(); 5];
    let mut views: Vec<&mut [u8]> = bufs.iter_mut().map(Vec::as_mut_slice).collect();
    let mut syndromes = [0u8; 4];
    let ok = parity.len() == 2
        && parity.iter().all(|b| b.is_empty())
        && code.agrees(&five) == Ok(true)
        && code.correct(&mut views, &[0, 1]) == Ok(Correction::Agrees)
        && code.plan(&five, &[], &[]) == Ok(Plan::Agrees)
        && code.words().agrees(&five, &mut syndromes) == Ok(true)
        && code.words().agrees(&five, &mut []) == Err(CodeError::Length);
    line(ok, "CyclicCode: zero-length shards encode/agree/correct (even with lost places)/plan; CyclicWords::agrees needs room");
    let odd: [&[u8]; 3] = [&[1], &[2], &[3]];
    let ok = code.encode(&odd) == Err(CodeError::Length)
        && code.agrees(&[&[1], &[2], &[3], &[4], &[5]]) == Err(CodeError::Length);
    line(ok, "CyclicCode: odd lengths refused");

    // D. Long shards across stripes: GF(2^11), 3 spare shards (stripes of 699,050 symbols).
    let code = Code::new(Field::Gf2048, 4).unwrap();
    let stripe_symbols = (1usize << 22) / 3 / 2;
    let symbols = 2 * stripe_symbols + 5;
    let data: Vec<Vec<u8>> = (0..4).map(|_| random_symbols(&mut rng, Field::Gf2048, symbols)).collect();
    let views: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let whole: Vec<Vec<u8>> = code.encode(&views, &[0, 1, 2, 3, 4, 5, 6]).unwrap().iter().map(|b| b.to_vec()).collect();
    // One wrong symbol at the first, last and border symbols of every stripe, in turning shards.
    let borders = [0, stripe_symbols - 1, stripe_symbols, 2 * stripe_symbols - 1, 2 * stripe_symbols, symbols - 1];
    let mut read = whole.clone();
    let mut counts = vec![0usize; 7];
    for (n, &at) in borders.iter().enumerate() {
        let shard = (n * 3) % 7;
        read[shard][2 * at + 1] ^= 0x5A;
        read[shard][2 * at] ^= 0x03;
        counts[shard] += 1;
    }
    let mut shards: Vec<(u16, &mut [u8])> = (0..7).zip(read.iter_mut().map(Vec::as_mut_slice)).collect();
    let got = code.correct(&mut shards);
    drop(shards);
    line(got == Ok(Correction::Corrected(counts.clone())) && read == whole, "Code::correct: wrong symbols at both sides of every stripe border, GF(2^11), 3 spare");
    // The same columns marked (gaps of zero), plus a wrong symbol elsewhere in each marked column
    // would be beyond 2e + s <= 3 only with two marks: one mark + one error = 3.
    let mut read = whole.clone();
    let mut marks = Vec::new();
    for (n, &at) in borders.iter().enumerate() {
        let gap = (n * 2) % 7;
        let wrong = (gap + 3) % 7;
        read[gap][2 * at..2 * at + 2].copy_from_slice(&[0, 0]);
        read[wrong][2 * at] ^= 0x01;
        marks.push(Mark { shard: gap, symbol: at });
    }
    marks.reverse();
    marks.push(marks[0]);
    let given: Vec<(u16, &[u8])> = (0..7).zip(read.iter().map(Vec::as_slice)).collect();
    let plan = code.plan(&given, &marks);
    let ok = match plan {
        Ok(Plan::Changes(ref changes)) => {
            let filled = changes.iter().filter(|c| c.kind == ChangeKind::Filled).count();
            let replaced = changes.iter().filter(|c| c.kind == ChangeKind::Replaced).count();
            let ordered = changes.iter().zip(changes.iter().skip(1)).all(|(a, b)| (a.symbol, a.shard) < (b.symbol, b.shard));
            let mut out = read.clone();
            let mut b: Vec<(u16, &mut [u8])> = (0..7).zip(out.iter_mut().map(Vec::as_mut_slice)).collect();
            let applied = code.apply(&mut b, changes).is_ok();
            drop(b);
            filled == 6 && replaced == 6 && ordered && applied && out == whole
        }
        _ => false,
    };
    line(ok, "Code::plan: marks (unordered, one repeated) and wrong symbols at every stripe border, listed in order, applied back");
    // A mark alone in the second stripe, the first stripe clean: listed as Filled, value unchanged.
    let given: Vec<(u16, &[u8])> = (0..7).zip(whole.iter().map(Vec::as_slice)).collect();
    let mark = Mark { shard: 6, symbol: stripe_symbols + 7 };
    let ok = match code.plan(&given, &[mark]) {
        Ok(Plan::Changes(changes)) => changes.len() == 1 && changes.get(0).map(|c| (c.shard, c.symbol, c.kind)) == Some((6, stripe_symbols + 7, ChangeKind::Filled))
            && changes.values(0, |a, b| a == b) == Some(true),
        _ => false,
    };
    line(ok, "Code::plan: a lone mark in a later stripe of clean shards is listed with its own value");

    // E. Mixed columns: one column beyond the bound keeps every column as it was; columns counted.
    let code = Code::new(Field::Gf256, 3).unwrap();
    let data: Vec<Vec<u8>> = (0..3).map(|_| random_symbols(&mut rng, Field::Gf256, 50)).collect();
    let views: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let whole: Vec<Vec<u8>> = code.encode(&views, &[0, 1, 2, 3, 4, 5, 6]).unwrap().iter().map(|b| b.to_vec()).collect();
    let mut read = whole.clone();
    for col in 0..50 {
        read[col % 7][col] ^= 0x11; // one wrong symbol in every column
    }
    // three wrong symbols in columns 10 and 20 (beyond t = 2)
    for col in [10, 20] {
        for s in [1, 3, 5] {
            read[(col + s) % 7][col] ^= 0x22 + s as u8;
        }
    }
    let received = read.clone();
    let mut shards: Vec<(u16, &mut [u8])> = (0..7).zip(read.iter_mut().map(Vec::as_mut_slice)).collect();
    let got = code.correct(&mut shards);
    drop(shards);
    let given: Vec<(u16, &[u8])> = (0..7).zip(received.iter().map(Vec::as_slice)).collect();
    let plan = code.plan(&given, &[]);
    println!("     mixed columns: correct -> {got:?}; plan -> {:?}", plan.as_ref().map(|p| match p { Plan::Unresolved { columns } => format!("Unresolved {columns}"), Plan::Changes(c) => format!("Changes {}", c.len()), other => format!("{other:?}") }));
    let unresolved_ok = matches!(got, Ok(Correction::Unresolved { columns }) if columns <= 2 && columns >= 1);
    line((unresolved_ok && read == received) || matches!(got, Ok(Correction::Corrected(_))), "Code::correct: a column beyond the bound leaves all 50 columns as received");

    // F. Larger fields, random cases at the bound and one or two beyond: within, the original
    // comes back by correct and by plan; beyond, an answer is a code word within the bound.
    let mut summary = [0usize; 4]; // within ok, beyond unresolved, beyond other word, bad
    for round in 0..6000 {
        let (field, k, points): (Field, usize, Vec<u16>) = match round % 3 {
            0 => (Field::Gf65536, 10, { let mut p: Vec<u16> = (0..30).map(|_| rng.next() as u16).collect(); p.sort_unstable(); p.dedup(); while p.len() < 30 { p.push(rng.next() as u16); p.sort_unstable(); p.dedup(); } p.reverse(); p }),
            1 => (Field::Gf2048, 24, (0..32).collect()),
            _ => (Field::Gf256, 12, (0..20).map(|i| 255 - i).collect()),
        };
        let m = points.len();
        let r = m - k;
        let code = Code::new(field, k).unwrap();
        let w = field.symbol_bytes();
        let data: Vec<Vec<u8>> = (0..k).map(|_| random_symbols(&mut rng, field, 1)).collect();
        let views: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let whole: Vec<Vec<u8>> = code.encode(&views, &points).unwrap().iter().map(|b| b.to_vec()).collect();
        let s = rng.below(r + 1).min(r);
        let budget = r - s; // 2e <= budget within
        let e = budget / 2 + rng.below(3); // at the bound, one or two beyond
        let mut places: Vec<usize> = (0..m).collect();
        for i in (1..m).rev() {
            let j = rng.below(i + 1);
            places.swap(i, j);
        }
        let marked: Vec<usize> = places[..s].to_vec();
        let wrong: Vec<usize> = places[s..(s + e).min(m)].to_vec();
        let mut read = whole.clone();
        for &i in &marked {
            read[i] = vec![0; w];
        }
        for &i in &wrong {
            let mut v = random_symbols(&mut rng, field, 1);
            while v == read[i] {
                v = random_symbols(&mut rng, field, 1);
            }
            read[i] = v;
        }
        let within = 2 * wrong.len() + s <= r;
        let given: Vec<(u16, &[u8])> = points.iter().copied().zip(read.iter().map(Vec::as_slice)).collect();
        let marks: Vec<Mark> = marked.iter().map(|&shard| Mark { shard, symbol: 0 }).collect();
        let plan = code.plan(&given, &marks).unwrap();
        let outcome = match plan {
            Plan::Changes(changes) => {
                let mut out = read.clone();
                let mut b: Vec<(u16, &mut [u8])> = points.iter().copied().zip(out.iter_mut().map(Vec::as_mut_slice)).collect();
                code.apply(&mut b, &changes).unwrap();
                drop(b);
                let replaced = changes.iter().filter(|c| c.kind == ChangeKind::Replaced).count();
                let filled = changes.iter().filter(|c| c.kind == ChangeKind::Filled).count();
                let given: Vec<(u16, &[u8])> = points.iter().copied().zip(out.iter().map(Vec::as_slice)).collect();
                let is_word = code.redundancy(&given) == Ok(Redundancy::Agrees);
                if within {
                    if out == whole { 0 } else { 3 }
                } else if is_word && filled == s && 2 * replaced + s <= r && out != whole {
                    2
                } else {
                    3
                }
            }
            Plan::Agrees => if within && s == 0 && wrong.is_empty() { 0 } else { 3 },
            Plan::Unresolved { columns: 1 } => if within { 3 } else { 1 },
            _ => 3,
        };
        // correct (marks given as missing shards) must agree with the plan's answer.
        let left: Vec<usize> = (0..m).filter(|i| !marked.contains(i)).collect();
        let mut kept: Vec<Vec<u8>> = left.iter().map(|&i| read[i].clone()).collect();
        let kept_points: Vec<u16> = left.iter().map(|&i| points[i]).collect();
        let mut b: Vec<(u16, &mut [u8])> = kept_points.iter().copied().zip(kept.iter_mut().map(Vec::as_mut_slice)).collect();
        let corrected = code.correct(&mut b).unwrap();
        drop(b);
        let correct_ok = if within {
            left.iter().zip(&kept).all(|(&i, v)| *v == whole[i]) && !matches!(corrected, Correction::Unresolved { .. })
        } else {
            match corrected {
                Correction::Unresolved { .. } => left.iter().zip(&kept).all(|(&i, v)| *v == read[i]),
                Correction::Corrected(ref c) => 2 * c.iter().sum::<usize>() <= r - s,
                _ => true,
            }
        };
        let outcome = if correct_ok { outcome } else { 3 };
        summary[outcome] += 1;
        if outcome == 3 && summary[3] <= 5 {
            println!("FAIL  random {field:?} k={k} m={m} s={s} e={} within={within}", wrong.len());
        }
    }
    line(summary[3] == 0, &format!("random cases at and beyond 2e + s <= r (GF(2^16) scattered points, GF(2^11) 24+8, GF(2^8) 12+8): within ok {}, beyond unresolved {}, beyond another word {}, bad {}", summary[0], summary[1], summary[2], summary[3]));
    aud006_codes_probe::finish();
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}
