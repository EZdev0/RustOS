use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::string::String;

pub struct SettingsApp {
    active_tab: usize,
}

impl SettingsApp {
    pub fn new() -> Self {
        Self { active_tab: 0 }
    }
}

// Lokale Hilfsfunktion für weichere Schrift-Integration
fn draw_string(compositor: &mut GraphicalCompositor, mut x: usize, y: usize, s: &str, r: u8, g: u8, b: u8) {
    for c in s.chars() {
        compositor.draw_char(x, y, c, r, g, b);
        x += 8;
    }
}

impl App for SettingsApp {
    fn title(&self) -> &str {
        "Settings"
    }

    fn update(&mut self) {}

    fn handle_event(&mut self, event: Event) {
        if let Event::MouseClick { x, y } = event {
            let sidebar_width = 150;
            if x < sidebar_width {
                // Tab 1: System Info (Klick-Bereich)
                if y >= 50 && y <= 80 {
                    self.active_tab = 0;
                }
                // Tab 2: Date & Time (Klick-Bereich)
                if y >= 90 && y <= 120 {
                    self.active_tab = 1;
                }
            }
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        let sidebar_width = 150;
        
        // ==========================================
        // 1. BACKGROUND & SIDEBAR (Glass Look)
        // ==========================================
        // Content-Hintergrund (Clean Light Gray)
        let content_x = x + sidebar_width;
        let content_width = width.saturating_sub(sidebar_width);
        compositor.draw_rect(content_x, y, content_width, height, 250, 250, 252);

        // Sidebar Glass-Gradient (Subtiler Übergang von Hell-Graublau zu Tief-Graublau)
        for i in 0..height {
            let r = 235 - (i as u32 * 15 / height as u32) as u8;
            let g = 235 - (i as u32 * 10 / height as u32) as u8;
            let b = 245 - (i as u32 * 10 / height as u32) as u8;
            compositor.draw_rect(x, y + i, sidebar_width, 1, r, g, b);
        }
        
        // Trennlinie Sidebar zu Content
        compositor.draw_rect(x + sidebar_width - 1, y, 1, height, 200, 200, 210);

        // Titel der Sidebar
        draw_string(compositor, x + 15, y + 20, "Settings", 40, 40, 50);

        // ==========================================
        // 2. TABS & NAVIGATION
        // ==========================================
        // --- Tab 1: System Info ---
        if self.active_tab == 0 {
            // Aktiver Button mit Gradient
            for i in 0..26 {
                let r = 210 - (i as u32 * 20 / 26) as u8;
                let g = 230 - (i as u32 * 10 / 26) as u8;
                let b = 255;
                compositor.draw_rect(x + 10, y + 50 + i, sidebar_width - 20, 1, r, g, b);
            }
            // Linker blauer Akzent-Strich
            compositor.draw_rect(x + 10, y + 50, 3, 26, 0, 120, 255);
            draw_string(compositor, x + 25, y + 59, "System Info", 0, 80, 200);
        } else {
            draw_string(compositor, x + 25, y + 59, "System Info", 100, 100, 110);
        }

        // --- Tab 2: Date & Time ---
        if self.active_tab == 1 {
            for i in 0..26 {
                let r = 210 - (i as u32 * 20 / 26) as u8;
                let g = 230 - (i as u32 * 10 / 26) as u8;
                let b = 255;
                compositor.draw_rect(x + 10, y + 90 + i, sidebar_width - 20, 1, r, g, b);
            }
            compositor.draw_rect(x + 10, y + 90, 3, 26, 0, 120, 255);
            draw_string(compositor, x + 25, y + 99, "Date & Time", 0, 80, 200);
        } else {
            draw_string(compositor, x + 25, y + 99, "Date & Time", 100, 100, 110);
        }

        // ==========================================
        // 3. MAIN CONTENT AREA
        // ==========================================
        if self.active_tab == 0 {
            // Header System Info
            draw_string(compositor, content_x + 30, y + 30, "System Information", 40, 40, 50);
            compositor.draw_rect(content_x + 30, y + 50, content_width.saturating_sub(60), 1, 220, 220, 230); // Divider

            // OS Logo Mockup (Farbverlauf-Box)
            for i in 0..60 {
                let color_val = 150 + (i as u32 * 100 / 60) as u8;
                compositor.draw_rect(content_x + 30, y + 70 + i, 60, 1, 50, color_val, 255);
            }
            draw_string(compositor, content_x + 38, y + 95, "VibeOS", 255, 255, 255);

            // Informationen
            draw_string(compositor, content_x + 110, y + 75, "OS Name:    RustOS / VibeOS", 60, 60, 70);
            draw_string(compositor, content_x + 110, y + 95, "Memory:     16 GB Installed", 60, 60, 70);
            draw_string(compositor, content_x + 110, y + 115,"Status:     Online / Smooth", 60, 60, 70);

            // Status Bar Mockup
            draw_string(compositor, content_x + 30, y + 160, "System Load", 80, 80, 90);
            compositor.draw_rect(content_x + 30, y + 180, 200, 8, 220, 220, 230); // Track-Hintergrund
            compositor.draw_rect(content_x + 30, y + 180, 45, 8, 0, 200, 100);    // Track-Füllung (Grünlich)
            
        } else if self.active_tab == 1 {
            // Header Date & Time
            draw_string(compositor, content_x + 30, y + 30, "Date & Time", 40, 40, 50);
            compositor.draw_rect(content_x + 30, y + 50, content_width.saturating_sub(60), 1, 220, 220, 230);

            let rtc = crate::hardware::rtc::read_rtc();
            let mut time_str = String::new();
            use core::fmt::Write;
            let _ = write!(&mut time_str, "{:02}:{:02}:{:02} UTC", rtc.hour, rtc.minute, rtc.second);

            // Zeitanzeige
            draw_string(compositor, content_x + 30, y + 80, "Current Time:", 80, 80, 90);
            draw_string(compositor, content_x + 150, y + 80, &time_str, 0, 120, 255);

            // Dropdown Mockup (Länderauswahl)
            draw_string(compositor, content_x + 30, y + 120, "Timezone:", 80, 80, 90);
            
            // Rahmen und Form des Dropdowns
            compositor.draw_rect(content_x + 150, y + 110, 180, 30, 245, 245, 250);
            compositor.draw_rect(content_x + 150, y + 110, 180, 1, 200, 200, 210); // Oben
            compositor.draw_rect(content_x + 150, y + 139, 180, 1, 200, 200, 210); // Unten
            compositor.draw_rect(content_x + 150, y + 110, 1, 30, 200, 200, 210);  // Links
            compositor.draw_rect(content_x + 329, y + 110, 1, 30, 200, 200, 210);  // Rechts
            
            draw_string(compositor, content_x + 160, y + 120, "Germany (+1)", 50, 50, 60);
            draw_string(compositor, content_x + 310, y + 120, "v", 100, 100, 110);
        }
    }
}
