// Temporary diagnostic allocator only; removed from shell/src/lib.rs after use.
// Retained bytes are live Rust allocation layout sizes, not malloc usable size,
// OS RSS, or allocator fragmentation. Run one exact test in its own process.
#[cfg(test)]
pub(crate) mod backlog_search_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct CountingAlloc;
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    unsafe impl GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() { LIVE.fetch_add(layout.size(), Ordering::Relaxed); }
            ptr
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static ALLOC: CountingAlloc = CountingAlloc;
    pub(crate) fn live() -> usize { LIVE.load(Ordering::Relaxed) }
}
