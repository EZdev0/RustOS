#![no_std]
#![no_main]

mod desktop;
mod interrupts;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let mut compositor = desktop::compositor::GraphicalCompositor::new(framebuffer);
        compositor.render_desktop();
        
        let mut last_len = 0;
        loop {
            // Check for new characters in TEXT_BUFFER and render them
            {
                let buf = desktop::terminal::TEXT_BUFFER.lock();
                if buf.len() != last_len {
                    compositor.draw_terminal_text(buf.as_str());
                    last_len = buf.len();
                }
            }
            // Sleep until next interrupt
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
