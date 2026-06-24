use core::sync::atomic::{AtomicUsize, Ordering};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use lazy_static::lazy_static;
use pic8259::ChainedPics;

pub static TIMER_TICKS: AtomicUsize = AtomicUsize::new(0);
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
    Mouse = PIC_1_OFFSET + 12,
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
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(gp_fault_handler);
        idt[InterruptIndex::Timer.as_u8()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Mouse.as_u8()].set_handler_fn(mouse_interrupt_handler);
        idt
    };
}

static mut MOUSE_PACKET: [u8; 3] = [0; 3];
static mut MOUSE_CYCLE: u8 = 0;

fn mouse_wait(a_type: u8) {
    use x86_64::instructions::port::Port;
    let mut port_64 = Port::<u8>::new(0x64);
    let timeout = 100_000;
    for _ in 0..timeout {
        let status = unsafe { port_64.read() };
        if a_type == 0 {
            if (status & 1) == 1 { return; }
        } else {
            if (status & 2) == 0 { return; }
        }
        core::hint::spin_loop();
    }
}

pub fn init_mouse() {
    use x86_64::instructions::port::Port;
    let mut port_64 = Port::<u8>::new(0x64);
    let mut port_60 = Port::<u8>::new(0x60);
    
    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            // 1. Disable Keyboard & Mouse
            mouse_wait(1);
            port_64.write(0xAD); // Disable Keyboard
            mouse_wait(1);
            port_64.write(0xA7); // Disable Mouse
            
            // 2. Flush Output Buffer
            while (port_64.read() & 1) == 1 {
                port_60.read();
            }

            // 3. Configure Command Byte
            mouse_wait(1);
            port_64.write(0x20); // Read Compaq Status Byte
            mouse_wait(0);
            let mut status = port_60.read();
            
            status |= 0x03; // Enable IRQ1 (Keyboard) and IRQ12 (Mouse)
            status &= !0x20; // Clear Mouse Clock Disable bit
            
            mouse_wait(1);
            port_64.write(0x60); // Write Compaq Status Byte
            mouse_wait(1);
            port_60.write(status);

            // 4. Enable Mouse Port
            mouse_wait(1);
            port_64.write(0xA8);

            // 5. Reset Mouse
            mouse_wait(1);
            port_64.write(0xD4); // Write to Auxiliary Device
            mouse_wait(1);
            port_60.write(0xFF); // Reset Command
            
            // Wait for Reset responses: ACK (0xFA), Self-test Success (0xAA), Mouse ID (0x00)
            mouse_wait(0);
            port_60.read(); // ACK
            mouse_wait(0);
            port_60.read(); // 0xAA
            mouse_wait(0);
            port_60.read(); // 0x00

            // 6. Enable Data Reporting
            mouse_wait(1);
            port_64.write(0xD4);
            mouse_wait(1);
            port_60.write(0xF4);
            mouse_wait(0);
            port_60.read(); // ACK

            // 7. Re-enable Keyboard Port
            mouse_wait(1);
            port_64.write(0xAE);
        }
    });
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
    
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    
    crate::task::keyboard::add_scancode(scancode);

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

pub fn init_pit(frequency: u32) {
    use x86_64::instructions::port::Port;
    
    let divisor = 1_193_182 / frequency;
    
    let mut command_port: Port<u8> = Port::new(0x43);
    let mut data_port: Port<u8> = Port::new(0x40);

    unsafe {
        command_port.write(0x36);
        data_port.write((divisor & 0xFF) as u8);
        data_port.write((divisor >> 8) as u8);
    }
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    
    // Future expansion: Wake up tasks here if needed
    // crate::task::executor::wake_timer_tasks();

    unsafe {
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode
) {
    use x86_64::registers::control::Cr2;
    panic!("EXCEPTION: PAGE FAULT\nAccessed Address: {:?}\nError Code: {:?}\n{:#?}", Cr2::read(), error_code, stack_frame);
}

extern "x86-interrupt" fn gp_fault_handler(
    stack_frame: InterruptStackFrame, error_code: u64
) {
    panic!("EXCEPTION: GENERAL PROTECTION FAULT\nError Code: {}\n{:#?}", error_code, stack_frame);
}

extern "x86-interrupt" fn mouse_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;
    let mut port = Port::<u8>::new(0x60);
    let packet = unsafe { port.read() };
    
    unsafe {
        match MOUSE_CYCLE {
            0 => {
                // Ignore ACK from Limbo emulator or real hardware
                if packet == 0xFA {
                    // Do nothing
                }
                // Ensure alignment: PS/2 Byte 1 always has bit 3 set to 1
                else if (packet & 8) == 8 {
                    MOUSE_PACKET[0] = packet;
                    MOUSE_CYCLE = 1;
                }
            },
            1 => { MOUSE_PACKET[1] = packet; MOUSE_CYCLE = 2; },
            2 => {
                MOUSE_PACKET[2] = packet; MOUSE_CYCLE = 0;
                
                let mut dx = MOUSE_PACKET[1] as i32;
                let mut dy = MOUSE_PACKET[2] as i32;
                
                if (MOUSE_PACKET[0] & 0x10) != 0 { dx -= 256; }
                if (MOUSE_PACKET[0] & 0x20) != 0 { dy -= 256; }
                dy = -dy; // Y-Achse invertieren

                let left_click = (MOUSE_PACKET[0] & 1) != 0;
                let right_click = (MOUSE_PACKET[0] & 2) != 0;
                
                crate::task::mouse::add_mouse_event(dx, dy, left_click, right_click);
            },
            _ => { MOUSE_CYCLE = 0; }
        }
        PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
    }
}
