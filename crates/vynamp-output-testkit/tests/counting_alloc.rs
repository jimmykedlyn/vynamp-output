use vynamp_output_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

#[test]
fn counts_an_allocation_on_this_thread() {
    let before = allocations_on_this_thread();
    let v: Vec<u8> = Vec::with_capacity(1024);
    std::hint::black_box(&v);
    assert_eq!(allocations_on_this_thread(), before + 1);
}

#[test]
fn no_allocation_is_counted_without_allocating() {
    let before = allocations_on_this_thread();
    let x = std::hint::black_box(1 + 1);
    assert_eq!(x, 2);
    assert_eq!(allocations_on_this_thread(), before);
}

#[test]
fn allocations_on_another_thread_are_not_counted_here() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static GO: AtomicBool = AtomicBool::new(false);
    static DONE: AtomicBool = AtomicBool::new(false);

    let other = std::thread::spawn(|| {
        while !GO.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let before = allocations_on_this_thread();
        let v: Vec<u8> = Vec::with_capacity(4096);
        std::hint::black_box(&v);
        let n = allocations_on_this_thread() - before;
        DONE.store(true, Ordering::Release);
        n
    });

    // Spawning is finished; from here this thread only spins on atomics.
    let before = allocations_on_this_thread();
    GO.store(true, Ordering::Release);
    while !DONE.load(Ordering::Acquire) {
        std::thread::yield_now();
    }
    assert_eq!(allocations_on_this_thread(), before);
    assert_eq!(other.join().unwrap(), 1);
}
