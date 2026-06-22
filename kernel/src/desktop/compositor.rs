use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use crate::desktop::window::Window;
use crate::desktop::app::Event;
use alloc::vec::Vec;
use alloc::string::ToString;
use font8x8::UnicodeFonts;

pub struct GraphicalCompositor {
    info: FrameBufferInfo,
    buffer: &'static mut [u8],
    pub windows: Vec<Window>,
}

impl GraphicalCompositor {
    pub fn new(framebuffer: &'static mut FrameBuffer) -> Self {
        let info = framebuffer.info();
        let buffer = framebuffer.buffer_mut();
        Self { info, buffer, windows: Vec::new() }
    }

    pub fn info(&self) -> &FrameBufferInfo {
        &self.info
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows.push(window);
    }

    #[inline]
    pub fn draw_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }

        // Berechnung des Pixel-Offsets im linearen FrameBuffer
        let pixel_offset = (y * self.info.stride + x) * (self.info.bytes_per_pixel);

        if pixel_offset + 2 < self.buffer.len() {
            match self.info.pixel_format {
                PixelFormat::Rgb => {
                    self.buffer[pixel_offset] = r;
                    self.buffer[pixel_offset + 1] = g;
                    self.buffer[pixel_offset + 2] = b;
                }
                PixelFormat::Bgr => {
                    self.buffer[pixel_offset] = b;
                    self.buffer[pixel_offset + 1] = g;
                    self.buffer[pixel_offset + 2] = r;
                }
                _ => {
                    self.buffer[pixel_offset] = b;
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

    pub fn render_all(&mut self) {
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

        // Wir muessen den Borrow Checker ueberlisten, indem wir die Indizes iterieren.
        // Denn wir koennen self nicht an window.app.draw uebergeben, waehrend wir ueber self.windows iterieren!
        for i in 0..self.windows.len() {
            // Update Window Logik
            self.windows[i].app.update();

            // Draw Fensterschatten
            let wx = self.windows[i].x;
            let wy = self.windows[i].y;
            let ww = self.windows[i].width;
            let wh = self.windows[i].height;
            let title = self.windows[i].app.title().to_string(); // we need an alloc crate import for this

            self.draw_rect(wx.saturating_sub(2), wy.saturating_sub(2), ww + 4, wh + 4, 25, 25, 28);
            // Fenster-Header
            self.draw_rect(wx, wy, ww, 34, 215, 218, 224);
            
            // Fenster-Bedienknöpfe
            self.draw_rect(wx + 14, wy + 11, 12, 12, 252, 92, 86);
            self.draw_rect(wx + 34, wy + 11, 12, 12, 251, 188, 46);
            self.draw_rect(wx + 54, wy + 11, 12, 12, 43, 202, 66);

            // Draw Titel
            let mut cur_tx = wx + 80;
            for c in title.chars() {
                self.draw_char(cur_tx, wy + 13, c, 30, 30, 30);
                cur_tx += 8;
            }
        }
        
        // Jetzt rufen wir app.draw() auf
        // Dazu muessen wir temporär die App aus dem Window swappen!
        for i in 0..self.windows.len() {
            // Dies ist ein Trick in Rust, da self in draw benoetigt wird
            // Wir verwenden Option oder wir lassen die Methode einfach x, y als Argumente nehmen.
            // Wait, since we are doing this in the same file, we can just use raw pointers or extract.
            // For now, let's just cheat the borrow checker with unsafe for simplicity in ring0:
            let window_ptr = &mut self.windows[i] as *mut Window;
            let wx = unsafe { (*window_ptr).x };
            let wy = unsafe { (*window_ptr).y };
            let ww = unsafe { (*window_ptr).width };
            let wh = unsafe { (*window_ptr).height };
            
            unsafe {
                (*window_ptr).app.draw(self, wx, wy + 34, ww, wh.saturating_sub(34));
            }
        }

        // 5. Hardware-Mauszeiger (Präzises Grafik-Dreieck im Zentrum)
        let mouse_x = width / 2;
        let mouse_y = height / 2;
        for i in 0..16 {
            self.draw_rect(mouse_x + i, mouse_y + i, 16 - i, 1, 255, 255, 255);
            self.draw_pixel(mouse_x + i, mouse_y + i, 5, 5, 10);
        }
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
        if let Some(focused) = self.windows.last_mut() {
            focused.app.handle_event(Event::KeyPress(c));
        }
    }

    pub fn dispatch_keycode_event(&mut self, code: u8) {
        if let Some(focused) = self.windows.last_mut() {
            focused.app.handle_event(Event::KeyCode(code));
        }
    }
}
