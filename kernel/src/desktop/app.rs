use crate::desktop::compositor::GraphicalCompositor;

pub enum Event {
    KeyPress(char),
    KeyCode(u8),
    MouseClick { x: usize, y: usize },
}

pub trait App {
    fn title(&self) -> &str;
    fn update(&mut self);
    fn draw(&mut self, compositor: &mut GraphicalCompositor, x: usize, y: usize, width: usize, height: usize);
    fn handle_event(&mut self, event: Event);
}
