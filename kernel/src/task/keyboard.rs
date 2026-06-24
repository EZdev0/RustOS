use core::{pin::Pin, task::{Context, Poll}};
use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{stream::Stream, task::AtomicWaker};
use pc_keyboard::{layouts, DecodedKey, HandleControl, PS2Keyboard, ScancodeSet1};

static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if x86_64::instructions::interrupts::without_interrupts(|| queue.push(scancode)).is_err() {
            // Queue full
        } else {
            WAKER.wake();
        }
    }
}

pub struct ScancodeStream {
    _private: (),
}

impl Default for ScancodeStream {
    fn default() -> Self {
        Self::new()
    }
}

impl ScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE.try_init_once(|| ArrayQueue::new(100))
            .expect("ScancodeStream::new should only be called once");
        ScancodeStream { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE.try_get().expect("not initialized");
        
        if let Some(scancode) = x86_64::instructions::interrupts::without_interrupts(|| queue.pop()) {
            return Poll::Ready(Some(scancode));
        }

        WAKER.register(cx.waker());
        
        match x86_64::instructions::interrupts::without_interrupts(|| queue.pop()) {
            Some(scancode) => {
                WAKER.take();
                Poll::Ready(Some(scancode))
            }
            None => Poll::Pending,
        }
    }
}

pub async fn keyboard_task(compositor: alloc::sync::Arc<spin::Mutex<crate::desktop::compositor::GraphicalCompositor>>) {
    let mut stream = ScancodeStream::new();
    let mut keyboard = PS2Keyboard::new(
        ScancodeSet1::new(),
        layouts::Us104Key,
        HandleControl::Ignore,
    );

    use futures_util::stream::StreamExt;
    
    while let Some(scancode) = stream.next().await {
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => {
                        let mut comp = compositor.lock();
                        comp.dispatch_keyboard_event(character);
                        comp.render_all();
                    },
                    DecodedKey::RawKey(_) => {},
                }
            }
        }
    }
}
