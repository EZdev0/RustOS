use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub const HEAP_SIZE: usize = 256 * 1024; // 256 KiB Heap

pub fn init_heap() {
    // Da wir noch kein vollwertiges Paging (Memory Management Unit) haben, 
    // weisen wir den Heap einfach statisch im .bss Segment zu.
    static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
    
    unsafe {
        ALLOCATOR.lock().init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}
