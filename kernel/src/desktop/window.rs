use super::app::App;
use alloc::boxed::Box;

pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub app: Box<dyn App>,
}

impl Window {
    pub fn new(app: Box<dyn App>, x: usize, y: usize, width: usize, height: usize) -> Self {
        Window {
            x,
            y,
            width,
            height,
            app,
        }
    }
}
