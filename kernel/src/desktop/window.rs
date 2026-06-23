use super::app::App;
use alloc::boxed::Box;

#[derive(Clone, Copy, PartialEq)]
pub enum WindowAnimState {
    Opening(usize),
    Open,
}

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
    pub anim_state: WindowAnimState,
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
            anim_state: WindowAnimState::Opening(0),
        }
    }

    pub fn tick_animation(&mut self) {
        if let WindowAnimState::Opening(tick) = self.anim_state {
            let next_tick = tick + 1;
            // T_TRACE (60) + T_PULSE_OUT (15) + T_PULSE_IN (15) = 90
            if next_tick >= 90 {
                self.anim_state = WindowAnimState::Open;
            } else {
                self.anim_state = WindowAnimState::Opening(next_tick);
            }
        }
    }
}
