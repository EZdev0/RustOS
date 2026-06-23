#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod allocator;
pub mod desktop;
pub mod hardware;
pub mod interrupts;
pub mod fs;
pub mod gdt;
pub mod task;
pub mod network;
use bootloader_api::{entry_point, BootInfo};
use core::panic::PanicInfo;
use crate::desktop::app::App;
use core::fmt::Write;
use font8x8::UnicodeFonts;

pub struct RawFrameBufferInfo {
    pub ptr: *mut u8,
    pub len: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub bytes_per_pixel: usize,
    pub pixel_format: bootloader_api::info::PixelFormat,
}

pub static mut CRASH_SCREEN_INFO: Option<RawFrameBufferInfo> = None;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::init_pit(1000);

    // Initialize the dynamic Heap Allocator
    allocator::init_heap();
    interrupts::init_mouse();

    // Detect CPU features and initialize SIMD/AVX
    let cpu_features = hardware::cpuid::detect_and_init();

    // Setup global compositor
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        let info = framebuffer.info();
        let ptr = framebuffer.buffer_mut().as_mut_ptr();
        let len = framebuffer.buffer_mut().len();
        unsafe {
            CRASH_SCREEN_INFO = Some(RawFrameBufferInfo {
                ptr,
                len,
                width: info.width,
                height: info.height,
                stride: info.stride,
                bytes_per_pixel: info.bytes_per_pixel,
                pixel_format: info.pixel_format,
            });
        }

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

        // Initialize PCI and Network
        let pci_devices = hardware::pci::scan_pci();
        if let Some(e1000) = hardware::e1000::init_e1000(&pci_devices) {
            let mac = e1000.mac_address();
            let pci_str = alloc::format!("\n[PCI] Intel PRO/1000 E1000 Network Card found!\n[NET] MAC Address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\n", 
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
            for c in pci_str.chars() {
                notepad_app.handle_event(desktop::app::Event::KeyPress(c));
            }
        } else {
            for c in "\n[PCI] No Intel E1000 Network Card found.\n".chars() {
                notepad_app.handle_event(desktop::app::Event::KeyPress(c));
            }
        }


        let notepad = alloc::boxed::Box::new(notepad_app);
        let window = desktop::window::Window::new(notepad, win_x, win_y, win_width, win_height);
        
        // INTERRUPTS MÜSSEN FÜR TIMER AKTIVIERT WERDEN
        x86_64::instructions::interrupts::enable();

        // BOOT ANIMATION (macOS Glass / Breathing Glow Style)
        let cx = width / 2;
        let cy = height / 2 - 20;
        let loading_bar_w = 200;
        let loading_bar_h = 4;
        let bar_x = cx - loading_bar_w / 2;
        let bar_y = cy + 80;

        for frame in 0..120 {
            compositor.draw_rect(0, 0, width, height, 18, 18, 22);

            let pulse = if frame % 40 < 20 { frame % 40 } else { 40 - (frame % 40) }; 
            let alpha = 150 + (pulse * 5) as u16;
            let radius = 40 + (pulse / 4); 

            compositor.draw_glowing_ring(cx, cy, radius, 12, alpha, 0, 150, 255);
            compositor.draw_glowing_ring(cx, cy, radius.saturating_sub(8), 3, alpha, 255, 255, 255);

            for y in bar_y..(bar_y + loading_bar_h) {
                for x in bar_x..(bar_x + loading_bar_w) {
                    compositor.blend_pixel(x, y, 255, 255, 255, 30);
                }
            }

            let progress = (frame * loading_bar_w) / 120;
            for y in bar_y..(bar_y + loading_bar_h) {
                for x in bar_x..(bar_x + progress) {
                    compositor.blend_pixel(x, y, 0, 150, 255, 255);
                }
            }

            compositor.swap_buffers();

            let start = crate::interrupts::TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed);
            while crate::interrupts::TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed) < start + 1 {
                x86_64::instructions::hlt();
            }
        }

        // 1. Arc/Mutex wrapper for the compositor so multiple async tasks can share it
        compositor.add_window(window);
        compositor.render_all();

        let shared_compositor = alloc::sync::Arc::new(spin::Mutex::new(compositor));

        // 2. Initialize Executor
        let mut executor = task::executor::Executor::new();

        // 3. Spawn UI Input Tasks
        executor.spawn(task::Task::new(task::keyboard::keyboard_task(shared_compositor.clone())));
        executor.spawn(task::Task::new(task::mouse::mouse_task(shared_compositor.clone())));

        // 4. Start Scheduler Loop (never returns)
        executor.run();
    }

    loop {
        x86_64::instructions::hlt();
    }
}

struct FbWriter<'a> {
    fb: &'a mut RawFrameBufferInfo,
    buffer: &'a mut [u8],
    x: usize,
    y: usize,
    scale: usize,
    margin_x: usize,
}

impl<'a> Write for FbWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            if c == '\n' {
                self.x = self.margin_x;
                self.y += 8 * self.scale + 10;
                continue;
            }
            
            if let Some(bitmap) = font8x8::BASIC_FONTS.get(c).or_else(|| font8x8::BASIC_FONTS.get('?')) {
                for (row, byte) in bitmap.iter().enumerate() {
                    for col in 0..8 {
                        if (*byte & (1 << col)) != 0 {
                            for sy in 0..self.scale {
                                for sx in 0..self.scale {
                                    let px = self.x + col * self.scale + sx;
                                    let py = self.y + row * self.scale + sy;
                                    if px < self.fb.width && py < self.fb.height {
                                        let offset = (py * self.fb.stride + px) * self.fb.bytes_per_pixel;
                                        if offset + 2 < self.buffer.len() {
                                            self.buffer[offset] = 255;
                                            self.buffer[offset+1] = 255;
                                            self.buffer[offset+2] = 255;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            self.x += 8 * self.scale + 2;
            if self.x + 8 * self.scale > self.fb.width {
                self.x = self.margin_x;
                self.y += 8 * self.scale + 10;
            }
        }
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    x86_64::instructions::interrupts::disable();

    unsafe {
        let crash_info_ptr = core::ptr::addr_of_mut!(CRASH_SCREEN_INFO);
        if let Some(fb) = (*crash_info_ptr).as_mut() {
            let buffer = core::slice::from_raw_parts_mut(fb.ptr, fb.len);
            
            for i in (0..buffer.len()).step_by(fb.bytes_per_pixel) {
                if i + 2 < buffer.len() {
                    match fb.pixel_format {
                        bootloader_api::info::PixelFormat::Rgb => {
                            buffer[i] = 170;
                            buffer[i+1] = 0;
                            buffer[i+2] = 0;
                        }
                        bootloader_api::info::PixelFormat::Bgr => {
                            buffer[i] = 0;
                            buffer[i+1] = 0;
                            buffer[i+2] = 170;
                        }
                        _ => {
                            buffer[i] = 170;
                            buffer[i+1] = 0;
                            buffer[i+2] = 0;
                        }
                    }
                }
            }

            let mut writer = FbWriter {
                fb: fb,
                buffer: buffer,
                x: 50,
                y: 50,
                scale: 3, 
                margin_x: 50,
            };
            
            let _ = write!(writer, "==========================================\n");
            let _ = write!(writer, " FATAL EXCEPTION DETECTED \n");
            let _ = write!(writer, " RED SCREEN OF DEATH \n");
            let _ = write!(writer, "==========================================\n\n");
            let _ = write!(writer, "{}\n\n", info);
            let _ = write!(writer, "SYSTEM HALTED. PLEASE RESTART.");
        }
    }

    loop {
        x86_64::instructions::hlt();
    }
}
