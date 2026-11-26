//! Independent finite-state oracles for the AUD-008 review. All data is synthetic.

#[cfg(test)]
mod tests {
    use reed_solomon_core::{
        ChangeKind, Changes, Code, CyclicCode, CyclicWords, Field, Mark, Plan,
    };

    // Literal multiplication table for GF(4), x^2+x+1. No library arithmetic or encoder.
    const MUL: [[u8; 4]; 4] = [[0, 0, 0, 0], [0, 1, 2, 3], [0, 2, 3, 1], [0, 3, 1, 2]];

    fn words(length: usize) -> Vec<Vec<u8>> {
        (0..4_usize.pow(length as u32))
            .map(|mut number| {
                (0..length)
                    .map(|_| {
                        let digit = (number % 4) as u8;
                        number /= 4;
                        digit
                    })
                    .collect()
            })
            .collect()
    }

    fn evaluate(coefficients: &[u8], x: u8) -> u8 {
        coefficients.iter().fold(0, |sum, &coefficient| {
            MUL[sum as usize][x as usize] ^ coefficient
        })
    }

    fn nearest<'a>(
        valid: &'a [Vec<u8>],
        received: &[u8],
        mask: usize,
        parity: usize,
    ) -> Option<&'a Vec<u8>> {
        let marked = mask.count_ones() as usize;
        let candidates: Vec<_> = valid
            .iter()
            .filter(|candidate| {
                let errors = candidate
                    .iter()
                    .zip(received)
                    .enumerate()
                    .filter(|(at, (expected, actual))| mask & (1 << at) == 0 && expected != actual)
                    .count();
                marked + 2 * errors <= parity
            })
            .collect();
        assert!(
            candidates.len() <= 1,
            "independent codebook violates unique decoding bound"
        );
        candidates.first().copied()
    }

    fn shards(word: &[u8]) -> Vec<Vec<u8>> {
        word.iter().map(|&value| vec![value]).collect()
    }

    fn check_changes(changes: &Changes, received: &[u8], wanted: &[u8], mask: usize) {
        let expected: Vec<_> = received
            .iter()
            .zip(wanted)
            .enumerate()
            .filter(|(at, (a, b))| mask & (1 << at) != 0 || a != b)
            .map(|(at, (&was, &becomes))| {
                (
                    at,
                    0,
                    if mask & (1 << at) != 0 {
                        ChangeKind::Filled
                    } else {
                        ChangeKind::Replaced
                    },
                    u16::from(was),
                    u16::from(becomes),
                )
            })
            .collect();
        let actual: Vec<_> = changes
            .iter()
            .enumerate()
            .map(|(at, change)| {
                let (was, becomes) = changes.values(at, |was, becomes| (was, becomes)).unwrap();
                (change.shard, change.symbol, change.kind, was, becomes)
            })
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn every_polynomial_word_and_erasure_mask_over_gf4() {
        let field = Field::binary(2, Some(0x7)).unwrap();
        let mut cases = 0;
        for threshold in [1, 2] {
            let code = Code::new(field, threshold).unwrap();
            let valid: Vec<Vec<u8>> = words(threshold)
                .iter()
                .map(|coefficients| (0..4).map(|x| evaluate(coefficients, x)).collect())
                .collect();
            for word in &valid {
                let input = shards(&word[..threshold]);
                let input: Vec<_> = input.iter().map(Vec::as_slice).collect();
                let encoded = code.encode(&input, &[0, 1, 2, 3]).unwrap();
                assert_eq!(encoded.iter().map(|v| v[0]).collect::<Vec<_>>(), *word);
            }
            for received in words(4) {
                for mask in 0..16 {
                    let expected = nearest(&valid, &received, mask, 4 - threshold);
                    let mut output = shards(&received);
                    let refs: Vec<_> = output
                        .iter()
                        .enumerate()
                        .map(|(at, v)| (at as u16, v.as_slice()))
                        .collect();
                    let marks: Vec<_> = (0..4)
                        .filter(|at| mask & (1 << at) != 0)
                        .map(|shard| Mark { shard, symbol: 0 })
                        .collect();
                    let plan = code.plan(&refs, &marks).unwrap();
                    match (expected, &plan) {
                        (None, Plan::Unresolved { columns: 1 }) => {}
                        (Some(wanted), Plan::Agrees) => {
                            assert_eq!(&received, wanted);
                            assert_eq!(mask, 0);
                        }
                        (Some(wanted), Plan::Changes(changes)) => {
                            check_changes(changes, &received, wanted, mask);
                            let count = received
                                .iter()
                                .zip(wanted)
                                .enumerate()
                                .filter(|(at, (a, b))| mask & (1 << at) != 0 || a != b)
                                .count();
                            assert_eq!(changes.len(), count);
                            let mut refs: Vec<_> = output
                                .iter_mut()
                                .enumerate()
                                .map(|(at, v)| (at as u16, v.as_mut_slice()))
                                .collect();
                            code.apply(&mut refs, changes).unwrap();
                            assert_eq!(output.iter().map(|v| v[0]).collect::<Vec<_>>(), *wanted);
                        }
                        _ => panic!(
                            "threshold={threshold} received={received:?} mask={mask} expected={expected:?} plan={plan:?}"
                        ),
                    }
                    cases += 1;
                }
            }
        }
        assert_eq!(cases, 8192);
        println!("polynomial exhaustive cases: {cases}");
    }

    #[test]
    fn every_cyclic_word_and_erasure_mask_over_gf4() {
        let field = Field::binary(2, Some(0x7)).unwrap();
        let powers = [1, 2, 3];
        let mut cases = 0;
        for data in [1, 2] {
            let parity = 3 - data;
            for first in 0..3 {
                for step in [1, 2] {
                    let code = CyclicCode::new(field, data, parity, first, step).unwrap();
                    let valid: Vec<_> = words(3)
                        .into_iter()
                        .filter(|word| {
                            (0..parity).all(|at| {
                                evaluate(word, powers[((first as usize + at) * step as usize) % 3])
                                    == 0
                            })
                        })
                        .collect();
                    assert_eq!(valid.len(), 4_usize.pow(data as u32));
                    for word in &valid {
                        let input = shards(&word[..data]);
                        let refs: Vec<_> = input.iter().map(Vec::as_slice).collect();
                        let encoded = code.encode(&refs).unwrap();
                        assert_eq!(
                            encoded.iter().map(|v| v[0]).collect::<Vec<_>>(),
                            word[data..]
                        );
                    }
                    for received in words(3) {
                        for mask in 0..8 {
                            let expected = nearest(&valid, &received, mask, parity);
                            let marks: Vec<_> = (0..3).filter(|at| mask & (1 << at) != 0).collect();
                            let mut word: Vec<_> = received
                                .iter()
                                .enumerate()
                                .map(|(at, &v)| {
                                    if mask & (1 << at) != 0 {
                                        0xFFFF
                                    } else {
                                        u16::from(v)
                                    }
                                })
                                .collect();
                            let untouched = word.clone();
                            let mut room = vec![0xFFFF; CyclicWords::room(parity)];
                            let result = code
                                .words()
                                .correct_word(&mut word, &marks, &mut room)
                                .unwrap();
                            assert!(room.iter().all(|&v| v == 0));
                            if let Some(wanted) = expected {
                                let count = wanted
                                    .iter()
                                    .zip(&received)
                                    .enumerate()
                                    .filter(|(at, (a, b))| mask & (1 << at) != 0 || a != b)
                                    .count();
                                assert_eq!(result, Some(count));
                                assert_eq!(
                                    word,
                                    wanted.iter().map(|&v| u16::from(v)).collect::<Vec<_>>()
                                );
                            } else {
                                assert_eq!(result, None);
                                assert_eq!(word, untouched);
                            }
                            let mut output = shards(&received);
                            let refs: Vec<_> = output.iter().map(Vec::as_slice).collect();
                            let marked: Vec<_> = marks
                                .iter()
                                .map(|&shard| Mark { shard, symbol: 0 })
                                .collect();
                            let plan = code.plan(&refs, &[], &marked).unwrap();
                            match (expected, &plan) {
                                (None, Plan::Unresolved { columns: 1 }) => {}
                                (Some(wanted), Plan::Agrees) => {
                                    assert_eq!(&received, wanted);
                                    assert_eq!(mask, 0);
                                }
                                (Some(wanted), Plan::Changes(changes)) => {
                                    check_changes(changes, &received, wanted, mask);
                                    let mut refs: Vec<_> =
                                        output.iter_mut().map(Vec::as_mut_slice).collect();
                                    code.apply(&mut refs, changes).unwrap();
                                    assert_eq!(
                                        output.iter().map(|v| v[0]).collect::<Vec<_>>(),
                                        *wanted
                                    );
                                }
                                _ => panic!(
                                    "data={data} first={first} step={step} received={received:?} mask={mask} expected={expected:?} plan={plan:?}"
                                ),
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 6144);
        println!("cyclic exhaustive cases: {cases}");
    }

    #[test]
    fn all_small_file_subsets_rebuild_and_repair_exactly() {
        use reed_solomon_core::{DataDigest, DigestCheck, Pile, SetError, SetFacts};
        let mut cases = 0;
        for field in [Field::Gf256, Field::Gf65536] {
            for threshold in 1..=3 {
                for length in 0..=8 {
                    let data: Vec<_> = (0..length).map(|at| (at * 31 + length) as u8).collect();
                    for symbols in 1..=3 {
                        let facts = SetFacts::for_data(
                            field,
                            b"AUD-008",
                            threshold,
                            &data,
                            (symbols * field.symbol_bytes()) as u32,
                            DataDigest::Sha256,
                        )
                        .unwrap();
                        let indexes: Vec<_> = (0..threshold + 2).map(|at| at as u16).collect();
                        let files = facts.files(&data, &indexes).unwrap();
                        for mask in 1_usize..1 << indexes.len() {
                            let selected: Vec<_> = files
                                .iter()
                                .enumerate()
                                .filter(|(at, _)| mask & (1 << at) != 0)
                                .map(|(_, file)| file.as_slice())
                                .collect();
                            let pile = Pile::new(&selected).unwrap();
                            let set = &pile.sets()[0];
                            if selected.len() < threshold {
                                assert!(matches!(set.rebuild(None), Err(SetError::Shards { .. })));
                            } else {
                                let rebuilt = set.rebuild(None).unwrap();
                                assert_eq!(rebuilt.data.as_slice(), data);
                                assert!(matches!(rebuilt.digest, DigestCheck::Matched(_)));
                                assert_eq!(rebuilt.steps, set.rebuild_work().unwrap());
                                if rebuilt.steps > 0 {
                                    assert!(matches!(
                                        set.rebuild(Some(rebuilt.steps - 1)),
                                        Err(SetError::Work { .. })
                                    ));
                                }
                                let mut wanted = indexes.clone();
                                wanted.reverse();
                                wanted.push(0);
                                let repaired = set.repairs(&wanted, None).unwrap();
                                for (&index, actual) in wanted.iter().zip(&repaired) {
                                    assert_eq!(actual, &files[index as usize]);
                                }
                                let vote = set.vote(None).unwrap();
                                assert!(vote.outvoted().is_empty());
                                assert_eq!(vote.undecided(), 0);
                                assert!(vote.steps() <= set.vote_work().unwrap());
                            }
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 2862);
        println!("file subset cases: {cases}");
    }
}
