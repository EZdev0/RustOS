use super::app::App;
use alloc::boxed::Box;

pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub app: Box<dyn App>,
    pub is_maximized: bool,
    pub orig_x: usize,
    pub orig_y: usize,
    pub orig_w: usize,
    pub orig_h: usize,
}

impl Window {
    pub fn new(app: Box<dyn App>, x: usize, y: usize, width: usize, height: usize) -> Self {
        Window {
            x,
            y,
            width,
            height,
            app,
            is_maximized: false,
            orig_x: x,
            orig_y: y,
            orig_w: width,
            orig_h: height,
        }
    }
}
