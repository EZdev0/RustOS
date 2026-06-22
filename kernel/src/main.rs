#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

mod desktop;
mod interrupts;
mod allocator;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    // Initialize the dynamic Heap Allocator
    allocator::init_heap();

    // Setup global compositor
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let mut compositor = desktop::compositor::GraphicalCompositor::new(framebuffer);
        
        let width = compositor.info().width;
        let height = compositor.info().height;
        let win_width = if width > 600 { width - 200 } else { width - 40 };
        let win_height = if height > 400 { height - 150 } else { height - 80 };
        let win_x = (width - win_width) / 2;
        let win_y = (height - win_height) / 2 - 20;

        let notepad = alloc::boxed::Box::new(desktop::notepad::NotepadApp::new());
        let window = desktop::window::Window::new(notepad, win_x, win_y, win_width, win_height);
        compositor.add_window(window);

        // Optional: put compositor in a global mutex for interrupts, or just handle interrupts via a queue.
        // For simplicity, we just loop and render
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
