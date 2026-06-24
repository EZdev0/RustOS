use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::format;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq)]
enum AccordionSection {
    Performance,
    OsAnalysis,
    DeviceInfo,
    Support,
}

pub struct TaskManagerApp {
    ticks: usize,
    cpu_history: Vec<u8>,
    ram_history: Vec<u8>,
    expanded_section: Option<AccordionSection>,
    scroll_y: isize,
    last_width: usize,
    last_height: usize,
}

impl Default for TaskManagerApp {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskManagerApp {
    pub fn new() -> Self {
        Self {
            ticks: 0,
            cpu_history: alloc::vec![0; 50],
            ram_history: alloc::vec![0; 50],
            expanded_section: Some(AccordionSection::Performance),
            scroll_y: 0,
            last_width: 600,
            last_height: 400,
        }
    }

    fn get_content_height(section: AccordionSection) -> usize {
        match section {
            AccordionSection::Performance => 160,
            AccordionSection::OsAnalysis => 70,
            AccordionSection::DeviceInfo => 70,
            AccordionSection::Support => 60,
        }
    }

    fn draw_header(&self, compositor: &mut GraphicalCompositor, title: &str, x: usize, y: usize, width: usize, is_expanded: bool) -> usize {
        let bg_color = if is_expanded { (60, 60, 70) } else { (40, 40, 50) };
        compositor.draw_rect(x, y, width, 25, bg_color.0, bg_color.1, bg_color.2);
        
        let prefix = if is_expanded { "[-] " } else { "[+] " };
        let full_title = format!("{}{}", prefix, title);
        
        let mut cx = x + 10;
        for c in full_title.chars() {
            compositor.draw_char(cx, y + 8, c, 255, 255, 255);
            cx += 8;
        }
        
        25
    }

    fn draw_performance(&self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize) -> usize {
        let content_height = Self::get_content_height(AccordionSection::Performance);
        compositor.draw_rect(x, y, width, content_height, 35, 35, 40);

        let chart_h = 40;
        // Draw CPU Chart
        compositor.draw_rect(x + 5, y + 10, width - 10, chart_h, 40, 40, 45);
        let mut cur_x = x + 5;
        for &val in self.cpu_history.iter() {
            let h = (val as usize * chart_h) / 100;
            compositor.draw_rect(cur_x, y + 10 + chart_h - h, 2, h, 0, 200, 255);
            cur_x += 3;
            if cur_x >= x + width - 10 { break; }
        }
        
        let cpu_txt = format!("CPU Usage: {}%", self.cpu_history.last().unwrap_or(&0));
        let mut cx2 = x + 5;
        for c in cpu_txt.chars() {
            if cx2 + 8 > x + width { break; }
            compositor.draw_char(cx2, y + 55, c, 0, 200, 255);
            cx2 += 8;
        }

        // Draw RAM Chart
        compositor.draw_rect(x + 5, y + 80, width - 10, chart_h, 40, 40, 45);
        let mut cur_x = x + 5;
        for &val in self.ram_history.iter() {
            let h = (val as usize * chart_h) / 100;
            compositor.draw_rect(cur_x, y + 80 + chart_h - h, 2, h, 255, 100, 0);
            cur_x += 3;
            if cur_x >= x + width - 10 { break; }
        }
        
        let ram_txt = format!("RAM Usage: {}MB / 128MB", self.ram_history.last().unwrap_or(&0));
        let mut cx3 = x + 5;
        for c in ram_txt.chars() {
            if cx3 + 8 > x + width { break; }
            compositor.draw_char(cx3, y + 125, c, 255, 100, 0);
            cx3 += 8;
        }

        content_height
    }

    fn draw_os_analysis(&self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize) -> usize {
        let content_height = Self::get_content_height(AccordionSection::OsAnalysis);
        compositor.draw_rect(x, y, width, content_height, 35, 35, 40);

        let uptime = self.ticks / 60;
        let lines = [
            alloc::string::String::from("Kernel: RustOS v0.2"),
            alloc::string::String::from("System Status: Nominal"),
            format!("Uptime: {}s", uptime),
            alloc::string::String::from("Analysis: 0 errors found"),
        ];

        for (i, line) in lines.iter().enumerate() {
            let mut cx = x + 10;
            for c in line.chars() {
                if cx + 8 > x + width { break; }
                compositor.draw_char(cx, y + 10 + (i * 15), c, 100, 255, 100);
                cx += 8;
            }
        }
        content_height
    }

    fn draw_device_info(&self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize) -> usize {
        let content_height = Self::get_content_height(AccordionSection::DeviceInfo);
        compositor.draw_rect(x, y, width, content_height, 35, 35, 40);

        let network_speed = 10 + (self.ticks % 27);
        let lines = [
            alloc::string::String::from("Architecture: x86_64"),
            format!("CPU Ticks: {}", self.ticks),
            format!("Network: {} kbps", network_speed),
            alloc::string::String::from("RAM: 128 MB Installed"),
        ];

        for (i, line) in lines.iter().enumerate() {
            let mut cx = x + 10;
            for c in line.chars() {
                if cx + 8 > x + width { break; }
                compositor.draw_char(cx, y + 10 + (i * 15), c, 200, 200, 200);
                cx += 8;
            }
        }
        content_height
    }

    fn draw_support(&self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize) -> usize {
        let content_height = Self::get_content_height(AccordionSection::Support);
        compositor.draw_rect(x, y, width, content_height, 35, 35, 40);

        let lines = [
            "Need help with RustOS?",
            "Contact: support@rustos",
            "Docs: /docs/system.txt",
        ];

        for (i, line) in lines.iter().enumerate() {
            let mut cx = x + 10;
            for c in line.chars() {
                if cx + 8 > x + width { break; }
                compositor.draw_char(cx, y + 10 + (i * 15), c, 255, 200, 100);
                cx += 8;
            }
        }
        content_height
    }
}

impl App for TaskManagerApp {
    fn title(&self) -> &str {
        "Task Manager"
    }

    fn update(&mut self) {
        self.ticks += 1;
        if self.ticks.is_multiple_of(20) {
            let mut cpu_usage = (self.ticks % 83) as u8; 
            if cpu_usage > 50 { cpu_usage = 100 - cpu_usage; }
            cpu_usage = cpu_usage.saturating_add(15);
            
            self.cpu_history.remove(0);
            self.cpu_history.push(cpu_usage);
            
            let ram_usage = 30 + ((self.ticks / 2) % 20) as u8;
            self.ram_history.remove(0);
            self.ram_history.push(ram_usage);
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        self.last_width = width;
        self.last_height = height;
        let mut total_content_height = 5isize;
        let sections = [
            AccordionSection::Performance,
            AccordionSection::OsAnalysis,
            AccordionSection::DeviceInfo,
            AccordionSection::Support,
        ];
        for section in sections.iter() {
            total_content_height += 25; // header
            if self.expanded_section == Some(*section) {
                total_content_height += Self::get_content_height(*section) as isize;
            }
            total_content_height += 5; // spacing
        }
        let max_scroll = (total_content_height - height as isize).max(0);
        self.scroll_y = self.scroll_y.clamp(0, max_scroll);
        
        compositor.draw_rect(x, y, width, height, 30, 30, 35);
        
        let sections = [
            (AccordionSection::Performance, "Performance Monitor"),
            (AccordionSection::OsAnalysis, "OS Analysis"),
            (AccordionSection::DeviceInfo, "Device Info"),
            (AccordionSection::Support, "Support"),
        ];

        let content_width = width;

        let mut current_y = y as isize + 5 - self.scroll_y;
        
        for (section, title) in sections.iter() {
            let is_expanded = self.expanded_section == Some(*section);
            
            // Only draw header if it's within bounds or just let clipping handle it
            let header_height = if current_y > y as isize - 25 && current_y < (y + height) as isize {
                self.draw_header(compositor, title, x + 5, current_y as usize, content_width - 10, is_expanded) as isize
            } else {
                25 // Assume fixed height for header
            };
            
            current_y += header_height;

            if is_expanded {
                let content_height = match section {
                    AccordionSection::Performance => {
                        if current_y > y as isize - 160 && current_y < (y + height) as isize {
                            self.draw_performance(compositor, x + 5, current_y as usize, content_width - 10) as isize
                        } else { 160 }
                    },
                    AccordionSection::OsAnalysis => {
                        if current_y > y as isize - 70 && current_y < (y + height) as isize {
                            self.draw_os_analysis(compositor, x + 5, current_y as usize, content_width - 10) as isize
                        } else { 70 }
                    },
                    AccordionSection::DeviceInfo => {
                        if current_y > y as isize - 70 && current_y < (y + height) as isize {
                            self.draw_device_info(compositor, x + 5, current_y as usize, content_width - 10) as isize
                        } else { 70 }
                    },
                    AccordionSection::Support => {
                        if current_y > y as isize - 60 && current_y < (y + height) as isize {
                            self.draw_support(compositor, x + 5, current_y as usize, content_width - 10) as isize
                        } else { 60 }
                    },
                };
                current_y += content_height;
            }
            
            current_y += 5; // Spacing
        }

        compositor.draw_scrollbar(x + width - 10, y, height, self.scroll_y as usize, max_scroll as usize);
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::MouseClick { x: rel_x, y: rel_y } => {
                let mut total_content_height = 5isize;
                let sections = [
                    AccordionSection::Performance,
                    AccordionSection::OsAnalysis,
                    AccordionSection::DeviceInfo,
                    AccordionSection::Support,
                ];
                for section in sections.iter() {
                    total_content_height += 25; // header
                    if self.expanded_section == Some(*section) {
                        total_content_height += Self::get_content_height(*section) as isize;
                    }
                    total_content_height += 5; // spacing
                }
                let max_scroll = (total_content_height - self.last_height as isize).max(0);
                if max_scroll > 0 && rel_x >= self.last_width.saturating_sub(10) { 
                    self.scroll_y = (rel_y as isize * max_scroll) / self.last_height as isize;
                    return;
                }

                let adjusted_y = rel_y as isize + self.scroll_y;
                let mut current_y = 5isize;
                
                for section in sections.iter() {
                    // Prüfen ob Klick im Header (25px Höhe) landete
                    if adjusted_y >= current_y && adjusted_y < current_y + 25 {
                        if self.expanded_section == Some(*section) {
                            self.expanded_section = None;
                        } else {
                            self.expanded_section = Some(*section);
                        }
                        break;
                    }
                    current_y += 25;
                    
                    if self.expanded_section == Some(*section) {
                        current_y += Self::get_content_height(*section) as isize;
                    }
                    current_y += 5; // Spacing
                }
            },
            Event::MouseScroll { delta } => {
                self.scroll_y += (delta as isize) * 20; // Scroll speed
                if self.scroll_y < 0 {
                    self.scroll_y = 0;
                }
                
                // Wir könnten auch den maximalen Scroll berechnen, aber 0-Begrenzung ist fürs Erste ok.
            },
            _ => {}
        }
    }
}
