#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod allocator;
pub mod desktop;
pub mod hardware;
pub mod interrupts;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use crate::desktop::app::App;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };

    // Initialize the dynamic Heap Allocator
    allocator::init_heap();

    // Detect CPU features and initialize SIMD/AVX
    let cpu_features = hardware::cpuid::detect_and_init();

    // Setup global compositor
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let mut compositor = desktop::compositor::GraphicalCompositor::new(framebuffer);
        
        let width = compositor.info().width;
        let height = compositor.info().height;
        let win_width = if width > 600 { width - 200 } else { width - 40 };
        let win_height = if height > 400 { height - 150 } else { height - 80 };
        let win_x = (width - win_width) / 2;
        let win_y = (height - win_height) / 2 - 20;

        let mut notepad_app = desktop::notepad::NotepadApp::new();
        // Insert the hardware string into the notepad so the user sees it immediately!
        notepad_app.handle_event(desktop::app::Event::KeyPress('\n'));
        for c in cpu_features.chars() {
            notepad_app.handle_event(desktop::app::Event::KeyPress(c));
        }

        let notepad = alloc::boxed::Box::new(notepad_app);
        let window = desktop::window::Window::new(notepad, win_x, win_y, win_width, win_height);
        compositor.add_window(window);

        // Optional: put compositor in a global mutex for interrupts, or just handle interrupts via a queue.
        // For simplicity, we just loop and render
        // INTERRUPTS ERST HIER AKTIVIEREN, WENN ALLES BEREIT IST!
        x86_64::instructions::interrupts::enable();

        loop {
            // Process events from a global queue (implemented in interrupts.rs)
            if let Some(c) = interrupts::pop_key() {
                if c == '\x08' {
                    compositor.dispatch_keycode_event(0x08);
                } else if c == '\n' {
                    compositor.dispatch_keyboard_event('\n');
                } else {
                    compositor.dispatch_keyboard_event(c);
                }
            }

            compositor.render_all();
            x86_64::instructions::hlt();
        }
    }

    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
