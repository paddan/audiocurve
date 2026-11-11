use audiocurve_dsp::triple::TripleBuffer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn triple_latest_wins() {
    let t = TripleBuffer::new(0u32);
    assert_eq!(t.read(), 0);
    t.write(1);
    t.write(2);
    assert_eq!(t.read(), 2);
    assert_eq!(t.read(), 2);
}

#[test]
fn triple_concurrent_monotonic() {
    let t = Arc::new(TripleBuffer::new([0u64; 8]));
    let done = Arc::new(AtomicBool::new(false));
    let w = {
        let (t, done) = (t.clone(), done.clone());
        std::thread::spawn(move || {
            for i in 1..=100_000u64 {
                t.write([i; 8]);
            }
            done.store(true, Ordering::Release);
        })
    };
    let mut last = 0u64;
    let mut reads = 0u64;
    loop {
        let finished = done.load(Ordering::Acquire);
        let v = t.read();
        assert!(v.iter().all(|&x| x == v[0]), "torn read {v:?}");
        assert!(v[0] >= last, "decreased {} -> {}", last, v[0]);
        last = v[0];
        reads += 1;
        if finished {
            break;
        }
    }
    w.join().unwrap();
    assert_eq!(t.read()[0], 100_000);
    assert!(reads > 0);
}
