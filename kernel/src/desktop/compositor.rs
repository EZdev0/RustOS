use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use crate::desktop::window::Window;
use font8x8::UnicodeFonts;

pub struct GraphicalCompositor {
    info: FrameBufferInfo,
    buffer: &'static mut [u8],
}

impl GraphicalCompositor {
    pub fn new(framebuffer: &'static mut FrameBuffer) -> Self {
        let info = framebuffer.info();
        let buffer = framebuffer.buffer_mut();
        Self { info, buffer }
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

        // 4. Erstes Anwendungsfenster (GUI Window) - RESPONSIVE!
        let win_width = if width > 600 { width - 200 } else { width - 40 };
        let win_height = if height > 400 { height - 150 } else { height - 80 };
        let win_x = (width - win_width) / 2;
        let win_y = (height - win_height) / 2 - 20;

        let win = Window::new("Kernel space Terminal", win_x, win_y, win_width, win_height);
        
        // Fensterschatten
        self.draw_rect(win.x.saturating_sub(2), win.y.saturating_sub(2), win.width + 4, win.height + 4, 25, 25, 28);
        // Fenster-Header
        self.draw_rect(win.x, win.y, win.width, 34, 215, 218, 224);
        // Fenster-Inhalt (Dunkles Terminal-Innere)
        self.draw_rect(win.x, win.y + 34, win.width, win.height.saturating_sub(34), 18, 19, 24);

        // Fenster-Bedienknöpfe (macOS-Style Ampelsystem)
        self.draw_rect(win.x + 14, win.y + 11, 12, 12, 252, 92, 86);   // Rot
        self.draw_rect(win.x + 34, win.y + 11, 12, 12, 251, 188, 46);  // Gelb
        self.draw_rect(win.x + 54, win.y + 11, 12, 12, 43, 202, 66);   // Grün

        // Simulierter Textinhalt im Terminal (Nur wenn Platz ist)
        if win.width > 200 && win.height > 150 {
            self.draw_rect(win.x + 25, win.y + 60, win.width / 3, 5, 240, 240, 245);
            self.draw_rect(win.x + 25, win.y + 80, win.width / 2, 5, 120, 220, 120);
            self.draw_rect(win.x + 25, win.y + 100, win.width / 4, 5, 250, 150, 100);
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

    pub fn draw_terminal_text(&mut self, text: &str) {
        let width = self.info.width;
        let height = self.info.height;

        let win_width = if width > 600 { width - 200 } else { width - 40 };
        let win_height = if height > 400 { height - 150 } else { height - 80 };
        let win_x = (width - win_width) / 2;
        let win_y = (height - win_height) / 2 - 20;

        let padding = 10;
        
        let start_x = win_x + padding;
        let start_y = win_y + 34 + padding;
        
        // Clear the terminal background first
        self.draw_rect(win_x, win_y + 34, win_width, win_height.saturating_sub(34), 18, 19, 24);

        let mut current_x = start_x;
        let mut current_y = start_y;

        // Terminal prompt
        let prompt = "root@RustOS:~$ ";
        for c in prompt.chars() {
            if current_x + 8 > win_x + win_width - padding {
                break; // Skip drawing if out of bounds to prevent crash
            }
            self.draw_char(current_x, current_y, c, 43, 202, 66); // Green prompt
            current_x += 8;
        }

        // Output text
        for c in text.chars() {
            if c == '\n' {
                current_y += 12;
                current_x = start_x;
            } else if c == '\x08' { // Backspace
                if current_x > start_x {
                    current_x -= 8;
                    // Draw a black rectangle over the character to erase it
                    self.draw_rect(current_x, current_y, 8, 8, 18, 19, 24);
                }
            } else {
                if current_x + 8 > win_x + win_width - padding {
                    current_y += 12;
                    current_x = start_x;
                }
                self.draw_char(current_x, current_y, c, 240, 240, 245);
                current_x += 8;
            }
        }
    }
}
