//! Realtek RTL8139 ("8139too") driver — the NIC Limbo and older QEMU
//! configurations default to.
//!
//! The driver follows the classic programming model of the chip:
//!
//! * register access through the I/O BAR (BAR0, bit 0 = 1) or the memory BAR
//!   (BAR1, mapped via the bootloader's physical-memory window),
//! * a single 8 KiB+16 receive ring that the chip fills linearly and wraps at
//!   `RX_RING_SIZE` (the "WRAP" bit), so a frame may straddle the end of the
//!   ring and the copy has to follow the wrap,
//! * four transmit buffers; the driver writes the length with the `OWN` bit and
//!   waits for the chip to hand the descriptor back.
//!
//! Everything here is polling based: the kernel has no MSI and the network task
//! calls `receive()` in its loop. A DMA address therefore has to be a *physical*
//! address (the chip does not know about page tables); we ask `paging` for the
//! translation and refuse to start when a buffer is not mapped.

use alloc::vec;
use alloc::vec::Vec;

use crate::hardware::pci::PciDevice;
use crate::paging;

const RTL_VENDOR_ID: u16 = 0x10EC;
/// 8139C/C+ ("8139 too") — the ID QEMU, Limbo and VirtualBox all report.
const RTL_DEVICE_IDS: [u16; 3] = [0x8139, 0x8138, 0x8136];

// ── registers ───────────────────────────────────────────────────────────────
const REG_IDR0: u16 = 0x00; // MAC bytes 0..5
const REG_TSD0: u16 = 0x10; // transmit status 0..3 (u32 each)
const REG_TSAD0: u16 = 0x20; // transmit buffer address 0..3 (u32 each)
const REG_RBSTART: u16 = 0x30;
const REG_CR: u16 = 0x37;
const REG_CAPR: u16 = 0x38;
const REG_CBR: u16 = 0x3A;
const REG_IMR: u16 = 0x3C;
const REG_ISR: u16 = 0x3E;
const REG_TCR: u16 = 0x40;
const REG_RCR: u16 = 0x44;
const REG_CONFIG1: u16 = 0x52;

const CR_RST: u8 = 0x10;
const CR_RE: u8 = 0x08;
const CR_TE: u8 = 0x04;

/// Accept broadcast + multicast + physical match + wrap at the ring end.
const RCR_VALUE: u32 = 0x8F;
/// `RCR` ring size 00 = 8 KiB + 16 byte header area.
const RX_RING_SIZE: usize = 8192 + 16;
/// Extra room so a frame that starts at the very end of the ring still fits.
const RX_BUFFER_SIZE: usize = RX_RING_SIZE + 1500;
const TX_BUFFERS: usize = 4;
/// The chip cannot transmit a frame larger than this (plus headers).
const TX_BUFFER_SIZE: usize = 1792;
/// `TSD` bit 13. Despite the name in the datasheet ("OWN") the bit means
/// **host owns the descriptor**: it is set at reset, and the chip sets it again
/// after it has transmitted. A descriptor is therefore only handed to the chip
/// by writing the length *without* this bit - QEMU's model refuses to send
/// while it is set, exactly like the real RTL8139 and like Linux' `8139too`
/// (`RTL_W32_F(TxStatus0, tx_flag | len)`, no `TxHostOwns`).
const TSD_HOST_OWNS: u32 = 1 << 13;
/// `TCR` bits 12:11 select the loopback mode: 0b11 = internal loopback, in
/// which transmitted frames come back through the receive ring without ever
/// touching the wire. That is exactly what a boot-time self test needs.
const TCR_LBM_INTERNAL: u32 = 0b11 << 11;
const TCR_LBM_MASK: u32 = 0b11 << 11;

/// Boot-time diagnostics, same idea as in the e1000 driver: the first few
/// transmit/receive events are logged so a dead direction is visible instead of
/// being guessed at. The network task polls `receive()` continuously, so the
/// counters keep the log bounded.
static TX_EVENTS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static TX_DROPPED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static RX_POLLS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Volatile read of a memory-mapped register.
///
/// # Safety
///
/// The caller must pass an address inside the NIC's mapped BAR window, aligned
/// to `size_of::<T>()`. The address is derived from `phys_offset + BAR`, which
/// `new()` validates with `paging::is_range_mapped` before the first access.
///
/// Whether `ptr::read_volatile` is a safe function depends on the toolchain
/// (newer nightlies made it safe, older ones require `unsafe`), so the block
/// stays - correct on both - and `unused_unsafe` is allowed for the newer ones.
#[allow(unused_unsafe)]
fn mmio_read<T: Copy>(address: usize) -> T {
    unsafe { core::ptr::read_volatile(address as *const T) }
}

/// Volatile write to a memory-mapped register (same contract as [`mmio_read`]).
#[allow(unused_unsafe)]
fn mmio_write<T: Copy>(address: usize, value: T) {
    unsafe { core::ptr::write_volatile(address as *mut T, value) }
}

pub struct Rtl8139 {
    /// I/O port base, when BAR0 is an I/O BAR (the normal x86 case).
    io_base: Option<u16>,
    /// Memory-mapped base, when only BAR1 is available.
    mmio_base: Option<usize>,
    mac: [u8; 6],
    rx_buffer: Vec<u8>,
    rx_physical: u64,
    tx_buffers: Vec<[u8; TX_BUFFER_SIZE]>,
    tx_physical: [u64; TX_BUFFERS],
    tx_index: usize,
    rx_offset: usize,
    /// Reported for the boot log; the I/O base or the memory BAR.
    bar_address: u64,
}

impl Rtl8139 {
    /// Try to bring the NIC at `device` up. `None` means "not usable", with the
    /// reason already on the serial console.
    pub fn new(device: &PciDevice, phys_offset: u64) -> Option<Self> {
        let io_base = if device.bar0 & 1 == 1 {
            Some((device.bar0 & !0x3) as u16)
        } else {
            None
        };
        let mmio_bar = if device.bar1 & 1 == 0 && device.bar1 != 0 {
            (device.bar1 & !0xF) as u64
        } else if device.bar0 & 1 == 0 && device.bar0 != 0 {
            (device.bar0 & !0xF) as u64
        } else {
            0
        };
        let mmio_base = if io_base.is_none() && mmio_bar != 0 {
            if phys_offset == 0
                || !paging::is_range_mapped(phys_offset + mmio_bar, 0x100, phys_offset)
            {
                crate::serial_println!(
                    "[net] rtl8139 registers at {:#x} are unmapped - driver skipped",
                    mmio_bar
                );
                return None;
            }
            Some((phys_offset + mmio_bar) as usize)
        } else {
            None
        };
        if io_base.is_none() && mmio_base.is_none() {
            crate::serial_println!("[net] rtl8139 has neither an I/O nor a memory BAR");
            return None;
        }

        let mut nic = Self {
            io_base,
            mmio_base,
            mac: [0; 6],
            rx_buffer: vec![0u8; RX_BUFFER_SIZE],
            rx_physical: 0,
            tx_buffers: vec![[0u8; TX_BUFFER_SIZE]; TX_BUFFERS],
            tx_physical: [0; TX_BUFFERS],
            tx_index: 0,
            rx_offset: 0,
            bar_address: if let Some(io) = io_base {
                u64::from(io)
            } else {
                mmio_bar
            },
        };

        // The chip DMAs into these buffers, so every one of them needs a
        // physical address. A heap buffer that the page walk cannot translate
        // is a hard "no" — better no network than memory corruption.
        let rx_phys = paging::physical_address(nic.rx_buffer.as_ptr() as u64, phys_offset);
        let Some(rx_phys) = rx_phys else {
            crate::serial_println!("[net] rtl8139 RX buffer is not physically addressable");
            return None;
        };
        nic.rx_physical = rx_phys;
        for index in 0..TX_BUFFERS {
            let Some(phys) =
                paging::physical_address(nic.tx_buffers[index].as_ptr() as u64, phys_offset)
            else {
                crate::serial_println!("[net] rtl8139 TX buffer {} is not addressable", index);
                return None;
            };
            nic.tx_physical[index] = phys;
        }

        if !nic.init() {
            return None;
        }
        Some(nic)
    }

    // ── register access ─────────────────────────────────────────────────────

    fn read_u8(&self, offset: u16) -> u8 {
        match self.io_base {
            Some(base) => {
                let mut port = x86_64::instructions::port::Port::<u8>::new(base + offset);
                unsafe { port.read() }
            }
            None => mmio_read::<u8>(self.mmio_base.unwrap_or(0) + offset as usize),
        }
    }

    fn read_u16(&self, offset: u16) -> u16 {
        match self.io_base {
            Some(base) => {
                let mut port = x86_64::instructions::port::Port::<u16>::new(base + offset);
                unsafe { port.read() }
            }
            None => mmio_read::<u16>(self.mmio_base.unwrap_or(0) + offset as usize),
        }
    }

    fn read_u32(&self, offset: u16) -> u32 {
        match self.io_base {
            Some(base) => {
                let mut port = x86_64::instructions::port::Port::<u32>::new(base + offset);
                unsafe { port.read() }
            }
            None => mmio_read::<u32>(self.mmio_base.unwrap_or(0) + offset as usize),
        }
    }

    fn write_u8(&self, offset: u16, value: u8) {
        match self.io_base {
            Some(base) => {
                let mut port = x86_64::instructions::port::Port::<u8>::new(base + offset);
                unsafe { port.write(value) }
            }
            None => mmio_write::<u8>(self.mmio_base.unwrap_or(0) + offset as usize, value),
        }
    }

    fn write_u16(&self, offset: u16, value: u16) {
        match self.io_base {
            Some(base) => {
                let mut port = x86_64::instructions::port::Port::<u16>::new(base + offset);
                unsafe { port.write(value) }
            }
            None => mmio_write::<u16>(self.mmio_base.unwrap_or(0) + offset as usize, value),
        }
    }

    fn write_u32(&self, offset: u16, value: u32) {
        match self.io_base {
            Some(base) => {
                let mut port = x86_64::instructions::port::Port::<u32>::new(base + offset);
                unsafe { port.write(value) }
            }
            None => mmio_write::<u32>(self.mmio_base.unwrap_or(0) + offset as usize, value),
        }
    }

    // ── bring-up ────────────────────────────────────────────────────────────

    fn init(&mut self) -> bool {
        // Power on: CONFIG1 = 0 (the reset default may keep the PHY asleep).
        self.write_u8(REG_CONFIG1, 0x00);

        // Software reset, then poll the RST bit until the chip is done.
        self.write_u8(REG_CR, CR_RST);
        let mut waited = 0u32;
        while self.read_u8(REG_CR) & CR_RST != 0 {
            waited += 1;
            if waited > 1_000_000 {
                crate::serial_println!("[net] rtl8139 reset timed out");
                return false;
            }
            core::hint::spin_loop();
        }

        let mut mac = [0u8; 6];
        for (index, byte) in mac.iter_mut().enumerate() {
            *byte = self.read_u8(REG_IDR0 + index as u16);
        }
        self.mac = mac;
        if self.mac == [0; 6] || self.mac == [0xFF; 6] {
            crate::serial_println!("[net] rtl8139 reports no MAC address");
            return false;
        }

        // Clear pending interrupts, unmask RX/TX completion (polling only).
        self.write_u16(REG_ISR, 0xFFFF);
        self.write_u16(REG_IMR, 0x0005);

        // Receive ring + policy.
        self.write_u32(REG_RBSTART, self.rx_physical as u32);
        self.write_u32(REG_RCR, RCR_VALUE);
        self.write_u32(REG_TCR, 0);

        // Transmit descriptors: address, then "owned by the host" (free).
        // Writing 0 here would look like a zero-length frame to the chip.
        for index in 0..TX_BUFFERS {
            self.write_u32(
                REG_TSAD0 + (index as u16) * 4,
                self.tx_physical[index] as u32,
            );
            self.write_u32(REG_TSD0 + (index as u16) * 4, TSD_HOST_OWNS);
        }

        // Ring is empty: the chip's read pointer sits 16 bytes behind the
        // packet offset (documented CAPR quirk), so offset 0 means CAPR = ring-16.
        self.rx_offset = 0;
        self.write_u16(REG_CAPR, (RX_RING_SIZE - 16) as u16);

        // Enable receiver + transmitter.
        self.write_u8(REG_CR, CR_RE | CR_TE);

        // Bit 3 of CONFIG1 is the "link up" latch on most boards; report it,
        // but do not refuse to run just because the PHY is still negotiating.
        let link = self.read_u8(REG_CONFIG1) & 0x04 != 0;
        crate::serial_println!(
            "[net] rtl8139 up: bar={:#x} mac={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X} link={}",
            self.bar_address,
            self.mac[0],
            self.mac[1],
            self.mac[2],
            self.mac[3],
            self.mac[4],
            self.mac[5],
            if link { "up" } else { "negotiating" }
        );

        // Read the configuration back: a register that silently stayed 0 (BAR
        // not decoded, wrong access width) makes the chip deaf, and the
        // resulting "no reply" looks identical to a broken cable.
        crate::serial_println!(
            "[net] rtl8139 regs: cr={:#04x} isr={:#06x} rcr={:#010x} tcr={:#010x} cbr={} capr={}",
            self.read_u8(REG_CR),
            self.read_u16(REG_ISR),
            self.read_u32(REG_RCR),
            self.read_u32(REG_TCR),
            self.read_u16(REG_CBR),
            self.read_u16(REG_CAPR)
        );
        crate::serial_println!(
            "[net] rtl8139 dma: rx_ring={:#x} tx_buf0={:#x} rx_offset={}",
            self.rx_physical,
            self.tx_physical[0],
            self.rx_offset
        );

        // Prove TX -> RX through the chip itself, without a peer and without
        // the emulator's backend: if this fails the driver is broken, if it
        // passes but no reply arrives the problem is on the wire.
        let loopback = self.loopback_test();
        crate::serial_println!(
            "[net] rtl8139 loopback: {} (cbr={} rx_offset={} isr={:#06x} tx_status={:#010x})",
            if loopback { "ok" } else { "failed" },
            self.read_u16(REG_CBR),
            self.rx_offset,
            self.read_u16(REG_ISR),
            self.read_u32(REG_TSD0)
        );
        true
    }

    /// Internal loopback self test (`TCR` bits 12:11 = 0b11).
    ///
    /// Transmits one frame with a private ethertype and waits for it in the
    /// receive ring. Restores the ring state and normal `TCR` afterwards, so the
    /// driver leaves `init()` exactly as it would without the test.
    fn loopback_test(&mut self) -> bool {
        const TEST_ETHERTYPE: [u8; 2] = [0x88, 0xB5];

        let saved_rx_offset = self.rx_offset;
        let saved_tx_index = self.tx_index;
        let saved_tcr = self.read_u32(REG_TCR);

        self.write_u32(REG_TCR, (saved_tcr & !TCR_LBM_MASK) | TCR_LBM_INTERNAL);

        let mut frame = [0u8; 64];
        frame[0..6].copy_from_slice(&self.mac);
        frame[6..12].copy_from_slice(&self.mac);
        frame[12..14].copy_from_slice(&TEST_ETHERTYPE);
        for (index, byte) in frame[14..].iter_mut().enumerate() {
            *byte = index as u8;
        }

        let mut found = false;
        if self.transmit(&frame) {
            for _ in 0..5_000 {
                if let Some(received) = self.receive() {
                    if received.len() >= 14
                        && received[12] == TEST_ETHERTYPE[0]
                        && received[13] == TEST_ETHERTYPE[1]
                    {
                        found = true;
                        break;
                    }
                }
            }
        }

        // Leave loopback mode and forget everything the test consumed.
        self.write_u32(REG_TCR, saved_tcr);
        self.rx_offset = saved_rx_offset;
        self.tx_index = saved_tx_index;
        self.write_u16(REG_ISR, 0xFFFF);
        found
    }

    // ── smoltcp plumbing ────────────────────────────────────────────────────

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    pub fn bar_address(&self) -> u64 {
        self.bar_address
    }

    /// A descriptor is free once the chip has handed it back, i.e. as soon as
    /// it owns it again (`TSD_HOST_OWNS` set).
    pub fn can_transmit(&self) -> bool {
        self.read_u32(REG_TSD0 + (self.tx_index as u16) * 4) & TSD_HOST_OWNS != 0
    }

    pub fn transmit(&mut self, data: &[u8]) -> bool {
        if data.len() > TX_BUFFER_SIZE || !self.can_transmit() {
            let dropped = TX_DROPPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if dropped < 4 {
                crate::serial_println!(
                    "[net] rtl8139 tx dropped: len={} tsd={:#010x}",
                    data.len(),
                    self.read_u32(REG_TSD0 + (self.tx_index as u16) * 4)
                );
            }
            return false;
        }
        let index = self.tx_index;
        let event = TX_EVENTS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if event < 4 {
            crate::serial_println!(
                "[net] rtl8139 tx #{} len={} idx={} tsad={:#x}",
                event,
                data.len(),
                index,
                self.tx_physical[index]
            );
        }
        self.tx_buffers[index][..data.len()].copy_from_slice(data);
        // Address first, then the bare length: writing the status register is
        // the trigger, and bit 13 must stay clear ("host no longer owns it").
        self.write_u32(
            REG_TSAD0 + (index as u16) * 4,
            self.tx_physical[index] as u32,
        );
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        self.write_u32(REG_TSD0 + (index as u16) * 4, data.len() as u32);
        self.tx_index = (index + 1) % TX_BUFFERS;
        true
    }

    /// Copy the next received frame out of the ring, if the chip delivered one.
    pub fn receive(&mut self) -> Option<Vec<u8>> {
        let cbr = self.read_u16(REG_CBR) as usize % RX_RING_SIZE;
        let poll = RX_POLLS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if poll < 4 {
            crate::serial_println!(
                "[net] rtl8139 rx probe #{} cbr={} rx_offset={} empty={}",
                poll,
                cbr,
                self.rx_offset,
                cbr == self.rx_offset
            );
        }
        if cbr == self.rx_offset {
            return None; // ring empty
        }

        let status = self.read_ring_u16(self.rx_offset);
        let length = self.read_ring_u16(self.rx_offset + 2) as usize;
        // ROK (bit 0) must be set and the length has to make sense; a broken
        // descriptor would otherwise let us allocate gigabytes.
        if status & 0x0001 == 0 || length == 0 || length > 1518 {
            // Resynchronise: skip this header and let the chip continue.
            self.advance_ring(4);
            return None;
        }

        let mut frame = vec![0u8; length];
        for (offset, byte) in frame.iter_mut().enumerate() {
            *byte = self.read_ring_u8(self.rx_offset + 4 + offset);
        }

        // Acknowledge: the driver owns everything up to the next header.
        self.advance_ring(4 + length);
        Some(frame)
    }

    fn advance_ring(&mut self, mut bytes: usize) {
        bytes = (bytes + 3) & !3; // 4-byte aligned headers
        self.rx_offset = (self.rx_offset + bytes) % RX_RING_SIZE;
        // CAPR points 16 bytes *behind* the packet offset we hand to the chip.
        let capr = (self.rx_offset + RX_RING_SIZE - 16) % RX_RING_SIZE;
        self.write_u16(REG_CAPR, capr as u16);
    }

    fn read_ring_u8(&self, offset: usize) -> u8 {
        self.rx_buffer[offset % RX_RING_SIZE]
    }

    fn read_ring_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.read_ring_u8(offset), self.read_ring_u8(offset + 1)])
    }
}

/// True when `device` is a Realtek 8139 this driver can program.
pub fn is_supported(device: &PciDevice) -> bool {
    device.vendor_id == RTL_VENDOR_ID && RTL_DEVICE_IDS.contains(&device.device_id)
}

/// Bring up the NIC behind `device`, or explain why not.
pub fn probe_one(device: &PciDevice, phys_offset: u64) -> Option<Rtl8139> {
    if !is_supported(device) {
        return None;
    }
    match Rtl8139::new(device, phys_offset) {
        Some(nic) => Some(nic),
        None => {
            crate::serial_println!(
                "[net] rtl8139 {:04x}:{:04x} could not be started",
                device.vendor_id,
                device.device_id
            );
            None
        }
    }
}
