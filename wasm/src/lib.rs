//! The library on wasm32-unknown-unknown, the browser build, which `run.mjs` runs in Node: the
//! known answers of the self-test, a set written and rebuilt with its buffers in the allocator's
//! memory, and the strict mode's refusal, as a browser gives no protected stack.

use reed_solomon_core::{Buffer, Code, CodeError, Field, protect, self_test, strict};

/// How many known answers the self-test ran, when every one held and none was left out; else 0.
/// `run.mjs` asks for exactly 17: the fields, the codes and the shard file give what independent
/// implementations give.
#[expect(
    unsafe_code,
    reason = "Node finds the export by its name, which `no_mangle` keeps"
)]
#[unsafe(no_mangle)]
pub extern "C" fn known_answers() -> u32 {
    let report = self_test();
    let total = report.checks().len();
    let passed = report.checks().iter().filter(|check| check.passed).count();
    match report.passed() && passed == total {
        true => u32::try_from(total).unwrap_or(0),
        false => 0,
    }
}

/// Five checks of a set of three shards, two of them data: the parity shard, the data rebuilt
/// from shards 1 and 2, a wrong shard refused, and buffers that report no protection, as the
/// browser gives none.
#[expect(
    unsafe_code,
    reason = "Node finds the export by its name, which `no_mangle` keeps"
)]
#[unsafe(no_mangle)]
pub extern "C" fn codes() -> u32 {
    let data: [&[u8]; 2] = [&[0x46, 0x91, 0x23, 0x00], &[0x24, 0x42, 0x10, 0xFF]];
    let Ok(code) = Code::new(Field::Gf256, 2) else {
        return 0;
    };
    let Ok(parity) = code.encode(&data, &[2]) else {
        return 0;
    };
    let Some(parity) = parity.first() else {
        return 0;
    };
    let mut passed = u32::from(parity.len() == 4);
    let Some(second) = data.get(1) else {
        return 0;
    };
    let rebuilt = code.rebuild(&[(1, second), (2, parity.as_slice())], &[0]);
    passed += u32::from(
        rebuilt.is_ok_and(|shards| shards.first().map(Buffer::as_slice) == data.first().copied()),
    );
    let short = parity.as_slice().get(..2).unwrap_or_default();
    passed += u32::from(matches!(
        code.rebuild(&[(1, second), (2, short)], &[0]),
        Err(CodeError::Length | CodeError::Buffer)
    ));
    passed += u32::from(!parity.is_locked() && !parity.is_out_of_dumps());
    passed += u32::from(!parity.is_out_of_children());
    passed
}

/// Three checks of the strict mode, which a browser cannot give its protected stack: a work area
/// and an empty buffer are refused as unprotected, and the empty buffer counts as guarded.
#[expect(
    unsafe_code,
    reason = "Node finds the export by its name, which `no_mangle` keeps"
)]
#[unsafe(no_mangle)]
pub extern "C" fn strict() -> u32 {
    let Ok(code) = strict::Code::new(Field::Gf256, 1) else {
        return 0;
    };
    let Some(secret) = protect::WorkArea::new(1) else {
        return 0;
    };
    let empty = Buffer::default();
    let mut passed = u32::from(matches!(
        code.encode(&[&secret], &[0]),
        Err(CodeError::Unprotected)
    ));
    passed += u32::from(strict::Held::guarded(&empty));
    passed += u32::from(matches!(
        code.encode(&[&empty], &[0]),
        Err(CodeError::Unprotected)
    ));
    passed
}
