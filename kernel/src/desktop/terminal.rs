use crate::desktop::app::{App, Event};
use crate::desktop::compositor::GraphicalCompositor;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct TerminalApp {
    output: Vec<(String, (u8, u8, u8))>,
    input_buffer: String,
    cursor_visible: bool,
    ticks: usize,
    current_path: String,
}

impl TerminalApp {
    pub fn new() -> Self {
        let mut output = Vec::new();
        let default_color = (0, 255, 0); // Green
        output.push((String::from("Welcome to RustOS Terminal!"), default_color));
        output.push((String::from("Type 'help' for commands."), default_color));
        Self {
            output,
            input_buffer: String::new(),
            cursor_visible: true,
            ticks: 0,
            current_path: String::new(),
        }
    }

    fn print(&mut self, text: String, color: (u8, u8, u8)) {
        self.output.push((text, color));
    }

    fn execute_command(&mut self) {
        let cmd = self.input_buffer.trim().to_string();
        // Echo the executed command in white
        self.print(String::from("> ") + &cmd, (255, 255, 255));
        
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            self.input_buffer.clear();
            return;
        }

        let cmd_color = (0, 255, 0); // Green
        let err_color = (255, 0, 0); // Red

        match parts[0] {
            "help" => {
                self.print(String::from("Available commands: help, clear, echo, date, uname, sysinfo, mem, reboot, ls, touch, rm, cat, mkdir, cd, pwd"), cmd_color);
            }
            "clear" => {
                self.output.clear();
            }
            "pwd" => {
                let p = if self.current_path.is_empty() { "/" } else { &self.current_path };
                self.print(String::from(p), cmd_color);
            }
            "ls" => {
                let current = if self.current_path.is_empty() { "/" } else { &self.current_path };
                if let Some(files) = crate::fs::RAM_FS.lock().list_dir(current) {
                    if files.is_empty() {
                        self.print(String::from("(empty)"), cmd_color);
                    } else {
                        for (name, is_dir) in files {
                            let label = if is_dir { format!("[DIR] {}", name) } else { name };
                            self.print(label, cmd_color);
                        }
                    }
                } else {
                    self.print(String::from("Directory not found"), err_color);
                }
            }
            "mkdir" => {
                if parts.len() > 1 {
                    let path = if self.current_path.is_empty() {
                        String::from(parts[1])
                    } else {
                        format!("{}/{}", self.current_path, parts[1])
                    };
                    crate::fs::RAM_FS.lock().mkdir(&path);
                    self.print(String::from("Created directory"), cmd_color);
                } else {
                    self.print(String::from("Usage: mkdir <dirname>"), err_color);
                }
            }
            "cd" => {
                if parts.len() > 1 {
                    let target = parts[1];
                    if target == ".." {
                        if !self.current_path.is_empty() {
                            let mut p: Vec<&str> = self.current_path.split('/').collect();
                            p.pop();
                            self.current_path = p.join("/");
                        }
                    } else if target == "/" {
                        self.current_path = String::new();
                    } else {
                        let path = if self.current_path.is_empty() {
                            String::from(target)
                        } else {
                            format!("{}/{}", self.current_path, target)
                        };
                        
                        if let Some(_) = crate::fs::RAM_FS.lock().list_dir(&path) {
                            self.current_path = path;
                        } else {
                            self.print(String::from("Directory not found"), err_color);
                        }
                    }
                } else {
                    self.print(String::from("Usage: cd <dirname>"), err_color);
                }
            }
            "touch" => {
                if parts.len() > 1 {
                    let path = if self.current_path.is_empty() { String::from(parts[1]) } else { format!("{}/{}", self.current_path, parts[1]) };
                    crate::fs::RAM_FS.lock().write_file(&path, b"");
                    self.print(String::from("Created file"), cmd_color);
                } else {
                    self.print(String::from("Usage: touch <filename>"), err_color);
                }
            }
            "rm" => {
                if parts.len() > 1 {
                    let path = if self.current_path.is_empty() { String::from(parts[1]) } else { format!("{}/{}", self.current_path, parts[1]) };
                    if crate::fs::RAM_FS.lock().delete_file(&path) {
                        self.print(String::from("Deleted file"), cmd_color);
                    } else {
                        self.print(String::from("File not found"), err_color);
                    }
                } else {
                    self.print(String::from("Usage: rm <filename>"), err_color);
                }
            }
            "cat" => {
                if parts.len() > 1 {
                    let path = if self.current_path.is_empty() { String::from(parts[1]) } else { format!("{}/{}", self.current_path, parts[1]) };
                    if let Some(content) = crate::fs::RAM_FS.lock().read_file(&path) {
                        if let Ok(s) = core::str::from_utf8(&content) {
                            self.print(String::from(s), cmd_color);
                        } else {
                            self.print(String::from("(Binary data)"), err_color);
                        }
                    } else {
                        self.print(String::from("File not found"), err_color);
                    }
                } else {
                    self.print(String::from("Usage: cat <filename>"), err_color);
                }
            }
            "echo" => {
                if parts.len() > 1 {
                    if let Some(pos) = parts.iter().position(|&x| x == ">") {
                        if pos + 1 < parts.len() {
                            let filename = parts[pos + 1];
                            let path = if self.current_path.is_empty() { String::from(filename) } else { format!("{}/{}", self.current_path, filename) };
                            let content = parts[1..pos].join(" ");
                            crate::fs::RAM_FS.lock().write_file(&path, content.as_bytes());
                            self.print(String::from("File written"), cmd_color);
                            return;
                        }
                    }
                    self.print(parts[1..].join(" "), cmd_color);
                } else {
                    self.print(String::new(), cmd_color);
                }
            }
            "date" => {
                self.print(String::from("Tue Jun 23 10:58:18 UTC 2026 (Fake Time)"), cmd_color);
            }
            "uname" => {
                let mut sys_name = String::from("RustOS x86_64");
                if parts.len() > 1 && parts[1] == "-a" {
                    sys_name.push_str(" 1.0.0 Custom Kernel");
                }
                self.print(sys_name, cmd_color);
            }
            "sysinfo" => {
                let cpuid = raw_cpuid::CpuId::new();
                if let Some(cinfo) = cpuid.get_vendor_info() {
                    self.print(format!("CPU Vendor: {}", cinfo.as_str()), cmd_color);
                } else {
                    self.print(String::from("CPU Vendor: Unknown"), err_color);
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
                    self.print(format!("Features: {}", features.trim()), cmd_color);
                } else {
                    self.print(String::from("Features: Unknown"), err_color);
                }
            }
            "mem" => {
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
                
                self.print(format!("RAM: {} MB / {} MB", used_ram_mb, total_ram_mb), cmd_color);
                self.print(bar, cmd_color);
            }
            "reboot" => {
                self.print(String::from("Rebooting..."), cmd_color);
                unsafe { x86_64::instructions::port::Port::<u8>::new(0x64).write(0xFE); }
            }
            _ => {
                self.print(String::from("Unknown command: ") + parts[0], err_color);
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

        for (i, (line, color)) in self.output.iter().enumerate() {
            if i >= start_line {
                let mut cur_x = x + padding;
                for c in line.chars() {
                    if cur_x + 8 > x + width - padding {
                        break; // Truncate text that exceeds the window bounds
                    }
                    compositor.draw_char(cur_x, cur_y, c, color.0, color.1, color.2);
                    cur_x += 8;
                }
                cur_y += 12;
            }
        }

        if total_lines >= start_line && start_line <= self.output.len() {
            let mut cur_x = x + padding;
            if cur_x + 16 <= x + width - padding {
                compositor.draw_char(cur_x, cur_y, '>', 0, 255, 0);
                cur_x += 16;

                for c in self.input_buffer.chars() {
                    if cur_x + 8 > x + width - padding {
                        break;
                    }
                    compositor.draw_char(cur_x, cur_y, c, 255, 255, 255);
                    cur_x += 8;
                }

                if self.cursor_visible && cur_x + 8 <= x + width - padding {
                    compositor.draw_rect(cur_x, cur_y, 8, 10, 0, 255, 0);
                }
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
