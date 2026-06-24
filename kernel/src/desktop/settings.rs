use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::String;

pub struct SettingsApp {
    active_tab: usize,
    timezone_offset: i8, // UTC offset in hours
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
            if x < sidebar_width {
                // General Tab
                if (50..=80).contains(&y) {
                    self.active_tab = 0;
                }
                // Info Tab
                if (90..=120).contains(&y) {
                    self.active_tab = 1;
                }
            } else if self.active_tab == 1 {
                // Handle Timezone Click
                let content_x = sidebar_width;
                if (content_x + 150..=content_x + 330).contains(&x) && (110..=140).contains(&y) {
                        // Toggle timezone offset between UTC (0) and Germany (+1) and Japan (+9)
                        self.timezone_offset = match self.timezone_offset {
                            0 => 1,
                            1 => 9,
                            _ => 0,
                        };
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
        
        // 1. BACKGROUND & SIDEBAR
        compositor.draw_rect(content_x, y, content_width, height, 250, 250, 252);

        // Sidebar Gradient
        for i in 0..height {
            let r = 235 - (i as u32 * 15 / height as u32) as u8;
            let g = 235 - (i as u32 * 10 / height as u32) as u8;
            let b = 245 - (i as u32 * 10 / height as u32) as u8;
            compositor.draw_rect(x, y + i, sidebar_width, 1, r, g, b);
        }
        
        compositor.draw_rect(x + sidebar_width - 1, y, 1, height, 200, 200, 210);
        draw_string(compositor, x + 15, y + 20, "Settings", 40, 40, 50);

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
            draw_string(compositor, x + 25, y + 59, "System Info", 100, 100, 110);
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
            draw_string(compositor, x + 25, y + 99, "Date & Time", 100, 100, 110);
        }

        // 3. MAIN CONTENT AREA
        if self.active_tab == 0 {
            // SYSTEM INFO
            draw_string(compositor, content_x + 30, y + 30, "System Information", 40, 40, 50);
            compositor.draw_rect(content_x + 30, y + 50, content_width.saturating_sub(60), 1, 220, 220, 230);

            for i in 0..60 {
                let color_val = 150 + (i as u32 * 100 / 60) as u8;
                compositor.draw_rect(content_x + 30, y + 70 + i, 60, 1, 50, color_val, 255);
            }
            draw_string(compositor, content_x + 38, y + 95, "RustOS", 255, 255, 255);

            draw_string(compositor, content_x + 110, y + 75, "OS Name:      RustOS v0.2", 60, 60, 70);
            draw_string(compositor, content_x + 110, y + 95, "Architecture: x86_64 Bare-Metal", 60, 60, 70);
            draw_string(compositor, content_x + 110, y + 115,"Compiler:     Rust 1.96 Nightly", 60, 60, 70);

            // Fetch live RAM info
            let (used, total) = crate::allocator::ALLOCATOR.get_memory_usage();
            let used_mb = used / (1024 * 1024);
            let total_mb = total / (1024 * 1024);
            
            let mut mem_str = String::new();
            use core::fmt::Write;
            let _ = write!(&mut mem_str, "Memory Used:  {} MB / {} MB", used_mb, total_mb);
            draw_string(compositor, content_x + 30, y + 160, &mem_str, 80, 80, 90);
            
            // Dynamic Progress Bar
            let track_width = 300;
            let mut filled_width = (used * track_width).checked_div(total).unwrap_or(0);
            if filled_width > track_width { filled_width = track_width; }
            
            compositor.draw_rect(content_x + 30, y + 180, track_width, 10, 220, 220, 230); // Track
            compositor.draw_rect(content_x + 30, y + 180, filled_width, 10, 0, 200, 100);  // Fill
            
        } else if self.active_tab == 1 {
            // DATE & TIME
            draw_string(compositor, content_x + 30, y + 30, "Date & Time", 40, 40, 50);
            compositor.draw_rect(content_x + 30, y + 50, content_width.saturating_sub(60), 1, 220, 220, 230);

            let mut rtc = crate::hardware::rtc::read_rtc();
            rtc.apply_timezone(self.timezone_offset);

            let mut time_str = String::new();
            use core::fmt::Write;
            let _ = write!(&mut time_str, "{:04}-{:02}-{:02} {:02}:{:02}:{:02}", 
                rtc.year, rtc.month, rtc.day, rtc.hour, rtc.minute, rtc.second);

            draw_string(compositor, content_x + 30, y + 80, "Current Time:", 80, 80, 90);
            draw_string(compositor, content_x + 150, y + 80, &time_str, 0, 120, 255);

            draw_string(compositor, content_x + 30, y + 120, "Timezone:", 80, 80, 90);
            
            // Dropdown Button
            compositor.draw_rect(content_x + 150, y + 110, 180, 30, 245, 245, 250);
            compositor.draw_rect(content_x + 150, y + 110, 180, 1, 200, 200, 210);
            compositor.draw_rect(content_x + 150, y + 139, 180, 1, 200, 200, 210);
            compositor.draw_rect(content_x + 150, y + 110, 1, 30, 200, 200, 210);
            compositor.draw_rect(content_x + 329, y + 110, 1, 30, 200, 200, 210);
            
            let tz_name = match self.timezone_offset {
                0 => "UTC",
                1 => "Germany (+1)",
                9 => "Japan (+9)",
                _ => "Custom",
            };
            draw_string(compositor, content_x + 160, y + 120, tz_name, 50, 50, 60);
            draw_string(compositor, content_x + 310, y + 120, "v", 100, 100, 110);
            
            draw_string(compositor, content_x + 150, y + 150, "(Click timezone box to change)", 150, 150, 150);
        }
    }
}
