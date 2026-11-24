//! An implementation of GF(2^m) by tables of logarithms, written apart from reed-solomon-core,
//! with Gaussian elimination, polynomial division and brute-force decoding as oracles.

pub mod gf {
    /// GF(2^m) by a polynomial with its x^m term, tables of powers of x (assumed primitive).
    #[derive(Clone)]
    pub struct Gf {
        pub m: u32,
        pub poly: u32,
        pub q: usize,
        pub exp: Vec<u16>,
        pub log: Vec<u32>,
    }

    impl Gf {
        pub fn new(m: u32, poly: u32) -> Self {
            let q = 1usize << m;
            let mut exp = vec![0u16; 2 * q];
            let mut log = vec![u32::MAX; q];
            let mut v: u32 = 1;
            for i in 0..(q - 1) {
                exp[i] = v as u16;
                assert!(log[v as usize] == u32::MAX, "x is not primitive for {poly:#x}");
                log[v as usize] = i as u32;
                v <<= 1;
                if v & (1 << m) != 0 {
                    v ^= poly;
                }
            }
            assert_eq!(v, 1, "x^(q-1) != 1");
            for i in (q - 1)..(2 * q) {
                exp[i] = exp[i - (q - 1)];
            }
            Gf { m, poly, q, exp, log }
        }
        pub fn mul(&self, a: u16, b: u16) -> u16 {
            if a == 0 || b == 0 {
                return 0;
            }
            self.exp[(self.log[a as usize] + self.log[b as usize]) as usize]
        }
        pub fn inv(&self, a: u16) -> u16 {
            assert!(a != 0);
            self.exp[(self.q - 1 - self.log[a as usize] as usize) % (self.q - 1)]
        }
        pub fn pow(&self, a: u16, e: u64) -> u16 {
            if e == 0 {
                return 1;
            }
            if a == 0 {
                return 0;
            }
            let l = (self.log[a as usize] as u64 * e) % (self.q as u64 - 1);
            self.exp[l as usize]
        }
        /// alpha^e for alpha = x.
        pub fn alpha(&self, e: u64) -> u16 {
            self.exp[(e % (self.q as u64 - 1)) as usize]
        }
    }

    /// Slow carry-less product reduced by long division: a third route for spot checks.
    pub fn slow_mul(m: u32, poly: u32, a: u16, b: u16) -> u16 {
        let mut wide: u64 = 0;
        for bit in 0..m {
            if (b >> bit) & 1 == 1 {
                wide ^= (a as u64) << bit;
            }
        }
        for bit in (m..(2 * m)).rev() {
            if (wide >> bit) & 1 == 1 {
                wide ^= (poly as u64) << (bit - m);
            }
        }
        wide as u16
    }

    /// Whether `poly` (degree m with its x^m term) is primitive, by factoring 2^m - 1 and checking
    /// x^((2^m-1)/p) != 1 for each prime p, after x^(2^m-1) = 1 and irreducibility by Rabin's
    /// test. Written apart from the crate's order walk.
    pub fn is_primitive(m: u32, poly: u32) -> bool {
        if poly >> m != 1 || poly & 1 == 0 {
            return false;
        }
        // Polynomial arithmetic mod poly over GF(2), u64 bit vectors.
        let mulmod = |a: u64, b: u64| -> u64 {
            let mut r: u64 = 0;
            let mut a = a;
            let mut b = b;
            while b != 0 {
                if b & 1 == 1 {
                    r ^= a;
                }
                b >>= 1;
                a <<= 1;
                if (a >> m) & 1 == 1 {
                    a ^= poly as u64;
                }
            }
            r
        };
        let powmod = |base: u64, mut e: u64| -> u64 {
            let mut r: u64 = 1;
            let mut b = base;
            while e > 0 {
                if e & 1 == 1 {
                    r = mulmod(r, b);
                }
                b = mulmod(b, b);
                e >>= 1;
            }
            r
        };
        let n: u64 = (1u64 << m) - 1;
        if powmod(2, n) != 1 {
            return false;
        }
        // prime factors of n
        let mut primes = Vec::new();
        let mut t = n;
        let mut p = 2;
        while p * p <= t {
            if t % p == 0 {
                primes.push(p);
                while t % p == 0 {
                    t /= p;
                }
            }
            p += 1;
        }
        if t > 1 {
            primes.push(t);
        }
        // x^(n/p) != 1 for every prime p of n means x has order n; with x^n = 1 and a ring of
        // 2^m elements, that forces an irreducible poly and x primitive.
        primes.iter().all(|&p| powmod(2, n / p) != 1)
    }

    /// Solve V a = d for the Vandermonde system at points xs (Gaussian elimination), return the
    /// coefficients of the polynomial of degree < k through (xs[i], d[i]).
    pub fn interpolate(f: &Gf, xs: &[u16], d: &[u16]) -> Vec<u16> {
        let k = xs.len();
        let mut a: Vec<Vec<u16>> = (0..k)
            .map(|i| {
                let mut row: Vec<u16> = (0..k).map(|j| f.pow(xs[i], j as u64)).collect();
                row.push(d[i]);
                row
            })
            .collect();
        for col in 0..k {
            let piv = (col..k).find(|&r| a[r][col] != 0).expect("singular");
            a.swap(col, piv);
            let inv = f.inv(a[col][col]);
            for j in col..=k {
                a[col][j] = f.mul(a[col][j], inv);
            }
            for r in 0..k {
                if r != col && a[r][col] != 0 {
                    let factor = a[r][col];
                    for j in col..=k {
                        let v = f.mul(factor, a[col][j]);
                        a[r][j] ^= v;
                    }
                }
            }
        }
        (0..k).map(|i| a[i][k]).collect()
    }

    pub fn evaluate(f: &Gf, coeffs: &[u16], x: u16) -> u16 {
        coeffs.iter().rev().fold(0, |acc, &c| f.mul(acc, x) ^ c)
    }

    /// Systematic cyclic parity by long division: data first word highest power; roots
    /// beta^(first + i), beta = alpha^step. Returns (generator high-first, parity high-first).
    pub fn cyclic_parity(f: &Gf, data: &[u16], parity: usize, first: u64, step: u64) -> (Vec<u16>, Vec<u16>) {
        // generator, lowest coefficient first
        let mut g: Vec<u16> = vec![1];
        for i in 0..parity as u64 {
            let root = f.alpha(step * (first + i));
            let mut next = vec![0u16; g.len() + 1];
            for (j, &c) in g.iter().enumerate() {
                next[j + 1] ^= c; // x * c
                next[j] ^= f.mul(root, c); // root * c (char 2: + = -)
            }
            g = next;
        }
        // message times x^parity, highest first
        let mut rem: Vec<u16> = data.to_vec();
        rem.extend(std::iter::repeat(0).take(parity));
        let g_high: Vec<u16> = g.iter().rev().copied().collect(); // g_high[0] = 1
        for i in 0..data.len() {
            let lead = rem[i];
            if lead != 0 {
                for j in 0..g_high.len() {
                    let v = f.mul(lead, g_high[j]);
                    rem[i + j] ^= v;
                }
            }
        }
        (g_high, rem[data.len()..].to_vec())
    }
}

/// Whether a check of this run failed; [`finish`] then ends it with exit code 1.
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Records that a check failed.
pub fn fail() {
    FAILED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Ends the run with exit code 1 if any check failed.
pub fn finish() {
    if FAILED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(1);
    }
}
