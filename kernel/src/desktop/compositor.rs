use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use crate::desktop::window::Window;
use crate::desktop::app::Event;
use alloc::vec::Vec;
use font8x8::UnicodeFonts;

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

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
    pub long_press_ticks: usize,
    pub desktop_click_active: bool,
    pub active_app_click: Option<(usize, usize, usize)>,
    pub ticks: usize,
    pub dock_y_offset: isize,
    pub dock_target_offset: isize,
    pub dirty_rects: Vec<Rect>,
    pub full_redraw: bool,
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
            long_press_ticks: 0,
            desktop_click_active: false,
            active_app_click: None,
            ticks: 0,
            dock_y_offset: 0,
            dock_target_offset: 0,
            dirty_rects: Vec::new(),
            full_redraw: true,
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

    pub fn draw_rect_skew(&mut self, start_x: usize, start_y: usize, width: usize, height: usize, r: u8, g: u8, b: u8, skew: isize) {
        if skew == 0 {
            return self.draw_rect(start_x, start_y, width, height, r, g, b);
        }
        for y in 0..height {
            let shift = skew * (height as isize - y as isize) / height as isize;
            for x in 0..width {
                self.draw_pixel((start_x as isize + x as isize + shift).max(0) as usize, start_y + y, r, g, b);
            }
        }
    }

    pub fn draw_char_skew(&mut self, x: usize, y: usize, c: char, r: u8, g: u8, b: u8, skew: isize) {
        if skew == 0 {
            return self.draw_char(x, y, c, r, g, b);
        }
        if let Some(bitmap) = font8x8::BASIC_FONTS.get(c).or_else(|| font8x8::LATIN_FONTS.get(c)) {
            for (row, byte) in bitmap.iter().enumerate() {
                let shift = skew * (8 - row as isize) / 8;
                for col in 0..8 {
                    if (*byte & (1 << col)) != 0 {
                        self.draw_pixel((x as isize + col as isize + shift).max(0) as usize, y + row, r, g, b);
                    }
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

        let pulse = self.ticks % 100;
        let intensity_mod = if pulse < 50 { pulse } else { 100 - pulse };

        for py in ext_y..(ext_y + ext_h) {
            if py >= self.info.height { break; }
            
            // OPTIMIZATION: Skip the inner window pixels!
            // If we are between the top and bottom corners, we only need to render the left and right shadows.
            let is_middle_row = py >= y + corner_r && py < y + h - corner_r;
            
            let mut px = ext_x;
            while px < ext_x + ext_w {
                if px >= self.info.width { break; }
                
                if is_middle_row && px >= x && px < x + w {
                    // Jump straight to the right shadow!
                    px = x + w;
                    continue;
                }
                
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
                    if dist >= 0 && dist < shadow_size {
                        let alpha = 256 - (dist * 256 / shadow_size);
                        let alpha = alpha * alpha / 256; 
                        let intensity = if is_active { 
                            let base = (alpha * 180 / 256) as usize;
                            let breathe = (intensity_mod * 80 / 50) as usize;
                            base + breathe
                        } else { 
                            (alpha * 180 / 256) as usize
                        };
                        let intensity = intensity.min(255) as u16;
                        self.blend_pixel(px, py, sr, sg, sb, intensity);
                    }
                }
                px += 1;
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
        let dock_y_base = height.saturating_sub(dock_h + 15) as isize;
        let dock_y_calc = dock_y_base + self.dock_y_offset;
        
        if dock_y_calc >= height as isize {
            return;
        }
        let dock_y = (dock_y_base + self.dock_y_offset) as usize;
        let dock_x = (width as usize).saturating_sub(dock_w) / 2;
        let corner_r = 16usize; 
        
        let glass_alpha = 110u16; 

        let is_dock_hovered = self.mouse_x >= dock_x && self.mouse_x <= dock_x + dock_w 
                           && self.mouse_y >= dock_y && self.mouse_y <= dock_y + dock_h;
        
        let perimeter = (dock_w + dock_h) * 2;
        let glow_pos = (self.ticks * 8) % perimeter;

        self.draw_shadow_and_glow(dock_x, dock_y, dock_w, dock_h, corner_r, is_dock_hovered);

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

                let (final_r, final_g, final_b) = self.read_pixel(px, py);

                // Glass blend
                let inv_alpha = 256 - glass_alpha;
                let mut out_r = ((235u16 * glass_alpha + final_r as u16 * inv_alpha) >> 8) as u8;
                let mut out_g = ((240u16 * glass_alpha + final_g as u16 * inv_alpha) >> 8) as u8;
                let mut out_b = ((255u16 * glass_alpha + final_b as u16 * inv_alpha) >> 8) as u8;

                // Subtle inner border highlight
                let is_edge = dist_sq >= (corner_r - 2) * (corner_r - 2) || dx <= 1 || dy <= 1 || dx >= dock_w - 2 || dy >= dock_h - 2;
                if is_edge {
                    out_r = out_r.saturating_add(40);
                    out_g = out_g.saturating_add(40);
                    out_b = out_b.saturating_add(50);
                    
                    if is_dock_hovered {
                        let p_coord = if dy <= 2 { dx }
                                      else if dx >= dock_w - 3 { dock_w + dy }
                                      else if dy >= dock_h - 3 { dock_w + dock_h + dock_w.saturating_sub(dx) }
                                      else { dock_w * 2 + dock_h + dock_h.saturating_sub(dy) };
                        
                        let mut dist = (p_coord as isize - glow_pos as isize).abs() as usize;
                        dist = dist.min(perimeter.saturating_sub(dist));

                        if dist < 50 {
                            let intensity = (50 - dist) * 3;
                            out_r = out_r.saturating_add((intensity / 3) as u8);
                            out_g = out_g.saturating_add((intensity / 2) as u8);
                            out_b = out_b.saturating_add(intensity as u8);
                        }
                    }
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

        let mut hovered_icon = None;
        if is_dock_hovered && self.mouse_x >= dock_x + 25 {
            let rel_x = self.mouse_x - (dock_x + 25);
            let idx = rel_x / 85;
            let offset_in_icon = rel_x % 85;
            if offset_in_icon <= 40 && self.mouse_y >= dock_y + 10 && self.mouse_y <= dock_y + 50 {
                hovered_icon = Some(idx);
            }
        }

        // Icons
        for i in 0..5 {
            let icon_base_x = dock_x + 25 + i * 85;
            let mut icon_y = dock_y + 10;
            let is_hovered = hovered_icon == Some(i);
            let skew = if is_hovered { 6 } else { 0 };
            
            if is_hovered {
                icon_y = icon_y.saturating_sub(4);
            }

            self.draw_shadow_and_glow(icon_base_x, icon_y, 40, 40, 4, is_hovered);

            if i == 0 {
                self.draw_rect_skew(icon_base_x, icon_y, 40, 40, 255, 255, 255, skew);
                for line in 1..4 {
                    self.draw_rect_skew(icon_base_x + 5, icon_y + line * 10, 30, 2, 100, 150, 255, skew);
                }
            } else if i == 1 {
                self.draw_rect_skew(icon_base_x, icon_y, 40, 40, 30, 30, 30, skew);
                self.draw_char_skew(icon_base_x + 10, icon_y + 15, '>', 0, 255, 0, skew);
            } else if i == 2 {
                self.draw_rect_skew(icon_base_x, icon_y, 40, 40, 20, 20, 20, skew);
                self.draw_rect_skew(icon_base_x + 5, icon_y + 10, 30, 20, 0, 150, 255, skew);
            } else if i == 3 {
                self.draw_rect_skew(icon_base_x + 5, icon_y + 15, 30, 20, 100, 180, 255, skew);
                self.draw_rect_skew(icon_base_x + 5, icon_y + 10, 15, 5, 100, 180, 255, skew);
            } else {
                self.draw_rect_skew(icon_base_x, icon_y, 40, 40, 60 + (i as u8 * 30), 110 + (i as u8 * 20), 200, skew);
            }
        }
    }

    pub fn render_all(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);

        let mut any_maximized = false;
        for w in &self.windows {
            if let Some(win) = w {
                if win.is_maximized {
                    any_maximized = true;
                    break;
                }
            }
        }
        
        self.dock_target_offset = if any_maximized { 100 } else { 0 };
        let diff = self.dock_target_offset - self.dock_y_offset;
        self.dock_y_offset += diff / 4;
        if diff > 0 && diff < 4 { self.dock_y_offset += 1; }
        else if diff < 0 && diff > -4 { self.dock_y_offset -= 1; }

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
                
                // Close Button
                self.draw_rect(win_x + win_w - 20, win_y, 20, 20, 220, 50, 50); 
                self.draw_char(win_x + win_w - 14, win_y + 6, 'X', 255, 255, 255);
                
                // Maximize Button
                self.draw_rect(win_x + win_w - 40, win_y, 20, 20, 50, 150, 50); 
                self.draw_char(win_x + win_w - 34, win_y + 6, '^', 255, 255, 255);
                
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

        self.dirty_rects.push(Rect { x: self.mouse_x, y: self.mouse_y, width: 16, height: 16 });

        if self.full_redraw {
            self.framebuffer.copy_from_slice(&self.backbuffer);
            self.full_redraw = false;
            self.dirty_rects.clear();
        } else {
            let dock_w = 440;
            let dock_h = 60;
            let dock_x = (self.info.width.saturating_sub(dock_w)) / 2;
            let dock_y_base = (self.info.height as isize).saturating_sub(dock_h as isize + 15);
            let dock_y = (dock_y_base + self.dock_y_offset).max(0) as usize;
            self.dirty_rects.push(Rect { x: dock_x.saturating_sub(20), y: dock_y.saturating_sub(20), width: dock_w + 40, height: dock_h + 40 });

            for w in self.windows.iter().flatten() {
                self.dirty_rects.push(Rect { x: w.x.saturating_sub(20), y: w.y.saturating_sub(20), width: w.width + 40, height: w.height + 40 });
            }

            let bpp = self.info.bytes_per_pixel;
            let stride = self.info.stride;

            for rect in &self.dirty_rects {
                let start_y = rect.y;
                let end_y = core::cmp::min(rect.y + rect.height, self.info.height);
                let start_x = rect.x;
                let end_x = core::cmp::min(rect.x + rect.width, self.info.width);

                if start_x >= end_x || start_y >= end_y { continue; }

                for y in start_y..end_y {
                    let offset = y * stride * bpp + start_x * bpp;
                    let len = (end_x - start_x) * bpp;
                    self.framebuffer[offset..offset+len].copy_from_slice(&self.backbuffer[offset..offset+len]);
                }
            }
            self.dirty_rects.clear();
        }
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

    pub fn handle_mouse_event(&mut self, dx: i32, dy: i32, left_down: bool, _right_down: bool) {
        self.dirty_rects.push(Rect { x: self.mouse_x, y: self.mouse_y, width: 16, height: 16 });

        let mut new_x = self.mouse_x as i32 + dx;
        let mut new_y = self.mouse_y as i32 + dy;
        
        let width = self.info.width as i32;
        let height = self.info.height as i32;
        
        if new_x < 0 { new_x = 0; }
        if new_y < 0 { new_y = 0; }
        if new_x >= width { new_x = width - 1; }
        if new_y >= height { new_y = height - 1; }
        
        self.mouse_x = new_x as usize;
        self.mouse_y = new_y as usize;

        let clicked_this_frame = left_down && !self.mouse_left_down;
        let released_this_frame = !left_down && self.mouse_left_down;
        self.mouse_left_down = left_down;

        let mx = self.mouse_x;
        let my = self.mouse_y;

        if clicked_this_frame {
            self.full_redraw = true;
            let mut found = false;
            let mut clicked_idx = None;

            for i in (0..self.windows.len()).rev() {
                if let Some(w) = &self.windows[i] {
                    if mx >= w.x && mx <= w.x + w.width && my >= w.y && my <= w.y + w.height + 20 {
                        clicked_idx = Some(i);
                        break;
                    }
                }
            }

            if let Some(i) = clicked_idx {
                let win = self.windows.remove(i);
                self.windows.push(win);
                
                let new_i = self.windows.len() - 1;
                
                let mut close_win = false;
                let mut toggle_max = false;
                
                if let Some(w) = &self.windows[new_i] {
                    if my <= w.y + 20 && my >= w.y {
                        if mx >= w.x + w.width - 20 {
                            close_win = true;
                        } else if mx >= w.x + w.width - 40 && mx < w.x + w.width - 20 {
                            toggle_max = true;
                        } else {
                            if !w.is_maximized {
                                self.dragging_window = Some(new_i);
                                self.drag_offset_x = mx as isize - w.x as isize;
                                self.drag_offset_y = my as isize - w.y as isize;
                            }
                        }
                    } else if my > w.y + 20 && my <= w.y + w.height + 20 {
                        let rel_x = mx.saturating_sub(w.x);
                        let rel_y = my.saturating_sub(w.y + 20);
                        self.active_app_click = Some((new_i, rel_x, rel_y));
                        self.long_press_ticks = 0;
                    }
                }
                
                if close_win {
                    self.windows.remove(new_i);
                } else if toggle_max {
                    if let Some(w) = &mut self.windows[new_i] {
                        if w.is_maximized {
                            w.is_maximized = false;
                            w.x = w.orig_x;
                            w.y = w.orig_y;
                            w.width = w.orig_w;
                            w.height = w.orig_h;
                        } else {
                            w.is_maximized = true;
                            w.orig_x = w.x;
                            w.orig_y = w.y;
                            w.orig_w = w.width;
                            w.orig_h = w.height;
                            
                            w.x = 0;
                            w.y = 28;
                            w.width = width as usize;
                            w.height = (height as usize).saturating_sub(123);
                        }
                    }
                }
                found = true;
            }

            let dock_w = 440;
            let dock_h = 60;
            let dock_x = (width as usize).saturating_sub(dock_w) / 2;
            let dock_y_base = (height as isize).saturating_sub(dock_h as isize + 15);
            let dock_y = (dock_y_base + self.dock_y_offset) as usize;
            let in_dock = mx >= dock_x && mx <= dock_x + dock_w && my >= dock_y && my <= dock_y + dock_h;

            if !found {
                if in_dock {
                    if mx >= dock_x + 25 {
                        let rel_x = mx - (dock_x + 25);
                        let icon_idx = rel_x / 85;
                        let offset_in_icon = rel_x % 85;
                        if offset_in_icon <= 40 && self.windows.len() < 12 {
                            let offset = (self.windows.len() * 20) % 100;
                            if icon_idx == 0 {
                                let notepad_app = crate::desktop::notepad::NotepadApp::new();
                                let win_width = if width > 600 { width as usize - 200 } else { width as usize - 40 };
                                let win_height = if height > 400 { height as usize - 150 } else { height as usize - 80 };
                                let win_x = (width as usize - win_width) / 2 + offset;
                                let win_y = (height as usize - win_height) / 2 - 20 + offset;
                                let new_win = crate::desktop::window::Window::new(alloc::boxed::Box::new(notepad_app), win_x, win_y, win_width, win_height);
                                self.add_window(new_win);
                            } else if icon_idx == 1 {
                                let terminal_app = crate::desktop::terminal::TerminalApp::new();
                                let win_width = 500;
                                let win_height = 350;
                                let win_x = (width as usize - win_width) / 2 + offset;
                                let win_y = (height as usize - win_height) / 2 - 20 + offset;
                                let new_win = crate::desktop::window::Window::new(alloc::boxed::Box::new(terminal_app), win_x, win_y, win_width, win_height);
                                self.add_window(new_win);
                            } else if icon_idx == 2 {
                                let tm_app = crate::desktop::taskmanager::TaskManagerApp::new();
                                let win_width = 400;
                                let win_height = 250;
                                let win_x = (width as usize - win_width) / 2 + offset;
                                let win_y = (height as usize - win_height) / 2 - 20 + offset;
                                let new_win = crate::desktop::window::Window::new(alloc::boxed::Box::new(tm_app), win_x, win_y, win_width, win_height);
                                self.add_window(new_win);
                            } else if icon_idx == 3 {
                                let fm_app = crate::desktop::filemanager::FileManagerApp::new();
                                let win_width = 500;
                                let win_height = 400;
                                let win_x = (width as usize - win_width) / 2 + offset;
                                let win_y = (height as usize - win_height) / 2 - 20 + offset;
                                let new_win = crate::desktop::window::Window::new(alloc::boxed::Box::new(fm_app), win_x, win_y, win_width, win_height);
                                self.add_window(new_win);
                            }
                        }
                    }
                } else {
                    self.desktop_click_active = true;
                    self.long_press_ticks = 0;
                }
            }
        }

        if released_this_frame {
            self.dragging_window = None;
            self.desktop_click_active = false;
            
            if let Some((idx, rel_x, rel_y)) = self.active_app_click {
                if self.long_press_ticks < 60 {
                    if idx < self.windows.len() {
                        if let Some(w) = &mut self.windows[idx] {
                            w.app.handle_event(Event::MouseClick { x: rel_x, y: rel_y });
                        }
                    }
                }
            }
            self.active_app_click = None;
            self.long_press_ticks = 0;
        }

        if left_down {
            if let Some(idx) = self.dragging_window {
                self.full_redraw = true;
                if idx < self.windows.len() {
                    if let Some(w) = &mut self.windows[idx] {
                        let target_x = (self.mouse_x as isize - self.drag_offset_x).max(0) as usize;
                        let target_y = (self.mouse_y as isize - self.drag_offset_y).max(0) as usize;
                        w.x = target_x;
                        w.y = target_y;
                    }
                }
            } else if let Some((idx, rel_x, rel_y)) = self.active_app_click {
                let mut should_cancel = false;
                let mut trigger_event = false;
                
                if let Some(w) = &self.windows[idx] {
                    if mx < w.x || mx > w.x + w.width || my <= w.y + 20 || my > w.y + w.height + 20 {
                        should_cancel = true;
                    }
                }
                
                if should_cancel {
                    self.active_app_click = None;
                    self.long_press_ticks = 0;
                } else {
                    self.long_press_ticks = self.long_press_ticks.saturating_add(1);
                    if self.long_press_ticks == 60 {
                        trigger_event = true;
                    }
                }
                
                if trigger_event {
                    if let Some(w) = &mut self.windows[idx] {
                        w.app.handle_event(Event::MouseLongPress { x: rel_x, y: rel_y });
                    }
                }
            } else if self.desktop_click_active {
                // (Bestehender Code für Desktop Long Press bleibt hier bestehen)
                self.long_press_ticks = self.long_press_ticks.saturating_add(1);
                if self.long_press_ticks == 60 {
                    let file_name = alloc::format!("Desktop_Datei_{}.txt", self.ticks);
                    crate::fs::RAM_FS.lock().write_file(&file_name, b"");
                    
                    self.desktop_click_active = false;
                    self.long_press_ticks = 0;
                }
            }
        }
    }
}
