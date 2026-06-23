pub mod fallback;

pub struct IntelligentRenderer {
    blitter: unsafe fn(*const u8, *mut u8, usize),
}

impl IntelligentRenderer {
    pub fn init(_framebuffer_base: *mut u8) -> Self {
        // Fallback for now due to x86_64-unknown-none disabling SSE/AVX registers
        let blitter = fallback::blit_basic;

        Self {
            blitter,
        }
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn draw_dirty_rect(&self, src: *const u8, dst: *mut u8, size_bytes: usize) {
        unsafe {
            (self.blitter)(src, dst, size_bytes);
        }
    }
}
