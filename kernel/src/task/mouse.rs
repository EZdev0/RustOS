use core::{pin::Pin, task::{Context, Poll}};
use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{stream::Stream, task::AtomicWaker};

static MOUSE_QUEUE: OnceCell<ArrayQueue<(i32, i32, bool, bool)>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

pub(crate) fn add_mouse_event(dx: i32, dy: i32, left: bool, right: bool) {
    if let Ok(queue) = MOUSE_QUEUE.try_get() {
        if let Err(_) = queue.push((dx, dy, left, right)) {
            // Queue full
        } else {
            WAKER.wake();
        }
    }
}

pub struct MouseStream {
    _private: (),
}

impl MouseStream {
    pub fn new() -> Self {
        MOUSE_QUEUE.try_init_once(|| ArrayQueue::new(100))
            .expect("MouseStream::new should only be called once");
        MouseStream { _private: () }
    }
}

impl Stream for MouseStream {
    type Item = (i32, i32, bool, bool);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let queue = MOUSE_QUEUE.try_get().expect("not initialized");
        
        if let Some(event) = queue.pop() {
            return Poll::Ready(Some(event));
        }

        WAKER.register(&cx.waker());
        
        match queue.pop() {
            Some(event) => {
                WAKER.take();
                Poll::Ready(Some(event))
            }
            None => Poll::Pending,
        }
    }
}

pub async fn mouse_task(compositor: alloc::sync::Arc<spin::Mutex<crate::desktop::compositor::GraphicalCompositor>>) {
    let mut stream = MouseStream::new();
    use futures_util::stream::StreamExt;
    
    while let Some((dx, dy, left, right)) = stream.next().await {
        let mut comp = compositor.lock();
        comp.handle_mouse_event(dx, dy, left, right);
        comp.render_all();
    }
}
