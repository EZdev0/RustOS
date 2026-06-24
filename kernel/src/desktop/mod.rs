pub mod compositor;
pub mod window;
pub mod terminal;
pub mod app;
pub mod notepad;
pub mod filemanager;
pub mod taskmanager;
pub mod settings;
pub mod renderer;
pub mod browser;

use core::sync::atomic::AtomicU8;
pub static THEME: AtomicU8 = AtomicU8::new(0); // 0 = Light, 1 = Dark
pub static THEME_CHANGE_REQUESTED: AtomicU8 = AtomicU8::new(0); // 1 = to light, 2 = to dark
