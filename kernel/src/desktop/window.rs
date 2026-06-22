pub struct Window {
    pub title: &'static str,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Window {
    pub const fn new(title: &'static str, x: usize, y: usize, width: usize, height: usize) -> Self {
        Self { title, x, y, width, height }
    }
}
