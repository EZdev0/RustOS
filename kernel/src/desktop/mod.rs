pub mod app;
pub mod browser;
pub mod compositor;
pub mod filemanager;
pub mod notepad;
pub mod renderer;
pub mod settings;
pub mod taskmanager;
pub mod terminal;
pub mod updater;
pub mod window;

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub static THEME: AtomicU8 = AtomicU8::new(0); // 0 = Light, 1 = Dark
pub static THEME_CHANGE_REQUESTED: AtomicU8 = AtomicU8::new(0); // 1 = to light, 2 = to dark

/// Set whenever *something* on screen changed (input, network, animation).
///
/// The timer task renders at 60 Hz, but only forwards frames when this flag is
/// set (or an animation is running, or the 2 Hz housekeeping tick is due). An
/// idle desktop therefore costs ~0 % CPU instead of a full-screen composite 60
/// times per second.
static REPAINT_REQUESTED: AtomicBool = AtomicBool::new(true);

/// True when the dark theme is active.
pub fn is_dark() -> bool {
    THEME.load(Ordering::Relaxed) == 1
}

/// Request a frame. Callable from tasks, input handlers and interrupt context.
#[inline]
pub fn request_repaint() {
    REPAINT_REQUESTED.store(true, Ordering::Relaxed);
}

/// Consume a pending repaint request (`true` = a frame is due).
#[inline]
pub fn take_repaint_request() -> bool {
    REPAINT_REQUESTED.swap(false, Ordering::Relaxed)
}

/// True when a frame is already queued.
#[inline]
pub fn repaint_pending() -> bool {
    REPAINT_REQUESTED.load(Ordering::Relaxed)
}
