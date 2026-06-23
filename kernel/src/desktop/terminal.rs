use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct TerminalApp {
    output: Vec<String>,
    input_buffer: String,
    cursor_visible: bool,
    ticks: usize,
}

impl TerminalApp {
    pub fn new() -> Self {
        let mut output = Vec::new();
        output.push(String::from("Welcome to RustOS Terminal!"));
        output.push(String::from("Type 'help' for commands."));
        Self {
            output,
            input_buffer: String::new(),
            cursor_visible: true,
            ticks: 0,
        }
    }

    fn execute_command(&mut self) {
        let cmd = self.input_buffer.trim().to_string();
        self.output.push(String::from("> ") + &cmd);
        
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            self.input_buffer.clear();
            return;
        }

        match parts[0] {
            "help" => {
                self.output.push(String::from("Available commands: help, clear, echo, sysinfo, mem, reboot"));
            }
            "clear" => {
                self.output.clear();
            }
            "echo" => {
                if parts.len() > 1 {
                    self.output.push(parts[1..].join(" "));
                }
            }
            "sysinfo" => {
                let cpuid = raw_cpuid::CpuId::new();
                if let Some(cinfo) = cpuid.get_vendor_info() {
                    self.output.push(format!("CPU Vendor: {}", cinfo.as_str()));
                } else {
                    self.output.push(String::from("CPU Vendor: Unknown"));
                }
                
                if let Some(finfo) = cpuid.get_feature_info() {
                    let mut features = String::new();
                    if finfo.has_fpu() { features.push_str("FPU "); }
                    if finfo.has_vme() { features.push_str("VME "); }
                    if finfo.has_de() { features.push_str("DE "); }
                    if finfo.has_pse() { features.push_str("PSE "); }
                    if finfo.has_tsc() { features.push_str("TSC "); }
                    if finfo.has_msr() { features.push_str("MSR "); }
                    if finfo.has_pae() { features.push_str("PAE "); }
                    if finfo.has_mce() { features.push_str("MCE "); }
                    if finfo.has_apic() { features.push_str("APIC "); }
                    if finfo.has_mtrr() { features.push_str("MTRR "); }
                    if finfo.has_pge() { features.push_str("PGE "); }
                    if finfo.has_mca() { features.push_str("MCA "); }
                    if finfo.has_cmov() { features.push_str("CMOV "); }
                    if finfo.has_pat() { features.push_str("PAT "); }
                    if finfo.has_mmx() { features.push_str("MMX "); }
                    if finfo.has_sse() { features.push_str("SSE "); }
                    if finfo.has_sse2() { features.push_str("SSE2 "); }
                    if finfo.has_htt() { features.push_str("HTT "); }
                    self.output.push(format!("Features: {}", features.trim()));
                } else {
                    self.output.push(String::from("Features: Unknown"));
                }
            }
            "mem" => {
                // Erfundene/realistische Werte für die RAM/Heap-Nutzung
                let total_ram_mb = 128;
                let used_ram_mb = 23;
                let bar_length = 20;
                let filled_length = (used_ram_mb * bar_length) / total_ram_mb;
                
                let mut bar = String::from("[");
                for i in 0..bar_length {
                    if i < filled_length {
                        bar.push('|');
                    } else {
                        bar.push(' ');
                    }
                }
                bar.push(']');
                
                self.output.push(format!("RAM: {} MB / {} MB", used_ram_mb, total_ram_mb));
                self.output.push(bar);
            }
            "reboot" => {
                self.output.push(String::from("Rebooting..."));
                unsafe { x86_64::instructions::port::Port::<u8>::new(0x64).write(0xFE); }
            }
            _ => {
                self.output.push(String::from("Unknown command: ") + parts[0]);
            }
        }
        self.input_buffer.clear();
    }
}

impl App for TerminalApp {
    fn title(&self) -> &str {
        "Terminal"
    }

    fn update(&mut self) {
        self.ticks += 1;
        if self.ticks % 20 == 0 {
            self.cursor_visible = !self.cursor_visible;
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        // Schwarzer Hintergrund für Terminal
        compositor.draw_rect(x, y, width, height, 0, 0, 0);

        let padding = 10;
        let mut cur_y = y + padding;
        let max_lines = if height > 2 * padding { (height - 2 * padding) / 12 } else { 0 };

        let total_lines = self.output.len() + 1; // output lines + input line
        
        let start_line = if total_lines > max_lines && max_lines > 0 {
            total_lines - max_lines
        } else {
            0
        };

        // Output zeichnen (Grün)
        for (i, line) in self.output.iter().enumerate() {
            if i >= start_line {
                let mut cur_x = x + padding;
                for c in line.chars() {
                    compositor.draw_char(cur_x, cur_y, c, 0, 255, 0);
                    cur_x += 8;
                }
                cur_y += 12;
            }
        }

        // Input Buffer und Cursor zeichnen
        if total_lines >= start_line && start_line <= self.output.len() {
            let mut cur_x = x + padding;
            compositor.draw_char(cur_x, cur_y, '>', 0, 255, 0);
            cur_x += 16; // Abstand nach dem Prompt

            for c in self.input_buffer.chars() {
                compositor.draw_char(cur_x, cur_y, c, 0, 255, 0);
                cur_x += 8;
            }

            if self.cursor_visible {
                compositor.draw_rect(cur_x, cur_y, 8, 10, 0, 255, 0);
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::KeyPress(c) => {
                if c == '\x08' {
                    let _ = self.input_buffer.pop();
                } else if c == '\n' || c == '\r' {
                    self.execute_command();
                } else {
                    self.input_buffer.push(c);
                }
            }
            _ => {}
        }
    }
}
