use x86_64::instructions::port::Port;

pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn read_register(reg: u8) -> u8 {
    let mut address_port = Port::<u8>::new(0x70);
    let mut data_port = Port::<u8>::new(0x71);
    unsafe {
        // CRITICAL BUGFIX: Das MSB (Bit 7) des CMOS Address Ports kontrolliert NMI!
        // Wenn wir 0x00 schreiben, ENABLEN wir NMIs. Wenn dann ein NMI feuert, crasht 
        // das OS mit #GP Error 16 (IDT Vektor 2 * 8), da kein NMI Handler existiert.
        // Daher MÜSSEN wir das NMI-Mask Bit (0x80) setzen, um NMI zu blockieren!
        address_port.write(reg | 0x80);
        data_port.read()
    }
}

fn is_update_in_progress() -> bool {
    (read_register(0x0A) & 0x80) != 0
}

fn bcd_to_binary(bcd: u8) -> u8 {
    (bcd & 0x0F) + ((bcd / 16) * 10)
}

pub fn read_rtc() -> DateTime {
    while is_update_in_progress() {
        core::hint::spin_loop();
    }

    let mut second = read_register(0x00);
    let mut minute = read_register(0x02);
    let mut hour = read_register(0x04);
    let mut day = read_register(0x07);
    let mut month = read_register(0x08);
    let mut year = read_register(0x09);
    let register_b = read_register(0x0B);

    if (register_b & 0x04) == 0 {
        // BCD Modus aktiv
        second = bcd_to_binary(second);
        minute = bcd_to_binary(minute);
        hour = bcd_to_binary(hour);
        day = bcd_to_binary(day);
        month = bcd_to_binary(month);
        year = bcd_to_binary(year);
    }

    // Handle 12-hour clock
    if (register_b & 0x02) == 0 && (hour & 0x80) != 0 {
        hour = ((hour & 0x7F) + 12) % 24;
    }

    DateTime {
        year: year as u16 + 2000,
        month,
        day,
        hour,
        minute,
        second,
    }
}
