//! MHFE-REPAIR-1: repair-word cards of every case and the three repairs of repair-words.json,
//! computed through the public CyclicCode / CyclicWords API, with an independent long division.

use aud006_codes_probe::gf::{Gf, cyclic_parity};
use reed_solomon_core::{ChangeKind, CodeError, Correction, CyclicCode, CyclicWords, Field, Mark, Plan};
use std::collections::BTreeMap;

fn be(words: &[u16]) -> Vec<Vec<u8>> {
    words.iter().map(|w| w.to_be_bytes().to_vec()).collect()
}
fn views(s: &[Vec<u8>]) -> Vec<&[u8]> {
    s.iter().map(Vec::as_slice).collect()
}
fn num(b: &[u8]) -> u16 {
    b.iter().fold(0u16, |v, &x| v << 8 | u16::from(x))
}

fn main() {
    let base = std::env::args().nth(1).expect("data dir");
    let text = std::fs::read_to_string(format!("{base}/mhfe-vectors.txt")).unwrap();
    let list: Vec<String> = std::fs::read_to_string(format!("{base}/../bip39-crate-english.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(list.len(), 2048);
    let name = |i: u16| list[usize::from(i)].clone();
    let gf = Gf::new(11, 0x805);
    let mut cases: BTreeMap<String, Vec<u16>> = BTreeMap::new();
    let mut cards: BTreeMap<(String, usize), Vec<u16>> = BTreeMap::new();
    let mut repairs = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("case") => {
                let id = parts.next().unwrap().to_owned();
                cases.insert(id, parts.map(|p| p.parse().unwrap()).collect());
            }
            Some("card") => {
                let id = parts.next().unwrap().to_owned();
                let k: usize = parts.next().unwrap().parse().unwrap();
                cards.insert((id, k), parts.map(|p| p.parse().unwrap()).collect());
            }
            Some("repair") => repairs.push(line.to_owned()),
            _ => {}
        }
    }
    // Generators the specification prints.
    let printed: [&[u16]; 4] = [
        &[1, 6, 8],
        &[1, 30, 216, 960, 1024],
        &[1, 126, 1181, 1719, 2029, 1077, 1034],
        &[1, 510, 1509, 1770, 1837, 850, 1339, 600, 680],
    ];
    for g in printed {
        let k = g.len() - 1;
        let code = CyclicCode::new(Field::Gf2048, 24, k, 1, 1).unwrap();
        let (mine, _) = cyclic_parity(&gf, &[0; 24], k, 1, 1);
        println!(
            "generator k={k}: crate {:?} spec {:?} independent {:?} -> {}",
            code.generator(),
            g,
            mine,
            if code.generator() == g && mine == g { "AGREE" } else { aud006_codes_probe::fail(); "DIFFER" }
        );
    }
    let mut all_agree = true;
    for ((id, k), expected) in &cards {
        let data = &cases[id];
        let n = data.len();
        let code = CyclicCode::new(Field::Gf2048, n, *k, 1, 1).unwrap();
        let shards = be(data);
        let parity: Vec<u16> = code.encode(&views(&shards)).unwrap().iter().map(|b| num(b)).collect();
        // The same through CyclicWords in caller memory.
        let mut generator = vec![0u16; k + 1];
        let words = CyclicWords::new(Field::Gf2048, n, *k, 1, 1, &mut generator).unwrap();
        let mut outs = vec![vec![0xEEu8; 2]; *k];
        let mut out_views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        let mut remainder = vec![0u16; *k];
        words.encode(&views(&shards), &mut out_views, &mut remainder).unwrap();
        let parity_words: Vec<u16> = outs.iter().map(|b| num(b)).collect();
        let (_, mine) = cyclic_parity(&gf, data, *k, 1, 1);
        let mut whole = shards.clone();
        whole.extend(be(&parity));
        let agrees = code.agrees(&views(&whole)).unwrap();
        let ok = &parity == expected && parity_words == parity && mine == parity && agrees;
        all_agree &= ok;
        println!(
            "card {id} n={n} k={k}: crate '{}' expected '{}' words-api {} independent {} agrees {agrees} -> {}",
            parity.iter().map(|&i| name(i)).collect::<Vec<_>>().join(" "),
            expected.iter().map(|&i| name(i)).collect::<Vec<_>>().join(" "),
            if parity_words == parity { "same" } else { "DIFFERENT" },
            if mine == parity { "same" } else { "DIFFERENT" },
            if ok { "AGREE" } else { aud006_codes_probe::fail(); "DIFFER" }
        );
    }
    println!("all cards agree: {all_agree}");
    if !all_agree {
        aud006_codes_probe::fail();
    }
    for line in &repairs {
        // repair <id> <case> <k> unreadable <pos...> ; replaced <p=i>... ; restored <p=i>...
        let (head, rest) = line.split_once(" unreadable").unwrap();
        let head: Vec<&str> = head.split_whitespace().collect();
        let (rid, case, k) = (head[1], head[2], head[3].parse::<usize>().unwrap());
        let sections: Vec<&str> = rest.split(';').collect();
        let unreadable: Vec<usize> = sections[0].split_whitespace().map(|p| p.parse().unwrap()).collect();
        let pairs = |s: &str| -> Vec<(usize, u16)> {
            s.split_whitespace()
                .skip(1)
                .map(|kv| {
                    let (p, i) = kv.split_once('=').unwrap();
                    (p.parse().unwrap(), i.parse().unwrap())
                })
                .collect()
        };
        let replaced = pairs(sections[1]);
        let restored = pairs(sections[2]);
        let data = &cases[case];
        let card = &cards[&(case.to_owned(), k)];
        let original: Vec<u16> = data.iter().chain(card).copied().collect();
        let n = original.len();
        let code = CyclicCode::new(Field::Gf2048, data.len(), k, 1, 1).unwrap();
        let erased: Vec<usize> = unreadable.iter().map(|p| p - 1).collect();
        let damaged = |placeholder: u16| {
            let mut w = original.clone();
            for &p in &erased {
                w[p] = placeholder;
            }
            for &(p, i) in &replaced {
                w[p - 1] = i;
            }
            w
        };
        let check = |label: &str, got: &[u16], counted: String| {
            let exact = got == original.as_slice();
            let at: Vec<String> = restored
                .iter()
                .map(|&(p, i)| format!("{p}:{}{}", name(got[p - 1]), if got[p - 1] == i { "" } else { "(WRONG)" }))
                .collect();
            println!("  {rid} via {label}: restored [{}] whole word {} {counted}", at.join(", "), if exact { "AGREES" } else { aud006_codes_probe::fail(); "DIFFERS" });
        };
        println!("repair {rid} (case {case}, k={k}, unreadable {unreadable:?}, replaced {:?})", replaced.iter().map(|&(p, i)| format!("{p}={}", name(i))).collect::<Vec<_>>());
        // (a) CyclicWords::correct_word with a placeholder outside the field and with zero.
        for placeholder in [0xFFFF_u16, 0] {
            let mut generator = vec![0u16; k + 1];
            let words = CyclicWords::new(Field::Gf2048, data.len(), k, 1, 1, &mut generator).unwrap();
            let mut w = damaged(placeholder);
            let mut room = vec![0u16; CyclicWords::room(k)];
            let r = words.correct_word(&mut w, &erased, &mut room);
            check(&format!("CyclicWords::correct_word placeholder {placeholder:#x}"), &w, format!("returned {r:?}"));
        }
        // (b) CyclicCode::plan with marks (placeholder zero, a field element) and apply.
        let mut held = be(&damaged(0));
        let marks: Vec<Mark> = erased.iter().map(|&shard| Mark { shard, symbol: 0 }).collect();
        match code.plan(&views(&held), &[], &marks).unwrap() {
            Plan::Changes(changes) => {
                let kinds: Vec<String> = changes
                    .iter()
                    .enumerate()
                    .map(|(at, c)| {
                        let (was, becomes) = changes.values(at, |a, b| (a, b)).unwrap();
                        format!("{}:{}->{}:{}", c.shard + 1, name(was), name(becomes), match c.kind { ChangeKind::Filled => "Filled", ChangeKind::Replaced => "Replaced" })
                    })
                    .collect();
                let mut b: Vec<&mut [u8]> = held.iter_mut().map(Vec::as_mut_slice).collect();
                code.apply(&mut b, &changes).unwrap();
                let got: Vec<u16> = held.iter().map(|s| num(s)).collect();
                check("CyclicCode::plan(marks)+apply", &got, format!("changes {kinds:?}"));
            }
            other => {
                aud006_codes_probe::fail();
                println!("  {rid} via CyclicCode::plan(marks): {other:?} -- NOT REPAIRED");
            }
        }
        // (c) CyclicCode::plan with the unreadable places as lost shards (placeholder 0xFFFF).
        let mut held = be(&damaged(0xFFFF));
        match code.plan(&views(&held), &erased, &[]).unwrap() {
            Plan::Changes(changes) => {
                let mut b: Vec<&mut [u8]> = held.iter_mut().map(Vec::as_mut_slice).collect();
                code.apply(&mut b, &changes).unwrap();
                let got: Vec<u16> = held.iter().map(|s| num(s)).collect();
                check("CyclicCode::plan(lost)+apply", &got, format!("{} changes", changes.len()));
            }
            other => {
                aud006_codes_probe::fail();
                println!("  {rid} via CyclicCode::plan(lost): {other:?} -- NOT REPAIRED");
            }
        }
        // (d) CyclicCode::correct in place with lost places (placeholder 0xFFFF).
        let mut held = be(&damaged(0xFFFF));
        let mut b: Vec<&mut [u8]> = held.iter_mut().map(Vec::as_mut_slice).collect();
        let r: Result<Correction, CodeError> = code.correct(&mut b, &erased);
        drop(b);
        let got: Vec<u16> = held.iter().map(|s| num(s)).collect();
        check("CyclicCode::correct(lost)", &got, format!("returned {r:?}"));
        let _ = n;
    }
    aud006_codes_probe::finish();
}
