use linked_list_allocator::LockedHeap;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::alloc::{GlobalAlloc, Layout};

pub struct TrackingAllocator {
    inner: LockedHeap,
    allocated: AtomicUsize,
    freed: AtomicUsize,
}

impl TrackingAllocator {
    pub const fn empty() -> Self {
        Self {
            inner: LockedHeap::empty(),
            allocated: AtomicUsize::new(0),
            freed: AtomicUsize::new(0),
        }
    }

    pub fn get_memory_usage(&self) -> (usize, usize) {
        let alloc = self.allocated.load(Ordering::Relaxed);
        let free = self.freed.load(Ordering::Relaxed);
        let used = alloc.saturating_sub(free);
        (used, HEAP_SIZE)
    }

    /// # Safety
    /// The caller must ensure that `heap_bottom` and `heap_size` are valid and the memory is unused.
    pub unsafe fn init(&self, heap_bottom: *mut u8, heap_size: usize) {
        self.inner.lock().init(heap_bottom, heap_size);
    }
}

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = self.inner.alloc(layout);
        if !ptr.is_null() {
            self.allocated.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.dealloc(ptr, layout);
        self.freed.fetch_add(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
pub static ALLOCATOR: TrackingAllocator = TrackingAllocator::empty();

pub const HEAP_SIZE: usize = 16 * 1024 * 1024; // 16 MiB Heap

pub fn init_heap() {
    // Da wir noch kein vollwertiges Paging (Memory Management Unit) haben, 
    // weisen wir den Heap einfach statisch im .bss Segment zu.
    static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
    
    unsafe {
        ALLOCATOR.init(core::ptr::addr_of_mut!(HEAP) as *mut u8, HEAP_SIZE);
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}
