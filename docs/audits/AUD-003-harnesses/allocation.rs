use reed_solomon_core::{Code, CodeError, DataDigest, Field, FileError, SetFacts, self_test};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};

struct Gate;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static CALL: AtomicUsize = AtomicUsize::new(0);
static DENY: AtomicUsize = AtomicUsize::new(0);
fn denied() -> bool {
    ACTIVE.load(SeqCst) && CALL.fetch_add(1, SeqCst) + 1 == DENY.load(SeqCst)
}
unsafe impl GlobalAlloc for Gate {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if denied() {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc(l) }
        }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        if denied() {
            std::ptr::null_mut()
        } else {
            unsafe { System.alloc_zeroed(l) }
        }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if denied() {
            std::ptr::null_mut()
        } else {
            unsafe { System.realloc(p, l, n) }
        }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOC: Gate = Gate;
fn measure<T>(ordinal: usize, op: impl FnOnce() -> T) -> (T, usize) {
    CALL.store(0, SeqCst);
    DENY.store(ordinal, SeqCst);
    ACTIVE.store(true, SeqCst);
    let result = op();
    ACTIVE.store(false, SeqCst);
    (result, CALL.load(SeqCst))
}
fn check<T>(name: &str, mut op: impl FnMut() -> Result<T, CodeError>) {
    let (ok, count) = measure(0, &mut op);
    assert!(ok.is_ok() && count > 0);
    for n in 1..=count {
        let (result, _) = measure(n, &mut op);
        assert!(
            matches!(result, Err(CodeError::Memory)),
            "{name} allocation {n}"
        );
    }
    println!("{name}: {count} allocation points");
}
fn main() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let data: [&[u8]; 2] = [b"ab", b"cd"];
    let original = code.encode(&data, &[0, 1, 2, 3]).unwrap();
    let mut damaged = [0u8; 2];
    damaged.copy_from_slice(&original[0]);
    damaged[0] ^= 1;
    let input = [
        (0, damaged.as_slice()),
        (1, &original[1]),
        (2, &original[2]),
        (3, &original[3]),
    ];
    check("encode", || code.encode(&data, &[2, 3]));
    check("rebuild", || code.rebuild(&input[1..3], &[0, 3]));
    check("redundancy", || code.redundancy(&input));
    check("plan", || code.plan(&input, &[]));
    let make = || {
        let mut v = [[0; 2]; 4];
        for (i, (_, s)) in input.iter().enumerate() {
            v[i].copy_from_slice(s);
        }
        v
    };
    let correct = |v: &mut [[u8; 2]; 4]| {
        let [a, b, c, d] = v;
        code.correct(&mut [(0, a), (1, b), (2, c), (3, d)])
    };
    let mut initial = make();
    let (ok, n) = measure(0, || correct(&mut initial));
    assert!(ok.is_ok() && n > 0);
    for k in 1..=n {
        let mut v = make();
        let (result, _) = measure(k, || correct(&mut v));
        assert!(matches!(result, Err(CodeError::Memory)));
        assert_eq!(v, make());
    }
    println!("correct: {n} allocation points; no partial writes");
    let facts = SetFacts::for_data(Field::Gf256, b"test", 2, b"abcd", 2, DataDigest::None).unwrap();
    let len = facts.file_len() as usize;
    let mut a = vec![0x5a; len];
    let mut b = a.clone();
    let writer = |a: &mut [u8], b: &mut [u8]| facts.files_into(b"abcd", &[2, 3], &mut [a, b]);
    let (res, n) = measure(0, || writer(&mut a, &mut b));
    assert!(res.is_ok() && n > 0);
    for i in 1..=n {
        a.fill(0x5a);
        b.fill(0x5a);
        let (res, _) = measure(i, || writer(&mut a, &mut b));
        assert!(matches!(res, Err(FileError::Memory)));
        assert!(a.iter().chain(&b).all(|v| *v == 0x5a) || a.iter().chain(&b).all(|v| *v == 0));
    }
    println!("files_into: {n} allocation points; outputs preserved or cleared");
    let files = facts.files(b"abcd", &[0, 1, 2, 3]).unwrap();
    let slices: Vec<_> = files.iter().map(|f| f.as_slice()).collect();
    let pile = reed_solomon_core::Pile::new(&slices).unwrap();
    let set = &pile.sets()[0];
    let repair = |a: &mut [u8], b: &mut [u8]| set.repairs_into(&[2, 3], &mut [a, b], None);
    let (ok, n) = measure(0, || repair(&mut a, &mut b));
    assert!(ok.is_ok() && n > 0);
    for k in 1..=n {
        a.fill(0x5a);
        b.fill(0x5a);
        let (res, _) = measure(k, || repair(&mut a, &mut b));
        assert!(res.is_err(), "repair allocation {k}");
        assert!(a.iter().chain(&b).all(|v| *v == 0x5a) || a.iter().chain(&b).all(|v| *v == 0));
    }
    println!("repairs_into: {n} allocation points; outputs preserved or cleared");
    if std::env::args().nth(1).as_deref() == Some("--io") {
        return;
    }
    let (ok, n) = measure(0, self_test);
    assert!(ok.passed() && n > 0);
    for k in 1..=n {
        let (report, _) = measure(k, self_test);
        assert!(!report.passed(), "self_test allocation {k}");
    }
    println!("self_test: {n} allocation points; negative status returned");
}
