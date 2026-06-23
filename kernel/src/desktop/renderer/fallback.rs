/// # Safety
/// The caller must ensure that `src` and `dst` are valid pointers and `len` is correct.
pub unsafe fn blit_basic(src: *const u8, dst: *mut u8, len: usize) {
    core::ptr::copy_nonoverlapping(src, dst, len);
}
