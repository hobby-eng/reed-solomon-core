//! AUD-006 probe: one copy of secret-protection-core shared by a program and reed-solomon-core.
use reed_solomon_core::Field;
use reed_solomon_core::strict::{self, Held};
use secret_protection_core::{Isolation, Protection, Secret, WorkArea};
use std::any::TypeId;

fn main() {
    let same_secret = TypeId::of::<Secret>() == TypeId::of::<reed_solomon_core::protect::Secret>();
    let same_area = TypeId::of::<WorkArea>() == TypeId::of::<reed_solomon_core::protect::WorkArea>();
    println!("same Secret type: {same_secret}, same WorkArea type: {same_area}");
    let mut ok = same_secret && same_area;
    let isolate = std::env::args().any(|arg| arg == "--isolate");
    if isolate {
        // The program enters isolation once, through its own dependency.
        let isolation = Isolation::enter();
        for protection in [
            Protection::NoCoreDumps,
            Protection::NoNewSockets,
            Protection::NoFileChanges,
            Protection::NoNewPrograms,
            Protection::WholeProcess,
        ] {
            println!("isolation {protection:?}: {:?}", isolation.outcome(protection));
        }
    }
    // Secrets of the program's own copy, given to a strict call of reed-solomon-core: this
    // compiles only when both name the same crate.
    let mut east = Secret::with_capacity(4);
    let mut west = Secret::with_capacity(4);
    let pushed = east.push(b"east").and_then(|()| west.push(b"west"));
    println!("pushed: {pushed:?}; guarded: {} {}", Held::guarded(&east), Held::guarded(&west));
    let code = strict::Code::new(Field::Gf256, 2).expect("code");
    let data: [&dyn Held; 2] = [&east, &west];
    match code.encode(&data, &[2]) {
        Ok(parity) => println!(
            "strict encode{}: ok, parity locked={} out_of_dumps={} out_of_children={}",
            if isolate { " after isolation" } else { "" },
            parity[0].is_locked(),
            parity[0].is_out_of_dumps(),
            parity[0].is_out_of_children()
        ),
        Err(error) => {
            ok = false;
            println!("strict encode: refused: {error:?}");
        }
    }
    if !ok {
        std::process::exit(1);
    }
}
