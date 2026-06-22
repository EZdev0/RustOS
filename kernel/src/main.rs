#![no_std]
#![no_main]

mod desktop;

use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;

// Offizielles Makro zur Registrierung des x86_64 Entrypoints
entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // Sicheres Auslesen des vom Bootloader bereitgestellten Grafik-Abbilds
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let mut compositor = desktop::compositor::GraphicalCompositor::new(framebuffer);
        compositor.render_desktop();
    }

    loop {
        // CPU in den Energiesparmodus versetzen, statt Hitze zu erzeugen
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
