//! Several lost files of a set made in one pass: each the file `repair` makes, in the order asked,
//! for steps counted once per block.

use crate::sets::{one_set, slices, written};
use crate::support::plain;
use reed_solomon_core::{Field, Pile, SetError};

#[test]
fn three_lost_files_come_back_in_one_pass_in_the_order_asked() {
    // k = 3, seven files, shards of eight bytes in two blocks of four symbols.
    let set = written(Field::Gf256, 21, 3, 7, 24);
    let left: Vec<Vec<u8>> = [0, 2, 5, 6].iter().map(|&i| set.files[i].clone()).collect();
    let pile = Pile::new(&slices(&left)).unwrap();
    let found = one_set(&pile);
    let made = found.repairs(&[4, 1, 3], None).unwrap();
    assert_eq!(
        plain(&made),
        [
            set.files[4].clone(),
            set.files[1].clone(),
            set.files[3].clone()
        ]
    );
    for (&index, file) in [4_u16, 1, 3].iter().zip(&made) {
        assert_eq!(&found.repair(index, None).unwrap(), file);
    }
    // Sources prepared once, k² = 9; per block one spare holder compared and three shards
    // made, each k · (s + 64) = 204.
    assert_eq!(found.repairs_work(&[4, 1, 3]), Ok(9 + 2 * 4 * 204));
    let apart: u128 = [4, 1, 3]
        .iter()
        .map(|&i| found.repair_work(i).unwrap())
        .sum();
    assert_eq!(apart, 3 * (9 + 2 * 2 * 204));
}

#[test]
fn a_number_asked_twice_gives_the_file_twice_and_no_number_costs_nothing() {
    let set = written(Field::Gf65536, 22, 2, 5, 40);
    let left: Vec<Vec<u8>> = [1, 3, 4].iter().map(|&i| set.files[i].clone()).collect();
    let pile = Pile::new(&slices(&left)).unwrap();
    let found = one_set(&pile);
    let made = found.repairs(&[0, 0, 7], None).unwrap();
    assert_eq!(*made[0], set.files[0]);
    assert_eq!(*made[1], set.files[0]);
    assert_eq!(made[2], found.repair(7, None).unwrap());
    assert_eq!(found.repairs_work(&[0, 7, 0]), found.repairs_work(&[0, 7]));
    assert_eq!(found.repairs(&[], None), Ok(Vec::new()));
    assert_eq!(found.repairs_work(&[]), Ok(0));
}

#[test]
fn a_number_the_field_lacks_and_the_limit_are_checked_before_any_work() {
    let set = written(Field::Gf256, 23, 3, 7, 24);
    let left: Vec<Vec<u8>> = [0, 2, 5, 6].iter().map(|&i| set.files[i].clone()).collect();
    let pile = Pile::new(&slices(&left)).unwrap();
    let found = one_set(&pile);
    assert_eq!(found.repairs(&[1, 300], None), Err(SetError::Index));
    assert_eq!(found.repairs_work(&[300]), Err(SetError::Index));
    let needed = 9 + 2 * 4 * 204;
    assert_eq!(
        found.repairs(&[4, 1, 3], Some(needed - 1)),
        Err(SetError::Work { needed })
    );
    assert!(found.repairs(&[4, 1, 3], Some(needed)).is_ok());
    // Too few files: the number is still checked first.
    let few = Pile::new(&slices(&left[..2])).unwrap();
    let short = one_set(&few);
    assert_eq!(short.repairs(&[300], None), Err(SetError::Index));
    assert!(matches!(
        short.repairs(&[1], None),
        Err(SetError::Shards { .. })
    ));
}
