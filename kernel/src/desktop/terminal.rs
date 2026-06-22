use crate::desktop::compositor::GraphicalCompositor;
use font8x8::UnicodeFonts;
use spin::Mutex;

pub static TERMINAL: Mutex<Option<Terminal>> = Mutex::new(None);

pub struct Terminal {
    // Window dimensions inside the compositor
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    // Store the framebuffer info to draw directly
    // Wait, drawing requires Framebuffer buffer. 
    // We cannot easily hold &'static mut Framebuffer inside a global without unsafe.
    // Instead, we will buffer the text in an array and the event loop will draw it!
}

pub static TEXT_BUFFER: Mutex<heapless::String<2048>> = Mutex::new(heapless::String::new());

pub fn print_char(c: char) {
    let mut buf = TEXT_BUFFER.lock();
    if c == '\x08' {
        let _ = buf.pop();
    } else if buf.len() < buf.capacity() {
        let _ = buf.push(c);
    }
}

pub fn print_string(s: &str) {
    for c in s.chars() {
        print_char(c);
    }
}
