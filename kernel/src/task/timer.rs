use core::{pin::Pin, task::{Context, Poll}};
use futures_util::{stream::Stream, task::AtomicWaker};
use crate::interrupts::TIMER_TICKS;
use core::sync::atomic::Ordering;

static WAKER: AtomicWaker = AtomicWaker::new();

pub(crate) fn wake_timer_tasks() {
    WAKER.wake();
}

pub struct TickStream {
    last_tick: usize,
}

impl Default for TickStream {
    fn default() -> Self {
        Self::new()
    }
}

impl TickStream {
    pub fn new() -> Self {
        TickStream { last_tick: TIMER_TICKS.load(Ordering::Relaxed) }
    }
}

impl Stream for TickStream {
    type Item = usize;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<usize>> {
        let current_tick = TIMER_TICKS.load(Ordering::Relaxed);
        // PIT runs at 1000Hz (1 tick = 1ms). 
        // We wait for 16 ticks to get ~60 frames per second.
        if current_tick >= self.last_tick + 16 {
            self.last_tick = current_tick;
            return Poll::Ready(Some(current_tick));
        }

        WAKER.register(cx.waker());
        
        // Double check after registering to prevent race conditions
        let current_tick = TIMER_TICKS.load(Ordering::Relaxed);
        if current_tick >= self.last_tick + 16 {
            WAKER.take();
            self.last_tick = current_tick;
            Poll::Ready(Some(current_tick))
        } else {
            Poll::Pending
        }
    }
}

pub async fn timer_task(compositor: alloc::sync::Arc<spin::Mutex<crate::desktop::compositor::GraphicalCompositor>>) {
    use futures_util::stream::StreamExt;
    let mut ticks = TickStream::new();
    
    while ticks.next().await.is_some() {
        compositor.lock().render_all();
        // Force yield to the executor to prevent starving the mouse/keyboard tasks 
        // if render_all() takes longer than 16ms!
        crate::task::yield_now::yield_now().await;
    }
}
