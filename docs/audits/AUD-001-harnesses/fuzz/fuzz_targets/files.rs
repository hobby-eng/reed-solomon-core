//! Any bytes as shard files. Nothing here may panic, stop the program or run without end: a file
//! that does not read is an answer, and so is a set that does not rebuild.
#![no_main]

use libfuzzer_sys::fuzz_target;
use reed_solomon_core::{Block, Pile, ShardFile};

/// The steps a call may take: enough for every set a few kilobytes can hold.
const LIMIT: Option<u128> = Some(1 << 22);

fuzz_target!(|bytes: &[u8]| {
    // The first byte says how many files the rest is cut into, at most four.
    let Some((&count, rest)) = bytes.split_first() else {
        return;
    };
    let count = usize::from(count % 4) + 1;
    let files: Vec<&[u8]> = rest.chunks(rest.len().div_ceil(count).max(1)).collect();
    for file in &files {
        if let Ok(read) = ShardFile::read(file) {
            let survey = read.survey();
            let blocks = read.header().set().blocks();
            assert!(survey.intact + survey.damaged + survey.missing == blocks);
            // The blocks before the first missing one, as the survey counts them.
            let present = u32::try_from(survey.intact + survey.damaged).unwrap_or(u32::MAX);
            let intact = (0..present)
                .map_while(|number| read.block(number))
                .filter(|block| matches!(block, Block::Intact(_)))
                .count();
            assert_eq!(u64::try_from(intact), Ok(survey.intact));
        }
    }
    let Ok(pile) = Pile::new(&files) else {
        return;
    };
    assert_eq!(pile.readings().len(), files.len());
    for set in pile.sets() {
        let _ = (set.coverage(), set.members(), set.disputes(), set.id_shared());
        let _ = set.rebuild_work();
        if let Ok(rebuilt) = set.rebuild(LIMIT) {
            assert_eq!(u64::try_from(rebuilt.data.len()), Ok(set.facts().data_len()));
        }
        if let Ok(vote) = set.vote(LIMIT) {
            let _ = (vote.outvoted(), vote.undecided(), vote.first_undecided());
        }
        let _ = set.repair(0, LIMIT);
        let _ = set.repairs(&[0, 1, 1], LIMIT);
        if let Ok(left) = set.without(&[0]) {
            let _ = left.rebuild(LIMIT);
        }
    }
});
