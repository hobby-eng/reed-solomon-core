//! The README's example of the strict mode, as a test.

#[test]
fn a_card_held_in_protected_memory_is_put_right_in_the_strict_mode()
-> Result<(), Box<dyn std::error::Error>> {
    use reed_solomon_core::protect::Secret;
    use reed_solomon_core::strict::{self, CyclicCode, Held, HeldMut};
    use reed_solomon_core::{Field, Mark, Plan};

    // The system may protect less than it is asked to: memory is checked before a byte goes in.
    let protected = |length| match Secret::with_capacity(length) {
        secret if Held::guarded(&secret) => Ok(secret),
        _ => Err("the system does not protect this memory"),
    };
    // Each word of a card in protected memory of its own, as a program reads it in; the numbers
    // stand in the open here only to keep the example short.
    let code = CyclicCode::new(Field::Gf2048, 12, 4, 1, 1)?;
    let card: [u16; 12] = [
        0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
    ];
    let mut words = Vec::new();
    for word in card {
        let mut held = protected(2)?;
        held.push(&word.to_be_bytes())?;
        words.push(held);
    }
    let data: Vec<&dyn Held> = words.iter().map(|word| word as &dyn Held).collect();
    let mut repair = code.encode(&data)?;
    assert!(repair.iter().all(|word| word.is_locked()));

    // Word 3 is smudged and marked. The plan keeps what it read and what it writes in protected
    // pages and fits only these words; `strict::values` hands a value over on the protected
    // stack, here straight into protected memory of the program's.
    words[2].as_mut_bytes().fill(0);
    let mut read: Vec<&dyn Held> = words.iter().map(|word| word as &dyn Held).collect();
    read.extend(repair.iter().map(|word| word as &dyn Held));
    let Plan::Changes(changes) = code.plan(
        &read,
        &[],
        &[Mark {
            shard: 2,
            symbol: 0,
        }],
    )?
    else {
        return Err("the card is within 2e + s ≤ 4".into());
    };
    let mut shown = protected(2)?;
    let pushed = strict::values(&changes, 0, |_, becomes| shown.push(&becomes.to_be_bytes()))?;
    assert!(matches!(pushed, Some(Ok(()))) && shown.as_bytes() == 0x220_u16.to_be_bytes());
    let mut shards: Vec<&mut dyn HeldMut> =
        words.iter_mut().map(|w| w as &mut dyn HeldMut).collect();
    shards.extend(repair.iter_mut().map(|w| w as &mut dyn HeldMut));
    code.apply(&mut shards, &changes)?;
    assert_eq!(words[2].as_bytes(), 0x220_u16.to_be_bytes());
    Ok(())
}
