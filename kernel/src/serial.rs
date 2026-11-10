use core::fmt;
use spin::Mutex;
use uart_16550::SerialPort;

lazy_static::lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    x86_64::instructions::interrupts::without_interrupts(|| {
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    });
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {{
        // No trailing semicolon: the macro is also used in expression
        // position (`if x { serial_print!(..) }`), where a stray `;` would
        // trip the `semicolon_in_expressions_from_macros` lint (-D warnings).
        $crate::serial::_print(format_args!($($arg)*))
    }};
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_print!(
        concat!($fmt, "\n"), $($arg)*));
}

#[macro_export]
macro_rules! log {
    ($lvl:expr, $($arg:tt)*) => {{
        $crate::serial_println!("[{}] {}", $lvl, format_args!($($arg)*))
    }};
}

// ─── Raw, allocation-free serial access ─────────────────────────────────────
//
// The QEMU boot test in CI needs output *before* the heap exists and *while*
// panicking, so these helpers talk to the 16550 UART directly: no `lazy_static`,
// no mutex, no allocation, no formatting machinery that could recurse.

/// Initialise COM1 (115200 8N1) without touching the heap or any lock.
pub fn init_early() {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut data = Port::<u8>::new(0x3F8);
        let mut divisor_high = Port::<u8>::new(0x3F9);
        let mut fifo = Port::<u8>::new(0x3FA);
        let mut line_control = Port::<u8>::new(0x3FB);
        let mut modem_control = Port::<u8>::new(0x3FC);
        let mut interrupt_enable = Port::<u8>::new(0x3F9);

        interrupt_enable.write(0x00); // interrupts off
        line_control.write(0x80); // enable DLAB
        data.write(0x01); // divisor low byte: 1 -> 115200 baud
        divisor_high.write(0x00); // divisor high byte
        line_control.write(0x03); // 8 data bits, no parity, 1 stop bit
        fifo.write(0xC7); // enable + clear FIFOs
        modem_control.write(0x03); // DTR + RTS
    }
}

/// Write bytes to COM1, busy-waiting on the transmit buffer.
///
/// The iteration guard keeps a hung/absent UART from wedging the panic path.
pub fn write_bytes_raw(bytes: &[u8]) {
    use x86_64::instructions::port::Port;
    let mut line_status = Port::<u8>::new(0x3FD);
    let mut data = Port::<u8>::new(0x3F8);
    for &byte in bytes {
        let mut guard = 0u32;
        unsafe {
            while (line_status.read() & 0x20) == 0 {
                guard += 1;
                if guard > 1_000_000 {
                    return;
                }
                core::hint::spin_loop();
            }
            data.write(byte);
        }
    }
}

/// `core::fmt::Write` sink for the panic handler.
pub struct RawSerial;

impl core::fmt::Write for RawSerial {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        write_bytes_raw(text.as_bytes());
        Ok(())
    }
}

/// Format directly to the UART without allocating (panic-safe).
pub fn panic_write(args: core::fmt::Arguments<'_>) {
    use core::fmt::Write;
    let _ = RawSerial.write_fmt(args);
}
