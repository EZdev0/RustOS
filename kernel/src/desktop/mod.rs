pub mod app;
pub mod browser;
pub mod compositor;
pub mod filemanager;
pub mod renderer;
pub mod settings;
pub mod taskmanager;
pub mod updater;
pub mod notepad;
pub mod terminal;
pub mod window;

use core::sync::atomic::AtomicU8;
pub static THEME: AtomicU8 = AtomicU8::new(0); // 0 = Light, 1 = Dark
pub static THEME_CHANGE_REQUESTED: AtomicU8 = AtomicU8::new(0); // 1 = to light, 2 = to dark
