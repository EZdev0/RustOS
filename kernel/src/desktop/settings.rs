use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;

pub struct SettingsApp {
    active_tab: usize,
    timezone_offset: i8, // UTC offset in hours
    show_prompt: bool,
    pending_theme: u8,
}

impl Default for SettingsApp {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsApp {
    pub fn new() -> Self {
        Self { 
            active_tab: 0,
            timezone_offset: 1, // Default: UTC+1 (Germany)
            show_prompt: false,
            pending_theme: 0,
        }
    }
}

#[allow(clippy::many_single_char_names)]
fn draw_string(compositor: &mut GraphicalCompositor, mut x: usize, y: usize, s: &str, r: u8, g: u8, b: u8) {
    for c in s.chars() {
        compositor.draw_char(x, y, c, r, g, b);
        x += 8;
    }
}

impl App for SettingsApp {
    fn title(&self) -> &str {
        "System Settings"
    }

    fn update(&mut self) {
        // Here we could periodically request a redraw if we want a live clock
        // For now, moving the mouse over it triggers enough redraws.
    }

    fn handle_event(&mut self, event: Event) {
        if let Event::MouseClick { x, y } = event {
            let sidebar_width = 150;

            if self.show_prompt {
                let px = 100;
                let py = 100;
                let pw = 300;
                let ph = 120;
                if x >= px && x <= px + pw && y >= py && y <= py + ph {
                    // OK
                    if x >= px + 30 && x <= px + 130 && y >= py + 70 && y <= py + 100 {
                        crate::desktop::THEME_CHANGE_REQUESTED.store(self.pending_theme, core::sync::atomic::Ordering::Relaxed);
                        self.show_prompt = false;
                    }
                    // Cancel
                    if x >= px + 170 && x <= px + 270 && y >= py + 70 && y <= py + 100 {
                        self.show_prompt = false;
                    }
                }
                return;
            }

            if x < sidebar_width {
                // General Tab
                if (50..=80).contains(&y) {
                    self.active_tab = 0;
                }
                // Info Tab
                if (90..=120).contains(&y) {
                    self.active_tab = 1;
                }
                // Appearance Tab
                if (130..=160).contains(&y) {
                    self.active_tab = 2;
                }
            } else {
                let content_x = sidebar_width;
                if self.active_tab == 1 {
                    // Handle Timezone Click
                    if (content_x + 150..=content_x + 330).contains(&x) && (110..=140).contains(&y) {
                            // Toggle timezone offset between UTC (0) and Germany (+1) and Japan (+9)
                            self.timezone_offset = match self.timezone_offset {
                                0 => 1,
                                1 => 9,
                                _ => 0,
                            };
                    }
                } else if self.active_tab == 2 {
                    // Light Mode
                    if x >= content_x + 30 && x <= content_x + 150 && (80..=110).contains(&y) {
                        self.pending_theme = 1;
                        self.show_prompt = true;
                    }
                    // Dark Mode
                    if x >= content_x + 170 && x <= content_x + 290 && (80..=110).contains(&y) {
                        self.pending_theme = 2;
                        self.show_prompt = true;
                    }
                }
            }
        }
    }

    #[allow(clippy::many_single_char_names)]
    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        let sidebar_width = 150;
        let content_x = x + sidebar_width;
        let content_width = width.saturating_sub(sidebar_width);
        
        let is_dark = crate::desktop::THEME.load(core::sync::atomic::Ordering::Relaxed) == 1;
        let (bg_r, bg_g, bg_b) = if is_dark { (30, 30, 35) } else { (250, 250, 252) };
        let (text_r, text_g, text_b) = if is_dark { (220, 220, 220) } else { (40, 40, 50) };
        let (subtext_r, subtext_g, subtext_b) = if is_dark { (150, 150, 160) } else { (80, 80, 90) };
        let (line_r, line_g, line_b) = if is_dark { (50, 50, 60) } else { (220, 220, 230) };

        // 1. BACKGROUND & SIDEBAR
        compositor.draw_rect(content_x, y, content_width, height, bg_r, bg_g, bg_b);

        // Sidebar Gradient
        for i in 0..height {
            let (sr, sg, sb) = if is_dark {
                let val = 40 - (i as u32 * 10 / height as u32) as u8;
                (val, val, val + 5)
            } else {
                let r = 235 - (i as u32 * 15 / height as u32) as u8;
                let g = 235 - (i as u32 * 10 / height as u32) as u8;
                let b = 245 - (i as u32 * 10 / height as u32) as u8;
                (r, g, b)
            };
            compositor.draw_rect(x, y + i, sidebar_width, 1, sr, sg, sb);
        }
        
        compositor.draw_rect(x + sidebar_width - 1, y, 1, height, line_r, line_g, line_b);
        draw_string(compositor, x + 15, y + 20, "Settings", text_r, text_g, text_b);

        // 2. TABS & NAVIGATION
        // System Info Tab
        if self.active_tab == 0 {
            for i in 0..26 {
                let r = 210 - (i as u32 * 20 / 26) as u8;
                let g = 230 - (i as u32 * 10 / 26) as u8;
                compositor.draw_rect(x + 10, y + 50 + i, sidebar_width - 20, 1, r, g, 255);
            }
            compositor.draw_rect(x + 10, y + 50, 3, 26, 0, 120, 255);
            draw_string(compositor, x + 25, y + 59, "System Info", 0, 80, 200);
        } else {
            draw_string(compositor, x + 25, y + 59, "System Info", subtext_r, subtext_g, subtext_b);
        }

        // Date & Time Tab
        if self.active_tab == 1 {
            for i in 0..26 {
                let r = 210 - (i as u32 * 20 / 26) as u8;
                let g = 230 - (i as u32 * 10 / 26) as u8;
                compositor.draw_rect(x + 10, y + 90 + i, sidebar_width - 20, 1, r, g, 255);
            }
            compositor.draw_rect(x + 10, y + 90, 3, 26, 0, 120, 255);
            draw_string(compositor, x + 25, y + 99, "Date & Time", 0, 80, 200);
        } else {
            draw_string(compositor, x + 25, y + 99, "Date & Time", subtext_r, subtext_g, subtext_b);
        }

        // Appearance Tab
        if self.active_tab == 2 {
            for i in 0..26 {
                let r = 210 - (i as u32 * 20 / 26) as u8;
                let g = 230 - (i as u32 * 10 / 26) as u8;
                compositor.draw_rect(x + 10, y + 130 + i, sidebar_width - 20, 1, r, g, 255);
            }
            compositor.draw_rect(x + 10, y + 130, 3, 26, 0, 120, 255);
            draw_string(compositor, x + 25, y + 139, "Appearance", 0, 80, 200);
        } else {
            draw_string(compositor, x + 25, y + 139, "Appearance", subtext_r, subtext_g, subtext_b);
        }

        // 3. MAIN CONTENT AREA
        if self.active_tab == 0 {
            // SYSTEM INFO
            draw_string(compositor, content_x + 30, y + 30, "System Information", text_r, text_g, text_b);
            compositor.draw_rect(content_x + 30, y + 50, content_width.saturating_sub(60), 1, line_r, line_g, line_b);

            for i in 0..60 {
                let color_val = 150 + (i as u32 * 100 / 60) as u8;
                compositor.draw_rect(content_x + 30, y + 70 + i, 60, 1, 50, color_val, 255);
            }
            draw_string(compositor, content_x + 38, y + 95, "RustOS", 255, 255, 255);

            draw_string(compositor, content_x + 110, y + 75, "OS Name:      RustOS v0.2", subtext_r, subtext_g, subtext_b);
            draw_string(compositor, content_x + 110, y + 95, "Architecture: x86_64 Bare-Metal", subtext_r, subtext_g, subtext_b);
            draw_string(compositor, content_x + 110, y + 115,"Compiler:     Rust 1.96 Nightly", subtext_r, subtext_g, subtext_b);

            let (used, total) = crate::allocator::ALLOCATOR.get_memory_usage();
            let used_mb = used / (1024 * 1024);
            let total_mb = total / (1024 * 1024);
            
            let mut mem_str = alloc::string::String::new();
            use core::fmt::Write;
            let _ = write!(&mut mem_str, "Memory Used:  {} MB / {} MB", used_mb, total_mb);
            draw_string(compositor, content_x + 30, y + 160, &mem_str, subtext_r, subtext_g, subtext_b);
            
            let track_width = 300;
            let mut filled_width = (used * track_width).checked_div(total).unwrap_or(0);
            if filled_width > track_width { filled_width = track_width; }
            
            compositor.draw_rect(content_x + 30, y + 180, track_width, 10, line_r, line_g, line_b);
            compositor.draw_rect(content_x + 30, y + 180, filled_width, 10, 0, 200, 100);
            
        } else if self.active_tab == 1 {
            // DATE & TIME
            draw_string(compositor, content_x + 30, y + 30, "Date & Time", text_r, text_g, text_b);
            compositor.draw_rect(content_x + 30, y + 50, content_width.saturating_sub(60), 1, line_r, line_g, line_b);

            let mut rtc = crate::hardware::rtc::read_rtc();
            rtc.apply_timezone(self.timezone_offset);

            let mut time_str = alloc::string::String::new();
            use core::fmt::Write;
            let _ = write!(&mut time_str, "{:04}-{:02}-{:02} {:02}:{:02}:{:02}", 
                rtc.year, rtc.month, rtc.day, rtc.hour, rtc.minute, rtc.second);

            draw_string(compositor, content_x + 30, y + 80, "Current Time:", subtext_r, subtext_g, subtext_b);
            draw_string(compositor, content_x + 150, y + 80, &time_str, 0, 120, 255);

            draw_string(compositor, content_x + 30, y + 120, "Timezone:", subtext_r, subtext_g, subtext_b);
            
            let (btn_bg_r, btn_bg_g, btn_bg_b) = if is_dark { (40, 40, 45) } else { (245, 245, 250) };
            compositor.draw_rect(content_x + 150, y + 110, 180, 30, btn_bg_r, btn_bg_g, btn_bg_b);
            compositor.draw_rect(content_x + 150, y + 110, 180, 1, line_r, line_g, line_b);
            compositor.draw_rect(content_x + 150, y + 139, 180, 1, line_r, line_g, line_b);
            compositor.draw_rect(content_x + 150, y + 110, 1, 30, line_r, line_g, line_b);
            compositor.draw_rect(content_x + 329, y + 110, 1, 30, line_r, line_g, line_b);
            
            let tz_name = match self.timezone_offset {
                0 => "UTC",
                1 => "Germany (+1)",
                9 => "Japan (+9)",
                _ => "Custom",
            };
            draw_string(compositor, content_x + 160, y + 120, tz_name, text_r, text_g, text_b);
            draw_string(compositor, content_x + 310, y + 120, "v", subtext_r, subtext_g, subtext_b);
            
            draw_string(compositor, content_x + 150, y + 150, "(Click timezone box to change)", subtext_r, subtext_g, subtext_b);
        } else if self.active_tab == 2 {
            // APPEARANCE
            draw_string(compositor, content_x + 30, y + 30, "Appearance", text_r, text_g, text_b);
            compositor.draw_rect(content_x + 30, y + 50, content_width.saturating_sub(60), 1, line_r, line_g, line_b);

            let (btn_bg_r, btn_bg_g, btn_bg_b) = if is_dark { (60, 60, 65) } else { (220, 220, 225) };

            // Light Mode Btn
            compositor.draw_rect(content_x + 30, y + 80, 120, 30, btn_bg_r, btn_bg_g, btn_bg_b);
            draw_string(compositor, content_x + 45, y + 90, "Light Mode", text_r, text_g, text_b);

            // Dark Mode Btn
            compositor.draw_rect(content_x + 170, y + 80, 120, 30, btn_bg_r, btn_bg_g, btn_bg_b);
            draw_string(compositor, content_x + 185, y + 90, "Dark Mode", text_r, text_g, text_b);
        }

        // PROMPT DRAWING
        if self.show_prompt {
            let px = x + 100;
            let py = y + 100;
            let pw = 300;
            let ph = 120;
            
            compositor.draw_rect(px, py, pw, ph, bg_r, bg_g, bg_b);
            compositor.draw_rect(px, py, pw, 1, line_r, line_g, line_b);
            compositor.draw_rect(px, py + ph - 1, pw, 1, line_r, line_g, line_b);
            compositor.draw_rect(px, py, 1, ph, line_r, line_g, line_b);
            compositor.draw_rect(px + pw - 1, py, 1, ph, line_r, line_g, line_b);

            draw_string(compositor, px + 20, py + 20, "All open apps will be closed", text_r, text_g, text_b);
            draw_string(compositor, px + 20, py + 40, "to apply the theme.", text_r, text_g, text_b);

            let btn_bg_r = if is_dark { 60 } else { 220 };
            let btn_bg_g = if is_dark { 60 } else { 220 };
            let btn_bg_b = if is_dark { 65 } else { 225 };

            compositor.draw_rect(px + 30, py + 70, 100, 30, btn_bg_r, btn_bg_g, btn_bg_b);
            draw_string(compositor, px + 65, py + 80, "OK", text_r, text_g, text_b);

            compositor.draw_rect(px + 170, py + 70, 100, 30, btn_bg_r, btn_bg_g, btn_bg_b);
            draw_string(compositor, px + 195, py + 80, "Cancel", text_r, text_g, text_b);
        }
    }
}
