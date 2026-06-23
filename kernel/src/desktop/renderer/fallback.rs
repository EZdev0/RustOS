pub unsafe fn blit_basic(src: *const u8, dst: *mut u8, len: usize) {
    core::ptr::copy_nonoverlapping(src, dst, len);
}
