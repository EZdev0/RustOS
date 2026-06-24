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
    ping_state: Option<([u8; 4], u16)>,
    ping_delay: usize,
    scroll_y: isize,
}

impl Default for TerminalApp {
    fn default() -> Self {
        Self::new()
    }
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
            ping_state: None,
            ping_delay: 0,
            scroll_y: -1, // -1 means auto-scroll to bottom
        }
    }

    fn print(&mut self, text: String, color: (u8, u8, u8)) {
        self.output.push((text, color));
        if self.output.len() > 500 {
            self.output.remove(0);
        }
        self.scroll_y = -1; // Auto-scroll to bottom on new output
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
                if parts.len() > 1 {
                    match parts[1] {
                        "ping" => {
                            self.print(String::from("Command: ping"), (0, 200, 255));
                            self.print(String::from("Description: Sends ICMP ECHO_REQUEST packets to network hosts to check connectivity."), cmd_color);
                            self.print(String::from("Syntax: ping <ip_address>"), cmd_color);
                            self.print(String::from("Example: ping 8.8.8.8"), cmd_color);
                            self.print(String::from("Note: Press Ctrl+C to cancel an ongoing ping."), (200, 200, 200));
                        }
                        "sysfetch" => {
                            self.print(String::from("Command: sysfetch"), (0, 200, 255));
                            self.print(String::from("Description: Displays a cool ASCII logo and system information."), cmd_color);
                        }
                        _ => {
                            self.print(format!("No detailed help available for '{}'.", parts[1]), err_color);
                        }
                    }
                } else {
                    self.print(String::from("RustOS v0.2 - Available Commands"), (0, 200, 255));
                    self.print(String::from("Type 'help <command>' for specific details."), (200, 200, 200));
                    self.print(String::from(""), cmd_color);
                    self.print(String::from("SYSTEM     sysinfo, uname, mem, sysfetch, date, reboot, clear"), cmd_color);
                    self.print(String::from("NETWORK    ping"), cmd_color);
                    self.print(String::from("FILE       ls, cd, pwd, cat, touch, mkdir, rm, echo"), cmd_color);
                }
            }
            "sysfetch" => {
                self.print(String::from("   ____           _    ___  ____  "), (0, 200, 255));
                self.print(String::from("  |  _ \\ _   _ __| |_ / _ \\/ ___| "), (0, 200, 255));
                self.print(String::from("  | |_) | | | / __| __| | | \\___ \\"), (0, 200, 255));
                self.print(String::from("  |  _ <| |_| \\__ \\ |_| |_| |___) |"), (0, 200, 255));
                self.print(String::from("  |_| \\_\\\\__,_|___/\\__|\\___/|____/ "), (0, 200, 255));
                self.print(String::from(""), cmd_color);
                self.print(String::from("  OS: RustOS v0.2 Bare-Metal"), cmd_color);
                self.print(String::from("  Arch: x86_64"), cmd_color);
                self.print(format!("  Uptime: {} ticks", self.ticks), cmd_color);
                self.print(String::from("  RAM: 128 MB (Emulated)"), cmd_color);
                self.print(String::from("  Shell: RustTerm 1.0"), cmd_color);
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
                if let Ok(files) = crate::fs::RAM_FS.list_dir(current) {
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
                    let _ = crate::fs::RAM_FS.mkdir(&path);
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
                        
                        if crate::fs::RAM_FS.list_dir(&path).is_ok() {
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
                    let _ = crate::fs::RAM_FS.write_file(&path, b"");
                    self.print(String::from("Created file"), cmd_color);
                } else {
                    self.print(String::from("Usage: touch <filename>"), err_color);
                }
            }
            "rm" => {
                if parts.len() > 1 {
                    let path = if self.current_path.is_empty() { String::from(parts[1]) } else { format!("{}/{}", self.current_path, parts[1]) };
                    if crate::fs::RAM_FS.delete_file(&path).is_ok() {
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
                    if let Ok(content) = crate::fs::RAM_FS.read_file(&path) {
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
                            let _ = crate::fs::RAM_FS.write_file(&path, content.as_bytes());
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
                let rtc = crate::hardware::rtc::read_rtc();
                self.print(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", 
                    rtc.year, rtc.month, rtc.day, rtc.hour, rtc.minute, rtc.second), cmd_color);
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
            "ping" => {
                if parts.len() > 1 {
                    let target_ip = parts[1];
                    let ip_parts: Vec<&str> = target_ip.split('.').collect();
                    if ip_parts.len() == 4 {
                        if let (Ok(a), Ok(b), Ok(c), Ok(d)) = (ip_parts[0].parse::<u8>(), ip_parts[1].parse::<u8>(), ip_parts[2].parse::<u8>(), ip_parts[3].parse::<u8>()) {
                            let ip = [a, b, c, d];
                            self.print(format!("PING {} (32 data bytes)", target_ip), cmd_color);
                            self.ping_state = Some((ip, 1));
                            self.ping_delay = 0;
                        } else {
                            self.print(String::from("Invalid IP format"), err_color);
                        }
                    } else {
                        self.print(String::from("Invalid IP format"), err_color);
                    }
                } else {
                    self.print(String::from("Usage: ping <ip_address>"), err_color);
                }
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
        if self.ticks.is_multiple_of(30) {
            self.cursor_visible = !self.cursor_visible;
        }

        if let Some((ip, seq)) = self.ping_state {
            if self.ping_delay == 0 {
                // Send Ping
                if let Some(ref mut nm) = *crate::network::NETWORK_MANAGER.lock() {
                    let _ = nm.send_ping(ip, seq);
                }
                self.ping_delay = 60; // wait ~1 second
            } else {
                self.ping_delay -= 1;
            }

            // Check reply
            if let Some(ref mut nm) = *crate::network::NETWORK_MANAGER.lock() {
                if let Some((_, reply_seq)) = nm.ping_reply.take() {
                    self.print(format!("32 bytes from {}.{}.{}.{}: icmp_seq={} time=1ms", 
                        ip[0], ip[1], ip[2], ip[3], reply_seq), (200, 200, 200));
                    self.ping_state = Some((ip, seq + 1));
                    self.ping_delay = 60; // reset delay after reply
                }
            }
            
            if seq >= 5 { // Auto-stop after 4 pings for demo
                self.print(String::from("--- ping statistics ---"), (0, 255, 0));
                self.print("4 packets transmitted, 4 received, 0% packet loss".to_string(), (0, 200, 255));
                self.ping_state = None;
            }
        }
    }

    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize) {
        compositor.draw_rect(x, y, width, height, 0, 0, 0);

        let padding = 10;
        let max_lines = if height > 2 * padding { (height - 2 * padding) / 12 } else { 0 };
        let chars_per_line = if width > 2 * padding { (width - 2 * padding) / 8 } else { 1 };

        let mut visual_lines = 0;
        for (line, _) in &self.output {
            let len = line.chars().count();
            visual_lines += if len == 0 { 1 } else { len.div_ceil(chars_per_line) };
        }
        
        let input_len = 2 + self.input_buffer.chars().count();
        visual_lines += if input_len == 0 { 1 } else { input_len.div_ceil(chars_per_line) };

        let max_scroll = if visual_lines > max_lines && max_lines > 0 {
            visual_lines - max_lines
        } else {
            0
        };

        if self.scroll_y == -1 {
            self.scroll_y = max_scroll as isize;
        } else {
            self.scroll_y = self.scroll_y.clamp(0, max_scroll as isize);
        }

        let start_line = self.scroll_y as usize;
        let mut current_visual_line = 0;
        let content_width = width; // Draw scrollbar as an overlay

        for (line, color) in &self.output {
            let mut cur_x = x + padding;
            let mut line_empty = true;
            for c in line.chars() {
                line_empty = false;
                if cur_x + 8 > x + content_width - padding {
                    current_visual_line += 1;
                    cur_x = x + padding;
                }
                if current_visual_line >= start_line {
                    let cur_y = y + padding + (current_visual_line - start_line) * 12;
                    if cur_y + 12 <= y + height - padding {
                        compositor.draw_char(cur_x, cur_y, c, color.0, color.1, color.2);
                    }
                }
                cur_x += 8;
            }
            if line_empty && current_visual_line >= start_line {
                // Empty line still takes space vertically
            }
            current_visual_line += 1;
        }

        // Draw input
        let mut cur_x = x + padding;
        
        let prompt = "> ";
        for c in prompt.chars() {
            if cur_x + 8 > x + content_width - padding {
                current_visual_line += 1;
                cur_x = x + padding;
            }
            if current_visual_line >= start_line {
                let cur_y = y + padding + (current_visual_line - start_line) * 12;
                if cur_y + 12 <= y + height - padding {
                    compositor.draw_char(cur_x, cur_y, c, 0, 255, 0);
                }
            }
            cur_x += 8;
        }

        for c in self.input_buffer.chars() {
            if cur_x + 8 > x + content_width - padding {
                current_visual_line += 1;
                cur_x = x + padding;
            }
            if current_visual_line >= start_line {
                let cur_y = y + padding + (current_visual_line - start_line) * 12;
                if cur_y + 12 <= y + height - padding {
                    compositor.draw_char(cur_x, cur_y, c, 255, 255, 255);
                }
            }
            cur_x += 8;
        }

        if self.cursor_visible {
            if cur_x + 8 > x + content_width - padding {
                current_visual_line += 1;
                cur_x = x + padding;
            }
            if current_visual_line >= start_line {
                let cur_y = y + padding + (current_visual_line - start_line) * 12;
                if cur_y + 12 <= y + height - padding {
                    compositor.draw_rect(cur_x, cur_y, 8, 10, 0, 255, 0);
                }
            }
        }

        compositor.draw_scrollbar(x + width - 10, y, height, self.scroll_y as usize, max_scroll);
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::KeyPress(c) => {
                if c == '\x03' { // Ctrl+C to cancel ping
                    if self.ping_state.is_some() {
                        self.print(String::from("Ping canceled."), (255, 255, 0));
                        self.ping_state = None;
                    }
                    return;
                }
                if self.ping_state.is_some() {
                    return; // Ignore typing while ping is running
                }
                if c == '\n' || c == '\r' {
                    self.execute_command();
                } else if c == '\x08' {
                    let _ = self.input_buffer.pop();
                } else {
                    self.input_buffer.push(c);
                }
                self.scroll_y = -1; // Scroll down when typing
            },
            Event::MouseScroll { delta } => {
                if self.scroll_y == -1 {
                    // It will be calculated in draw() next frame, but roughly guess it
                    // The easiest way is to let the user scroll up from bottom
                }
                self.scroll_y = self.scroll_y.saturating_add(delta as isize);
                if self.scroll_y < 0 { self.scroll_y = 0; }
            },
            Event::MouseClick { x: _rel_x, y: _rel_y } => {
                // Not passing width/height to event, we just approximate it, or we could handle scrollbar dragging
            },
            _ => {}
        }
    }
}
