//! Public-data native-contract checks for the wasm32-unknown-unknown interpreter.

use reed_solomon_core::{Buffer, Code, CodeError, Field, protect, self_test, strict};

/// The number of successful checks. The interpreter must report 24.
#[expect(
    unsafe_code,
    reason = "the interpreter resolves this unique export by symbol name"
)]
#[unsafe(no_mangle)]
pub extern "C" fn check_public_contracts() -> u32 {
    let report = self_test();
    let Ok(count) = u32::try_from(report.checks().len()) else {
        return 0;
    };
    if !report.passed() {
        return 0;
    }

    let Ok(code) = Code::new(Field::Gf256, 1) else {
        return 0;
    };
    let Ok(shards) = code.encode(&[&[0x24, 0x42]], &[0]) else {
        return 0;
    };
    let Some(output) = shards.first() else {
        return 0;
    };
    let mut extra = u32::from(output.as_slice() == [0x24, 0x42]);
    extra += u32::from(!output.is_locked());
    extra += u32::from(!output.is_out_of_dumps());
    extra += u32::from(!output.is_out_of_children());

    let Ok(code) = strict::Code::new(Field::Gf256, 1) else {
        return 0;
    };
    let Some(secret) = protect::WorkArea::new(1) else {
        return 0;
    };
    extra += u32::from(matches!(
        code.encode(&[&secret], &[0]),
        Err(CodeError::Unprotected)
    ));
    let empty = Buffer::default();
    extra += u32::from(strict::Held::guarded(&empty));
    extra += u32::from(matches!(
        code.encode(&[&empty], &[0]),
        Err(CodeError::Unprotected)
    ));
    count + extra
}
