//! The known answers the crate's self-test and tests call "computed apart", recomputed with the
//! probe's own oracle (tables of logarithms, Gaussian elimination, long division), not the crate.

use aud006_codes_probe::gf::{Gf, cyclic_parity, evaluate, interpolate};

fn shards(gf: &Gf, xs: &[u16], d: &[u16], targets: &[u16]) -> Vec<u16> {
    let c = interpolate(gf, xs, d);
    targets.iter().map(|&t| evaluate(gf, &c, t)).collect()
}

fn report(label: &str, got: &[u16], want: &[u16]) {
    println!("{} {label}: oracle {:x?} stated {:x?}", if got == want { "AGREE " } else { aud006_codes_probe::fail(); "DIFFER" }, got, want);
}

fn main() {
    let g8 = Gf::new(8, 0x11D);
    let g11 = Gf::new(11, 0x805);
    // self-test
    report("GF(2^8) data 01 02 03 04 -> shards 4, 5", &shards(&g8, &[0, 1, 2, 3], &[1, 2, 3, 4], &[4, 5]), &[0x45, 0x5E]);
    report("growth data 46 91 23 -> shards 3, 4", &shards(&g8, &[0, 1, 2], &[0x46, 0x91, 0x23], &[3, 4]), &[0xF4, 0xF3]);
    report("growth shards 1,3,4 (91 F4 F3) -> 5, 6, 0", &shards(&g8, &[1, 3, 4], &[0x91, 0xF4, 0xF3], &[5, 6, 0]), &[0x24, 0x96, 0x46]);
    report("GF(2^11) 001 7FF 400 -> shards 3, 4", &shards(&g11, &[0, 1, 2], &[0x001, 0x7FF, 0x400], &[3, 4]), &[0x3FE, 0x7E3]);
    let g4 = Gf::new(4, 0x13);
    let g13 = Gf::new(13, 0x201B);
    report("GF(2^4) 1, F -> shards 2, 3", &shards(&g4, &[0, 1], &[1, 0xF], &[2, 3]), &[0xE, 0x0]);
    report("GF(2^13) 1, 1FFF -> shards 2, 3", &shards(&g13, &[0, 1], &[1, 0x1FFF], &[2, 3]), &[0x1FE6, 0x018]);
    // The card of 12 words and its four words at points 12..15 (self-test and plans.rs).
    let card: [u16; 12] = [0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453];
    report("GF(2^11) card -> points 12..15", &shards(&g11, &(0..12).collect::<Vec<_>>(), &card, &[12, 13, 14, 15]), &[0x143, 0x384, 0x313, 0x6DE]);
    // The word of seven with errors 01 02 03 at shards 0..2 (k = 3): no code word within two.
    let y = [1u16, 2, 3, 0, 0, 0, 0];
    let mut nearest = usize::MAX;
    for a in 0..7u16 {
        for b in (a + 1)..7 {
            for c in (b + 1)..7 {
                let xs = [a, b, c];
                let d: Vec<u16> = xs.iter().map(|&i| y[usize::from(i)]).collect();
                let w = shards(&g8, &xs, &d, &[0, 1, 2, 3, 4, 5, 6]);
                let dist = w.iter().zip(&y).filter(|(p, q)| p != q).count();
                nearest = nearest.min(dist);
            }
        }
    }
    println!("{} beyond the bound: nearest code word to 01 02 03 00 00 00 00 (k=3) is {nearest} symbols away (stated: none within 2)", if nearest > 2 { "AGREE " } else { aud006_codes_probe::fail(); "DIFFER" });
    // Syndromes of the reviewed words (correct.rs unit tests): zero word of k = 3 plus errors,
    // S_j = sum u_i x_i^j e_i over the points 0..m-1.
    let syn = |gf: &Gf, e: &[u16], k: usize| -> Vec<u16> {
        let m = e.len();
        (0..m - k)
            .map(|j| {
                (0..m).fold(0u16, |acc, i| {
                    let xi = i as u16;
                    let mut prod = 1u16;
                    for l in 0..m {
                        if l != i {
                            prod = gf.mul(prod, xi ^ l as u16);
                        }
                    }
                    let u = gf.inv(prod);
                    acc ^ gf.mul(gf.mul(u, gf.pow(xi, j as u64)), e[i])
                })
            })
            .collect()
    };
    report("syndromes GF(2^8) 01 0 0 0 0", &syn(&g8, &[1, 0, 0, 0, 0], 3), &[0x90, 0x00]);
    report("syndromes GF(2^8) 01 0 52 0 0 0 0", &syn(&g8, &[1, 0, 0x52, 0, 0, 0, 0], 3), &[0x00, 0x49, 0x92, 0x39]);
    report("syndromes GF(2^11) 001 0 0 0 0", &syn(&g11, &[1, 0, 0, 0, 0], 3), &[0x4FD, 0x000]);
    report("syndromes GF(2^11) 001 0 052 ...", &syn(&g11, &[1, 0, 0x52, 0, 0, 0, 0], 3), &[0x1B5, 0x11C, 0x238, 0x470]);
    report("syndromes GF(2^11) 001 0 2AA ...", &syn(&g11, &[1, 0, 0x2AA, 0, 0, 0, 0], 3), &[0x000, 0x276, 0x4EC, 0x1DD]);
    // Cyclic: QR 1-M example, GF(2^11) first root 5 step 3, GF(2^8) "shards" first root 1.
    let qr = [0x10u16, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11];
    report("QR 1-M parity (ISO/IEC 18004 annex I)", &cyclic_parity(&g8, &qr, 10, 0, 1).1, &[0xA5, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55]);
    let (g, p) = cyclic_parity(&g11, &[0x001, 0x7FF, 0x400, 0x123, 0x000], 4, 5, 3);
    report("GF(2^11) first 5 step 3 generator", &g, &[0x001, 0x69E, 0x2BE, 0x55A, 0x2EB]);
    report("GF(2^11) first 5 step 3 parity", &p, &[0x410, 0x680, 0x321, 0x134]);
    let s: Vec<u16> = b"shards".iter().map(|&b| u16::from(b)).collect();
    report("GF(2^8) first 1 \"shards\" parity", &cyclic_parity(&g8, &s, 4, 1, 1).1, &[0x7D, 0x8E, 0x96, 0x18]);
    // fields.rs SHARDS: data 1 and 2^m - 1 at points 0, 1 -> shards 2 and 3.
    let table: [(u32, u32, u16, u16); 17] = [
        (2, 0x7, 0x2, 0x0), (3, 0xB, 0x6, 0x0), (4, 0x13, 0xE, 0x0), (5, 0x25, 0x18, 0x6), (6, 0x43, 0x3E, 0x0),
        (7, 0x89, 0x74, 0xA), (8, 0x11D, 0xE0, 0x1E), (9, 0x211, 0x1EC, 0x12), (10, 0x409, 0x3F4, 0xA), (11, 0x805, 0x7F8, 0x6),
        (12, 0x1053, 0xFAE, 0x50), (13, 0x201B, 0x1FE6, 0x18), (14, 0x4443, 0x3BBE, 0x440), (15, 0x8003, 0x7FFE, 0x0),
        (16, 0x1100B, 0xEFF6, 0x1008), (8, 0x187, 0x7A, 0x84), (16, 0x1002D, 0xFFD0, 0x2E),
    ];
    let mut bad = 0;
    for (m, poly, two, three) in table {
        let gf = Gf::new(m, poly);
        let top = ((1u32 << m) - 1) as u16;
        if shards(&gf, &[0, 1], &[1, top], &[2, 3]) != [two, three] {
            bad += 1;
            println!("DIFFER fields.rs SHARDS {m} {poly:#x}");
        }
    }
    println!("{} fields.rs SHARDS: {bad} of 17 differ", if bad == 0 { "AGREE " } else { aud006_codes_probe::fail(); "DIFFER" });
    // field.rs PRODUCTS: (m, poly, a, b, a*b, 1/b).
    let products: [(u32, u32, u16, u16, u16, u16); 17] = [
        (2, 0x7, 0x3, 0x1, 0x3, 0x1), (3, 0xB, 0x7, 0x3, 0x2, 0x6), (4, 0x13, 0xF, 0x5, 0x6, 0xB), (5, 0x25, 0x1F, 0xB, 0x7, 0x10),
        (6, 0x43, 0x3F, 0x15, 0x27, 0x23), (7, 0x89, 0x7F, 0x2B, 0x41, 0x1A), (8, 0x11D, 0xFF, 0x55, 0x5E, 0x1A),
        (9, 0x211, 0x1FF, 0xAB, 0x1AC, 0x71), (10, 0x409, 0x3FF, 0x155, 0x196, 0x285), (11, 0x805, 0x7FF, 0x2AB, 0x664, 0x400),
        (12, 0x1053, 0xFFF, 0x555, 0xAB7, 0xF50), (13, 0x201B, 0x1FFF, 0xAAB, 0x655, 0x1361), (14, 0x4443, 0x3FFF, 0x1555, 0x2733, 0x655),
        (15, 0x8003, 0x7FFF, 0x2AAB, 0x3332, 0x6000), (16, 0x1100B, 0xFFFF, 0x5555, 0xF2E8, 0x19BC), (8, 0x187, 0xFF, 0x55, 0xDD, 0xD1),
        (16, 0x1002D, 0xFFFF, 0x5555, 0x33F7, 0xB96),
    ];
    let mut bad = 0;
    for (m, poly, a, b, ab, inv) in products {
        let gf = Gf::new(m, poly);
        if gf.mul(a, b) != ab || gf.inv(b) != inv {
            bad += 1;
            println!("DIFFER field.rs PRODUCTS {m} {poly:#x}: {:#x} {:#x}", gf.mul(a, b), gf.inv(b));
        }
    }
    println!("{} field.rs PRODUCTS: {bad} of 17 differ", if bad == 0 { "AGREE " } else { aud006_codes_probe::fail(); "DIFFER" });
    // gf2048.rs unit answers.
    report("gf2048 0x400*2, 0x7FF^2, 0x123*0x456, 1/0x123", &[g11.mul(0x400, 2), g11.mul(0x7FF, 0x7FF), g11.mul(0x123, 0x456), g11.inv(0x123)], &[0x005, 0x552, 0x2C8, 0x0A5]);
    // plans.rs: the byte example of seven shards of four bytes, k = 3, further ones computed apart.
    let set: [[u16; 4]; 7] = [
        [0x61, 0x62, 0x63, 0x64], [0x65, 0x66, 0x67, 0x68], [0x69, 0x6A, 0x6B, 0x6C], [0x6D, 0x6E, 0x6F, 0x60],
        [0x71, 0x72, 0x73, 0x34], [0x75, 0x76, 0x77, 0x38], [0x79, 0x7A, 0x7B, 0x3C],
    ];
    let mut bad = 0;
    for col in 0..4 {
        let d = [set[0][col], set[1][col], set[2][col]];
        let w = shards(&g8, &[0, 1, 2], &d, &[3, 4, 5, 6]);
        if w != [set[3][col], set[4][col], set[5][col], set[6][col]] {
            bad += 1;
        }
    }
    println!("{} plans.rs byte example: {bad} of 4 columns differ", if bad == 0 { "AGREE " } else { aud006_codes_probe::fail(); "DIFFER" });
    aud006_codes_probe::finish();
}
