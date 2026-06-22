use bootloader_api.info::{Framebuffer, FramebufferInfo, PixelFormat};
use crate::desktop::window::Window;

pub struct GraphicalCompositor {
    info: FramebufferInfo,
    buffer: &'static mut [u8],
}

impl GraphicalCompositor {
    pub fn new(framebuffer: &'static mut Framebuffer) -> Self {
        let info = framebuffer.info();
        let buffer = framebuffer.buffer_mut();
        Self { info, buffer }
    }

    #[inline]
    pub fn draw_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.info.horizontal_resolution || y >= self.info.vertical_resolution {
            return;
        }

        // Berechnung des Pixel-Offsets im linearen Framebuffer
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
        let width = self.info.horizontal_resolution;
        let height = self.info.vertical_resolution;

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

        // 4. Erstes Anwendungsfenster (GUI Window)
        let win = Window::new("Kernel space Terminal", 120, 90, 550, 380);
        
        // Fensterschatten
        self.draw_rect(win.x - 2, win.y - 2, win.width + 4, win.height + 4, 25, 25, 28);
        // Fenster-Header
        self.draw_rect(win.x, win.y, win.width, 34, 215, 218, 224);
        // Fenster-Inhalt (Dunkles Terminal-Innere)
        self.draw_rect(win.x, win.y + 34, win.width, win.height - 34, 18, 19, 24);

        // Fenster-Bedienknöpfe (macOS-Style Ampelsystem)
        self.draw_rect(win.x + 14, win.y + 11, 12, 12, 252, 92, 86);   // Rot
        self.draw_rect(win.x + 34, win.y + 11, 12, 12, 251, 188, 46);  // Gelb
        self.draw_rect(win.x + 54, win.y + 11, 12, 12, 43, 202, 66);   // Grün

        // Simulierter Textinhalt im Terminal
        self.draw_rect(win.x + 25, win.y + 60, 180, 5, 240, 240, 245);
        self.draw_rect(win.x + 25, win.y + 80, 320, 5, 120, 220, 120);
        self.draw_rect(win.x + 25, win.y + 100, 140, 5, 250, 150, 100);

        // 5. Hardware-Mauszeiger (Präzises Grafik-Dreieck im Zentrum)
        let mouse_x = width / 2;
        let mouse_y = height / 2;
        for i in 0..16 {
            self.draw_rect(mouse_x + i, mouse_y + i, 16 - i, 1, 255, 255, 255);
            self.draw_pixel(mouse_x + i, mouse_y + i, 5, 5, 10);
        }
    }
}
