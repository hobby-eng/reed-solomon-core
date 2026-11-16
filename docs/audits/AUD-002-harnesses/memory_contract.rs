//! A normal public-input contract check for the protected and portable profiles.
use reed_solomon_core::{Change, Code, Field};

fn main() {
    let portable = std::env::args().nth(1).as_deref() == Some("alloc");
    let sample = [b"ABCD".as_slice(), b"EFGH".as_slice()];
    let shards = Code::new(Field::Gf256, 2)
        .expect("valid parameters")
        .encode(&sample, &[0, 1])
        .expect("public systematic encoding");
    for (shard, original) in shards.iter().zip(sample) {
        assert_eq!(shard.as_slice(), original);
        let flags = (
            shard.is_locked(),
            shard.is_out_of_dumps(),
            shard.is_out_of_children(),
        );
        if portable {
            assert_eq!(flags, (false, false, false));
        }
        println!(
            "locked={} outside_dumps={} outside_children={}",
            flags.0, flags.1, flags.2
        );
    }
    fn requires_copy<T: Copy>() {}
    requires_copy::<Change>();
    println!("Public correction records implement Copy.");
}
