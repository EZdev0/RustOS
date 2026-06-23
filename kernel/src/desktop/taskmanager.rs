use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::format;
use alloc::vec::Vec;

pub struct TaskManagerApp {
    ticks: usize,
    cpu_history: Vec<u8>,
    ram_history: Vec<u8>,
}

impl TaskManagerApp {
    pub fn new() -> Self {
        Self {
            ticks: 0,
            cpu_history: alloc::vec![0; 50],
            ram_history: alloc::vec![0; 50],
        }
    }
}

impl App for TaskManagerApp {
    fn title(&self) -> &str {
        "Task Manager"
    }

    fn update(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks % 5 == 0 {
            let mut cpu_usage = (self.ticks % 100) as u8; 
            if cpu_usage > 50 { cpu_usage = 100 - cpu_usage; }
            cpu_usage = cpu_usage.saturating_add(10); // 10 to 60
            
            self.cpu_history.remove(0);
            self.cpu_history.push(cpu_usage);
            
            let ram_usage = 35 + (self.ticks % 15) as u8;
            self.ram_history.remove(0);
            self.ram_history.push(ram_usage);
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        compositor.draw_rect(x, y, width, height, 30, 30, 35);
        
        let title = "System Performance Monitor";
        let mut cx = x + 10;
        for c in title.chars() {
            compositor.draw_char(cx, y + 10, c, 255, 255, 255);
            cx += 8;
        }
        
        // Draw CPU Chart
        compositor.draw_rect(x + 10, y + 30, width - 20, 60, 40, 40, 45); // Chart bg
        let mut cur_x = x + 10;
        let chart_h = 60;
        for &val in self.cpu_history.iter() {
            let h = (val as usize * chart_h) / 100;
            compositor.draw_rect(cur_x, y + 30 + chart_h - h, 2, h, 0, 200, 255); // Cyan bar
            cur_x += 3;
            if cur_x >= x + width - 10 { break; }
        }
        
        let cpu_txt = format!("CPU Usage: {}%", self.cpu_history.last().unwrap_or(&0));
        let mut cx2 = x + 10;
        for c in cpu_txt.chars() {
            compositor.draw_char(cx2, y + 95, c, 0, 200, 255);
            cx2 += 8;
        }

        // Draw RAM Chart
        compositor.draw_rect(x + 10, y + 120, width - 20, 60, 40, 40, 45); // Chart bg
        let mut cur_x = x + 10;
        for &val in self.ram_history.iter() {
            let h = (val as usize * chart_h) / 100;
            compositor.draw_rect(cur_x, y + 120 + chart_h - h, 2, h, 255, 100, 0); // Orange bar
            cur_x += 3;
            if cur_x >= x + width - 10 { break; }
        }
        
        let ram_txt = format!("RAM Usage: {}MB / 128MB", self.ram_history.last().unwrap_or(&0));
        let mut cx3 = x + 10;
        for c in ram_txt.chars() {
            compositor.draw_char(cx3, y + 185, c, 255, 100, 0);
            cx3 += 8;
        }
        
        let hint = "Intelligent AI Background Processing Active.";
        let mut cx4 = x + 10;
        for c in hint.chars() {
            if cx4 + 8 > x + width - 10 { break; }
            compositor.draw_char(cx4, y + 215, c, 100, 255, 100);
            cx4 += 8;
        }
    }

    fn handle_event(&mut self, _event: Event) {
    }
}
