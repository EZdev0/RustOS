use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use crate::desktop::window::Window;
use crate::desktop::app::Event;
use alloc::vec::Vec;
use font8x8::UnicodeFonts;

pub struct GraphicalCompositor {
    info: FrameBufferInfo,
    pub framebuffer: &'static mut [u8],
    pub backbuffer: Vec<u8>,
    pub windows: Vec<Option<Window>>,
    pub mouse_x: usize,
    pub mouse_y: usize,
    pub mouse_left_down: bool,
    pub dragging_window: Option<usize>,
    pub drag_offset_x: isize,
    pub drag_offset_y: isize,
}

// Fast Integer Square Root for software rendering algorithms
fn fast_isqrt(n: usize) -> usize {
    if n <= 1 { return n; }
    let mut x0 = n / 2;
    let mut x1 = (x0 + n / x0) / 2;
    while x1 < x0 {
        x0 = x1;
        x1 = (x0 + n / x0) / 2;
    }
    x0
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

        Self { 
            info, 
            framebuffer: buffer, 
            backbuffer, 
            windows: Vec::new(), 
            mouse_x, 
            mouse_y,
            mouse_left_down: false,
            dragging_window: None,
            drag_offset_x: 0,
            drag_offset_y: 0,
        }
    }

    pub fn info(&self) -> &FrameBufferInfo {
        &self.info
    }

    pub fn add_window(&mut self, window: Window) {
        self.windows.push(Some(window));
    }

    #[inline]
    pub fn read_pixel(&self, x: usize, y: usize) -> (u8, u8, u8) {
        if x >= self.info.width || y >= self.info.height {
            return (0, 0, 0);
        }
        let pixel_offset = (y * self.info.stride + x) * (self.info.bytes_per_pixel);
        if pixel_offset + 2 < self.backbuffer.len() {
            match self.info.pixel_format {
                PixelFormat::Rgb => {
                    (self.backbuffer[pixel_offset], self.backbuffer[pixel_offset + 1], self.backbuffer[pixel_offset + 2])
                }
                PixelFormat::Bgr => {
                    (self.backbuffer[pixel_offset + 2], self.backbuffer[pixel_offset + 1], self.backbuffer[pixel_offset])
                }
                _ => (0, 0, 0),
            }
        } else {
            (0, 0, 0)
        }
    }

    #[inline]
    pub fn draw_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }

        let pixel_offset = (y * self.info.stride + x) * (self.info.bytes_per_pixel);

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

    #[inline]
    pub fn blend_pixel(&mut self, x: usize, y: usize, r: u8, g: u8, b: u8, alpha_256: u16) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        let pixel_offset = (y * self.info.stride + x) * (self.info.bytes_per_pixel);
        if pixel_offset + 2 < self.backbuffer.len() {
            let inv_alpha = 256 - alpha_256;
            
            let (bg_r, bg_g, bg_b) = match self.info.pixel_format {
                PixelFormat::Rgb => {
                    (self.backbuffer[pixel_offset], self.backbuffer[pixel_offset + 1], self.backbuffer[pixel_offset + 2])
                }
                PixelFormat::Bgr => {
                    (self.backbuffer[pixel_offset + 2], self.backbuffer[pixel_offset + 1], self.backbuffer[pixel_offset])
                }
                _ => (0, 0, 0),
            };

            let out_r = ((r as u16 * alpha_256 + bg_r as u16 * inv_alpha) >> 8) as u8;
            let out_g = ((g as u16 * alpha_256 + bg_g as u16 * inv_alpha) >> 8) as u8;
            let out_b = ((b as u16 * alpha_256 + bg_b as u16 * inv_alpha) >> 8) as u8;

            match self.info.pixel_format {
                PixelFormat::Rgb => {
                    self.backbuffer[pixel_offset] = out_r;
                    self.backbuffer[pixel_offset + 1] = out_g;
                    self.backbuffer[pixel_offset + 2] = out_b;
                }
                PixelFormat::Bgr => {
                    self.backbuffer[pixel_offset] = out_b;
                    self.backbuffer[pixel_offset + 1] = out_g;
                    self.backbuffer[pixel_offset + 2] = out_r;
                }
                _ => {
                    self.backbuffer[pixel_offset] = out_b;
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

    pub fn draw_shadow_and_glow(&mut self, x: usize, y: usize, w: usize, h: usize, is_active: bool) {
        let shadow_size = if is_active { 16isize } else { 10isize };
        let r = 8usize;
        
        let ext_x = (x as isize - shadow_size).max(0) as usize;
        let ext_y = (y as isize - shadow_size).max(0) as usize;
        let ext_w = w + (shadow_size as usize) * 2;
        let ext_h = h + (shadow_size as usize) * 2;
        
        let (sr, sg, sb) = if is_active {
            (0, 150, 255) // Glow Color (Cyan/Blue)
        } else {
            (0, 0, 0) // Shadow Color (Black)
        };

        for py in ext_y..(ext_y + ext_h) {
            if py >= self.info.height { break; }
            for px in ext_x..(ext_x + ext_w) {
                if px >= self.info.width { break; }
                
                let dx = if px < x + r {
                    (x + r) - px
                } else if px >= x + w - r {
                    px - (x + w - r) + 1
                } else {
                    0
                };

                let dy = if py < y + r {
                    (y + r) - py
                } else if py >= y + h - r {
                    py - (y + h - r) + 1
                } else {
                    0
                };

                let dist_sq = dx * dx + dy * dy;
                let r_sq = r * r;
                
                if dist_sq > r_sq {
                    let dist = fast_isqrt(dist_sq) as isize - r as isize;
                    if dist > 0 && dist < shadow_size {
                        let alpha = 256 - (dist * 256 / shadow_size);
                        let alpha = alpha * alpha / 256; 
                        let intensity = if is_active { 
                            alpha * 220 / 256 
                        } else { 
                            alpha * 180 / 256 
                        };
                        self.blend_pixel(px, py, sr, sg, sb, intensity as u16);
                    }
                }
            }
        }
    }

    pub fn render_desktop(&mut self) {
        let width = self.info.width;
        let height = self.info.height;

        // Modern Gradient Background
        for y in 0..height {
            let r = 15 + (y * 20 / height) as u8;
            let g = 20 + (y * 25 / height) as u8;
            let b = 30 + (y * 35 / height) as u8;
            for x in 0..width {
                self.draw_pixel(x, y, r, g, b);
            }
        }

        // Top System Bar (Dark Glass look)
        self.draw_rect(0, 0, width, 28, 10, 10, 12);
        self.draw_char(10, 6, 'V', 0, 180, 255);

        // Bottom Dock
        let dock_w = 440;
        let dock_h = 60;
        let dock_x = (width - dock_w) / 2;
        let dock_y = height - dock_h - 15;
        self.draw_rect(dock_x, dock_y, dock_w, dock_h, 255, 255, 255);

        for i in 0..5 {
            let icon_x = dock_x + 25 + i * 85;
            let icon_y = dock_y + 10;
            if i == 0 {
                self.draw_rect(icon_x, icon_y, 40, 40, 255, 255, 255);
                for line in 1..4 {
                    self.draw_rect(icon_x + 5, icon_y + line * 10, 30, 2, 100, 150, 255);
                }
            } else if i == 1 {
                self.draw_rect(icon_x, icon_y, 40, 40, 0, 0, 0);
                self.draw_char(icon_x + 10, icon_y + 15, '>', 0, 255, 0);
            } else {
                self.draw_rect(icon_x, icon_y, 40, 40, 60 + (i as u8 * 30), 110 + (i as u8 * 20), 200);
            }
        }
    }

    pub fn render_all(&mut self) {
        self.render_desktop();
        
        let num_windows = self.windows.len();
        
        for i in 0..num_windows {
            if let Some(mut window) = self.windows[i].take() {
                let is_active = i == num_windows - 1;
                
                let win_x = window.x;
                let win_y = window.y;
                let win_w = window.width;
                let win_h = window.height + 20;

                // 1. Draw Drop-Shadow or Glow first
                self.draw_shadow_and_glow(win_x, win_y, win_w, win_h, is_active);
                
                // 2. Backup corners (which now include shadow/glow + background)
                let r = 8;
                let mut corners_backup = [(0u8, 0u8, 0u8); 8 * 8 * 4];

                for cy in 0..r {
                    for cx in 0..r {
                        corners_backup[cy * r + cx] = self.read_pixel(win_x + cx, win_y + cy);
                        corners_backup[64 + cy * r + cx] = self.read_pixel(win_x + win_w - r + cx, win_y + cy);
                        corners_backup[128 + cy * r + cx] = self.read_pixel(win_x + cx, win_y + win_h - r + cy);
                        corners_backup[192 + cy * r + cx] = self.read_pixel(win_x + win_w - r + cx, win_y + win_h - r + cy);
                    }
                }

                // 3. Draw standard solid rectangular window contents
                self.draw_rect(win_x, win_y, win_w, 20, 60, 60, 60); // Title Bar
                self.draw_rect(win_x + win_w - 20, win_y, 20, 20, 220, 50, 50); // Close Button
                self.draw_char(win_x + win_w - 14, win_y + 6, 'X', 255, 255, 255);
                window.app.draw(self, win_x, win_y + 20, win_w, window.height); // App content
                
                // 4. Restore Corners (Masking with a circle equation)
                let r_sq = r * r;

                for cy in 0..r {
                    for cx in 0..r {
                        let dy_top = r - cy;
                        let dy_bot = cy + 1;
                        let dx_left = r - cx;
                        let dx_right = cx + 1;

                        if dx_left * dx_left + dy_top * dy_top > r_sq {
                            let (pr, pg, pb) = corners_backup[cy * r + cx];
                            self.draw_pixel(win_x + cx, win_y + cy, pr, pg, pb);
                        }
                        if dx_right * dx_right + dy_top * dy_top > r_sq {
                            let (pr, pg, pb) = corners_backup[64 + cy * r + cx];
                            self.draw_pixel(win_x + win_w - r + cx, win_y + cy, pr, pg, pb);
                        }
                        if dx_left * dx_left + dy_bot * dy_bot > r_sq {
                            let (pr, pg, pb) = corners_backup[128 + cy * r + cx];
                            self.draw_pixel(win_x + cx, win_y + win_h - r + cy, pr, pg, pb);
                        }
                        if dx_right * dx_right + dy_bot * dy_bot > r_sq {
                            let (pr, pg, pb) = corners_backup[192 + cy * r + cx];
                            self.draw_pixel(win_x + win_w - r + cx, win_y + win_h - r + cy, pr, pg, pb);
                        }
                    }
                }

                self.windows[i] = Some(window);
            }
        }

        // Hardware Cursor
        for i in 0..16 {
            self.draw_rect(self.mouse_x + i, self.mouse_y + i, 16 - i, 1, 255, 255, 255);
            self.draw_pixel(self.mouse_x + i, self.mouse_y + i, 5, 5, 10);
        }

        self.framebuffer.copy_from_slice(&self.backbuffer);
    }

    pub fn swap_buffers(&mut self) {
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
        if let Some(Some(window)) = self.windows.last_mut() {
            window.app.handle_event(Event::KeyPress(c));
        }
    }

    pub fn dispatch_keycode_event(&mut self, code: u8) {
        if let Some(Some(window)) = self.windows.last_mut() {
            window.app.handle_event(Event::KeyCode(code));
        }
    }
}
