use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use crate::desktop::window::Window;
use crate::desktop::app::Event;
use alloc::vec::Vec;
use alloc::string::ToString;
use font8x8::UnicodeFonts;

pub struct GraphicalCompositor {
    info: FrameBufferInfo,
    framebuffer: &'static mut [u8],
    backbuffer: Vec<u8>,
    pub windows: Vec<Option<Window>>,
    pub mouse_x: usize,
    pub mouse_y: usize,
}

impl GraphicalCompositor {
    pub fn new(framebuffer: &'static mut FrameBuffer) -> Self {
        let info = framebuffer.info();
        let buffer = framebuffer.buffer_mut();
        let size = buffer.len();
        
        let mut backbuffer = Vec::with_capacity(size);
        backbuffer.resize(size, 0);
        
        let mouse_x = info.width / 2;
        let mouse_y = info.height / 2;

        Self { info, framebuffer: buffer, backbuffer, windows: Vec::new(), mouse_x, mouse_y }
    }

    pub fn info(&self) -> &FrameBufferInfo {
        &self.info
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows.push(Some(window));
    }

    #[inline]
    pub fn draw_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }

        // Berechnung des Pixel-Offsets im linearen FrameBuffer
        let pixel_offset = (y * self.info.stride + x) * (self.info.bytes_per_pixel);

        // Zeichnen in den BACKBUFFER!
        if pixel_offset + 2 < self.backbuffer.len() {
            match self.info.pixel_format {
                PixelFormat::Rgb => {
                    self.backbuffer[pixel_offset] = r;
                    self.backbuffer[pixel_offset + 1] = g;
                    self.backbuffer[pixel_offset + 2] = b;
                }
                PixelFormat::Bgr => {
                    self.backbuffer[pixel_offset] = b;
                    self.backbuffer[pixel_offset + 1] = g;
                    self.backbuffer[pixel_offset + 2] = r;
                }
                _ => {
                    self.backbuffer[pixel_offset] = b;
                }
            }
        }
    }

    pub fn draw_rect(&mut self, start_x: usize, start_y: usize, width: usize, height: usize, r: u8, g: u8, b: u8) {
        for y in start_y..(start_y + height) {
            for x in start_x..(start_x + width) {
                self.draw_pixel(x, y, r, g, b);
            }
        }
    }

    pub fn render_desktop(&mut self) {
        let width = self.info.width;
        let height = self.info.height;

        // 1. Hintergrund (Modernes Anthrazit-Blau)
        self.draw_rect(0, 0, width, height, 43, 48, 59);

        // 2. Obere System-Leiste (Menu Bar)
        self.draw_rect(0, 0, width, 28, 238, 238, 242);

        // Apple/VibeOS Logo-Ersatz links in der Ecke (Blauer Block)
        self.draw_rect(10, 6, 16, 16, 50, 120, 220);

        // 3. Unteres Anwendungs-Dock
        let dock_w = 440;
        let dock_h = 60;
        let dock_x = (width - dock_w) / 2;
        let dock_y = height - dock_h - 15;
        self.draw_rect(dock_x, dock_y, dock_w, dock_h, 255, 255, 255);

        // Icons im Dock simulieren (Fünf Farbquadrate)
        for i in 0..5 {
            let icon_x = dock_x + 25 + i * 85;
            self.draw_rect(icon_x, dock_y + 10, 40, 40, 60 + (i as u8 * 30), 110 + (i as u8 * 20), 200);
        }
    }

    pub fn render_all(&mut self) {
        self.render_desktop();
        // Safe Architecture: Take the window temporarily out of the Vec to bypass borrow conflicts
        for i in 0..self.windows.len() {
            if let Some(mut window) = self.windows[i].take() {
                window.app.draw(self, window.x, window.y, window.width, window.height);
                self.windows[i] = Some(window);
            }
        }

        // 5. Hardware-Mauszeiger (Präzises Grafik-Dreieck im Zentrum)
        for i in 0..16 {
            self.draw_rect(self.mouse_x + i, self.mouse_y + i, 16 - i, 1, 255, 255, 255);
            self.draw_pixel(self.mouse_x + i, self.mouse_y + i, 5, 5, 10);
        }

        self.framebuffer.copy_from_slice(&self.backbuffer);
    }

    pub fn draw_char(&mut self, x: usize, y: usize, c: char, r: u8, g: u8, b: u8) {
        if let Some(bitmap) = font8x8::BASIC_FONTS.get(c).or_else(|| font8x8::LATIN_FONTS.get(c)) {
            for (row, byte) in bitmap.iter().enumerate() {
                for col in 0..8 {
                    if (*byte & (1 << col)) != 0 {
                        self.draw_pixel(x + col, y + row, r, g, b);
                    }
                }
            }
        }
    }

    pub fn dispatch_keyboard_event(&mut self, c: char) {
        for i in 0..self.windows.len() {
            if let Some(mut window) = self.windows[i].take() {
                window.app.handle_event(Event::KeyPress(c));
                self.windows[i] = Some(window);
            }
        }
    }

    pub fn dispatch_keycode_event(&mut self, code: u8) {
        for i in 0..self.windows.len() {
            if let Some(mut window) = self.windows[i].take() {
                window.app.handle_event(Event::KeyCode(code));
                self.windows[i] = Some(window);
            }
        }
    }
}
