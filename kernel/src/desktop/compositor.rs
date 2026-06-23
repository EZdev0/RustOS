use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use crate::desktop::window::Window;
use crate::desktop::app::Event;
use alloc::vec::Vec;
use font8x8::UnicodeFonts;

pub struct GraphicalCompositor {
    info: FrameBufferInfo,
    framebuffer: &'static mut [u8],
    backbuffer: Vec<u8>,
    dock_buffer: Vec<u8>,
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

        let mut dock_buffer = Vec::with_capacity(440 * 60 * 3);
        dock_buffer.resize(440 * 60 * 3, 0);
        
        let mouse_x = info.width / 2;
        let mouse_y = info.height / 2;

        Self { 
            info, 
            framebuffer: buffer, 
            backbuffer, 
            dock_buffer,
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

    pub fn draw_shadow_and_glow(&mut self, x: usize, y: usize, w: usize, h: usize, corner_r: usize, is_active: bool) {
        let shadow_size = if is_active { 16isize } else { 10isize };
        
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
                
                let dx = if px < x + corner_r {
                    (x + corner_r) - px
                } else if px >= x + w - corner_r {
                    px - (x + w - corner_r) + 1
                } else {
                    0
                };

                let dy = if py < y + corner_r {
                    (y + corner_r) - py
                } else if py >= y + h - corner_r {
                    py - (y + h - corner_r) + 1
                } else {
                    0
                };

                let dist_sq = dx * dx + dy * dy;
                let r_sq = corner_r * corner_r;
                
                if dist_sq > r_sq {
                    let dist = fast_isqrt(dist_sq) as isize - corner_r as isize;
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

        // Top System Bar (Dark Glass look via Blending)
        for dy in 0..28 {
            for dx in 0..width {
                self.blend_pixel(dx, dy, 10, 10, 15, 180);
            }
        }
        self.draw_char(10, 6, 'V', 0, 180, 255);
    }

    pub fn render_glass_dock(&mut self) {
        let width = self.info.width;
        let height = self.info.height;

        let dock_w = 440;
        let dock_h = 60;
        let dock_x = width.saturating_sub(dock_w) / 2;
        let dock_y = height.saturating_sub(dock_h + 15);
        let corner_r = 16usize; 
        let blur_r = 3usize;
        
        let glass_alpha = 110u16; 

        // Dock Shadow
        self.draw_shadow_and_glow(dock_x, dock_y, dock_w, dock_h, corner_r, false);

        for dy in 0..dock_h {
            for dx in 0..dock_w {
                let c_dx = if dx < corner_r { corner_r - dx - 1 } 
                           else if dx >= dock_w - corner_r { dx - (dock_w - corner_r) } 
                           else { 0 };
                let c_dy = if dy < corner_r { corner_r - dy - 1 } 
                           else if dy >= dock_h - corner_r { dy - (dock_h - corner_r) } 
                           else { 0 };
                
                let dist_sq = c_dx * c_dx + c_dy * c_dy;
                let r_sq = corner_r * corner_r;
                
                if dist_sq >= r_sq {
                    continue; 
                }

                let px = dock_x + dx;
                let py = dock_y + dy;

                let mut sum_r = 0usize;
                let mut sum_g = 0usize;
                let mut sum_b = 0usize;
                let mut count = 0usize;

                let start_y = py.saturating_sub(blur_r);
                let end_y = (py + blur_r).min(height - 1);
                let start_x = px.saturating_sub(blur_r);
                let end_x = (px + blur_r).min(width - 1);

                // Box Blur
                for by in start_y..=end_y {
                    for bx in start_x..=end_x {
                        let (r, g, b) = self.read_pixel(bx, by);
                        sum_r += r as usize;
                        sum_g += g as usize;
                        sum_b += b as usize;
                        count += 1;
                    }
                }

                let final_r = (sum_r / count) as u8;
                let final_g = (sum_g / count) as u8;
                let final_b = (sum_b / count) as u8;

                // Glass blend
                let inv_alpha = 256 - glass_alpha;
                let mut out_r = ((235u16 * glass_alpha + final_r as u16 * inv_alpha) >> 8) as u8;
                let mut out_g = ((240u16 * glass_alpha + final_g as u16 * inv_alpha) >> 8) as u8;
                let mut out_b = ((255u16 * glass_alpha + final_b as u16 * inv_alpha) >> 8) as u8;

                // Subtle inner border highlight
                if dist_sq >= (corner_r - 2) * (corner_r - 2) || dx <= 1 || dy <= 1 || dx >= dock_w - 2 || dy >= dock_h - 2 {
                    out_r = out_r.saturating_add(40);
                    out_g = out_g.saturating_add(40);
                    out_b = out_b.saturating_add(50);
                }

                let idx = (dy * dock_w + dx) * 3;
                self.dock_buffer[idx] = out_r;
                self.dock_buffer[idx+1] = out_g;
                self.dock_buffer[idx+2] = out_b;
            }
        }

        // Draw processed glass pixels back
        for dy in 0..dock_h {
            for dx in 0..dock_w {
                let c_dx = if dx < corner_r { corner_r - dx - 1 } 
                           else if dx >= dock_w - corner_r { dx - (dock_w - corner_r) } 
                           else { 0 };
                let c_dy = if dy < corner_r { corner_r - dy - 1 } 
                           else if dy >= dock_h - corner_r { dy - (dock_h - corner_r) } 
                           else { 0 };
                
                if c_dx * c_dx + c_dy * c_dy >= corner_r * corner_r {
                    continue; 
                }

                let idx = (dy * dock_w + dx) * 3;
                self.draw_pixel(dock_x + dx, dock_y + dy, self.dock_buffer[idx], self.dock_buffer[idx+1], self.dock_buffer[idx+2]);
            }
        }

        // Icons
        for i in 0..5 {
            let icon_x = dock_x + 25 + i * 85;
            let icon_y = dock_y + 10;
            
            // Icon Shadow
            self.draw_shadow_and_glow(icon_x, icon_y, 40, 40, 4, false);

            if i == 0 {
                self.draw_rect(icon_x, icon_y, 40, 40, 255, 255, 255);
                for line in 1..4 {
                    self.draw_rect(icon_x + 5, icon_y + line * 10, 30, 2, 100, 150, 255);
                }
            } else if i == 1 {
                self.draw_rect(icon_x, icon_y, 40, 40, 30, 30, 30);
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
                self.draw_shadow_and_glow(win_x, win_y, win_w, win_h, 8, is_active);
                
                // 2. Backup corners 
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
                window.app.update();
                window.app.draw(self, win_x, win_y + 20, win_w, window.height); // App content
                
                // 4. Restore Corners
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

        // Render Glass Dock after windows (floats above them, blurring windows behind it)
        self.render_glass_dock();

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
