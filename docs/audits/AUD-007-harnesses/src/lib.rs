//! Regression probes for AUD-007, using public synthetic shard data only.
#[cfg(test)]
mod tests {
    use reed_solomon_core::{Code, Field, Header, Pile, SetFacts, Vote};

    const K: usize = 3;
    const BLOCK: usize = 4;
    struct Case {
        holders: &'static [usize],
        wrong: &'static [(usize, usize)],
    }

    fn cases() -> Vec<Case> {
        vec![
            Case {
                holders: &[0, 1, 2, 3, 4],
                wrong: &[(0, 0)],
            },
            Case {
                holders: &[0, 1, 2, 5, 6],
                wrong: &[(6, 1)],
            },
            Case {
                holders: &[0, 2, 3, 4, 6],
                wrong: &[(0, 0), (2, 1)],
            },
            Case {
                holders: &[1, 2, 3, 4, 5],
                wrong: &[],
            },
            Case {
                holders: &[0, 1, 2, 3, 6],
                wrong: &[(3, 0), (3, 1)],
            },
            Case {
                holders: &[0, 1, 2, 3, 6],
                wrong: &[],
            },
            Case {
                holders: &[0, 1, 2, 3, 4, 5, 6],
                wrong: &[(0, 0), (6, 1)],
            },
            Case {
                holders: &[0, 1, 2],
                wrong: &[],
            },
            Case {
                holders: &[0, 1],
                wrong: &[],
            },
            Case {
                holders: &[0, 1, 2, 3, 4, 5, 6],
                wrong: &[],
            },
            Case {
                holders: &[0, 2, 3, 4, 5],
                wrong: &[(5, 0)],
            },
            Case {
                holders: &[0, 1, 3, 4, 6],
                wrong: &[],
            },
        ]
    }

    fn vote(files: &[Vec<u8>]) -> Vote {
        let refs: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
        let pile = Pile::new(&refs).unwrap();
        assert_eq!(pile.sets().len(), 1);
        let set = &pile.sets()[0];
        let bound = set.vote_work().unwrap();
        let vote = set.vote(Some(bound)).unwrap();
        assert!(vote.steps() <= bound);
        if bound > 0 {
            assert!(matches!(
                set.vote(Some(bound - 1)),
                Err(reed_solomon_core::SetError::Work { .. })
            ));
        }
        vote
    }

    fn marked(vote: &Vote, block: u64) -> Vec<usize> {
        (0..7)
            .filter(|&p| vote.outvoted_blocks(p).any(|b| b == block))
            .collect()
    }

    fn files(facts: SetFacts, indexes: &[u16], raw: &[Vec<u8>], cases: &[Case]) -> Vec<Vec<u8>> {
        indexes
            .iter()
            .enumerate()
            .map(|(place, &index)| {
                let header = Header::new(facts, index).unwrap();
                let mut bytes = header.file(&raw[place]).unwrap().to_vec();
                for (block, case) in cases.iter().enumerate() {
                    if !case.holders.contains(&place) {
                        // A bit changed after its CRC excludes this holder from this block.
                        bytes
                            [header.as_bytes().len() + block * (facts.block_len() as usize + 4)] ^=
                            0x80;
                    }
                }
                bytes
            })
            .collect()
    }

    fn check(field: Field) {
        let cases = cases();
        let indexes = if field == Field::Gf256 {
            [0, 1, 7, 31, 129, 254, 255]
        } else {
            [0, 1, 7, 31, 4097, 65000, 65535]
        };
        let width = field.symbol_bytes();
        let length = (cases.len() - 1) * BLOCK + 2;
        let data: Vec<Vec<u8>> = (0..K)
            .map(|row| {
                (0..length)
                    .map(|at| (row * 53 + at * 17 + 11) as u8)
                    .collect()
            })
            .collect();
        let refs: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let code = Code::new(field, K).unwrap();
        let mut raw: Vec<Vec<u8>> = code
            .encode(&refs, &indexes)
            .unwrap()
            .iter()
            .map(|b| b.to_vec())
            .collect();
        for (block, case) in cases.iter().enumerate() {
            for &(place, column) in case.wrong {
                raw[place][block * BLOCK + column * width] ^= 0x5b;
            }
        }
        let facts = SetFacts::new(field, b"r", K, (K * length) as u64, BLOCK as u32, None).unwrap();
        let whole_files = files(facts, &indexes, &raw, &cases);
        let whole = vote(&whole_files);
        let mut expected_steps = 0;
        let mut previous: &[usize] = &[];
        for (block, case) in cases.iter().enumerate() {
            let start = block * BLOCK;
            let end = (start + BLOCK).min(length);
            let part: Vec<Vec<u8>> = raw.iter().map(|bytes| bytes[start..end].to_vec()).collect();
            let one = SetFacts::new(
                field,
                b"r",
                K,
                (K * (end - start)) as u64,
                (end - start) as u32,
                None,
            )
            .unwrap();
            let fresh_files = files(one, &indexes, &part, core::slice::from_ref(case));
            let fresh = vote(&fresh_files);
            assert_eq!(
                marked(&whole, block as u64),
                marked(&fresh, 0),
                "{field:?} block {block}"
            );
            assert_eq!(
                whole.undecided_blocks().any(|b| b == block as u64),
                fresh.undecided() == 1,
                "{field:?} block {block}"
            );
            let mut literal: Vec<usize> = case.wrong.iter().map(|&(place, _)| place).collect();
            literal.sort_unstable();
            literal.dedup();
            if block == 2 {
                assert_eq!(fresh.undecided(), 1);
                assert!(marked(&fresh, 0).is_empty());
            } else {
                assert_eq!(fresh.undecided(), 0);
                assert_eq!(marked(&fresh, 0), literal);
            }
            expected_steps += fresh.steps();
            if case.holders.len() > K {
                if previous == case.holders {
                    expected_steps -= (case.holders.len() * case.holders.len()) as u128;
                }
                previous = case.holders;
            }
        }
        assert_eq!(whole.steps(), expected_steps, "{field:?}");
        assert_eq!(whole.undecided(), 1);
        println!(
            "{field:?}: {} stateful blocks match fresh searches, literal outcomes and step accounting",
            cases.len()
        );
    }

    #[test]
    fn refitted_search_matches_fresh_searches() {
        check(Field::Gf256);
        check(Field::Gf65536);
    }
}
