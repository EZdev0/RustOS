use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin::Mutex;
use pc_keyboard::{layouts, DecodedKey, HandleControl, PS2Keyboard, ScancodeSet1};
use heapless::Vec;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
        idt
    };
}

lazy_static! {
    static ref KEYBOARD: Mutex<PS2Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(PS2Keyboard::new(ScancodeSet1::new(), layouts::Us104Key, HandleControl::Ignore));

    static ref KEY_QUEUE: Mutex<Vec<char, 256>> = Mutex::new(Vec::new());
}

pub fn pop_key() -> Option<char> {
    if let Some(mut queue) = KEY_QUEUE.try_lock() {
        if queue.is_empty() {
            None
        } else {
            Some(queue.remove(0))
        }
    } else {
        None
    }
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(
    _stack_frame: InterruptStackFrame,
) {
    // Breakpoint handler
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame,
) {
    use x86_64::instructions::port::Port;

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => {
                    if let Some(mut queue) = KEY_QUEUE.try_lock() {
                        let _ = queue.push(character);
                    }
                },
                DecodedKey::RawKey(_) => {},
            }
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}
