use alloc::vec::Vec;
use alloc::vec;
use core::ptr::{read_volatile, write_volatile};
use crate::hardware::pci::{PciDevice, pci_read_config_32, pci_write_config_32};

const E1000_VENDOR_ID: u16 = 0x8086;
const E1000_DEVICE_ID: u16 = 0x100E;

const REG_CTRL: usize = 0x0000;
const REG_IMC: usize = 0x00D8; // Interrupt Mask Clear
const REG_RCTL: usize = 0x0100;
const REG_TCTL: usize = 0x0400;

const REG_RDBAL: usize = 0x2800;
const REG_RDBAH: usize = 0x2804;
const REG_RDLEN: usize = 0x2808;
const REG_RDH: usize = 0x2810;
const REG_RDT: usize = 0x2818;

const REG_TDBAL: usize = 0x3800;
const REG_TDBAH: usize = 0x3804;
const REG_TDLEN: usize = 0x3808;
const REG_TDH: usize = 0x3810;
const REG_TDT: usize = 0x3818;

const RX_RING_SIZE: usize = 32;
const TX_RING_SIZE: usize = 32;
const BUFFER_SIZE: usize = 2048;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct RxDesc {
    pub addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct TxDesc {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

pub struct E1000 {
    mmio_base: usize,
    mac_address: [u8; 6],
    rx_ring: Vec<RxDesc>,
    tx_ring: Vec<TxDesc>,
    rx_buffers: Vec<[u8; BUFFER_SIZE]>,
    tx_buffers: Vec<[u8; BUFFER_SIZE]>,
    rx_index: usize,
    tx_index: usize,
    phys_offset: u64,
}

impl E1000 {
    pub fn new(pci_device: &PciDevice, phys_offset: u64) -> Self {
        let mut mmio_base = (pci_device.bar0 & !0xF) as usize;
        
        // SAFETY FIX: In some QEMU configurations without SeaBIOS, BAR0 is uninitialized (0).
        // If we write to address 0, we corrupt the IDT/GDT, causing #GP or #DF.
        if mmio_base == 0 {
            mmio_base = 0xFEB8_0000;
            pci_write_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x10, mmio_base as u32);
        }
        
        // Aktiviere Bus Mastering und Memory Space
        let cmd_reg = pci_read_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x04);
        pci_write_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x04, cmd_reg | 0x04 | 0x02);

        // Wir lesen die MAC Adresse rudimentär via Eeprom oder MMIO Registers
        let mut mac = [0u8; 6];
        unsafe {
            let ral = read_volatile((mmio_base + 0x5400) as *const u32);
            let rah = read_volatile((mmio_base + 0x5404) as *const u32);
            
            mac[0] = (ral & 0xFF) as u8;
            mac[1] = ((ral >> 8) & 0xFF) as u8;
            mac[2] = ((ral >> 16) & 0xFF) as u8;
            mac[3] = ((ral >> 24) & 0xFF) as u8;
            mac[4] = (rah & 0xFF) as u8;
            mac[5] = ((rah >> 8) & 0xFF) as u8;
        }

        Self {
            mmio_base,
            mac_address: mac,
            rx_ring: Vec::new(),
            tx_ring: Vec::new(),
            rx_buffers: Vec::new(),
            tx_buffers: Vec::new(),
            rx_index: 0,
            tx_index: 0,
            phys_offset,
        }
    }

    pub fn init(&mut self) {
        unsafe {
            // Disable all interrupts (Mask Clear) to prevent interrupt storms
            self.write_register(REG_IMC, 0xFFFF_FFFF);

            // Set Link Up
            let mut ctrl = read_volatile((self.mmio_base + REG_CTRL) as *const u32);
            ctrl |= 0x40; // Setze SLU (Set Link Up)
            write_volatile((self.mmio_base + REG_CTRL) as *mut u32, ctrl);
            
            // Konfiguriere Transmit (TX) und Receive (RX) Ringe im Speicher
            self.rx_ring = vec![RxDesc { addr: 0, length: 0, checksum: 0, status: 0, errors: 0, special: 0 }; RX_RING_SIZE];
            self.tx_ring = vec![TxDesc { addr: 0, length: 0, cso: 0, cmd: 0, status: 0, css: 0, special: 0 }; TX_RING_SIZE];
            self.rx_buffers = vec![[0u8; BUFFER_SIZE]; RX_RING_SIZE];
            self.tx_buffers = vec![[0u8; BUFFER_SIZE]; TX_RING_SIZE];

            // RX
            for i in 0..RX_RING_SIZE {
                self.rx_ring[i].addr = (self.rx_buffers[i].as_ptr() as u64).saturating_sub(self.phys_offset);
            }

            let rx_addr = (self.rx_ring.as_ptr() as u64).saturating_sub(self.phys_offset);
            self.write_register(REG_RDBAL, (rx_addr & 0xFFFF_FFFF) as u32);
            self.write_register(REG_RDBAH, (rx_addr >> 32) as u32);
            self.write_register(REG_RDLEN, (RX_RING_SIZE * core::mem::size_of::<RxDesc>()) as u32);
            self.write_register(REG_RDH, 0);
            self.write_register(REG_RDT, (RX_RING_SIZE - 1) as u32);
            
            // RCTL: EN (1<<1), BAM (1<<15)
            self.write_register(REG_RCTL, (1 << 1) | (1 << 15));

            // TX
            for i in 0..TX_RING_SIZE {
                self.tx_ring[i].addr = (self.tx_buffers[i].as_ptr() as u64).saturating_sub(self.phys_offset);
            }

            let tx_addr = (self.tx_ring.as_ptr() as u64).saturating_sub(self.phys_offset);
            self.write_register(REG_TDBAL, (tx_addr & 0xFFFF_FFFF) as u32);
            self.write_register(REG_TDBAH, (tx_addr >> 32) as u32);
            self.write_register(REG_TDLEN, (TX_RING_SIZE * core::mem::size_of::<TxDesc>()) as u32);
            self.write_register(REG_TDH, 0);
            self.write_register(REG_TDT, 0);

            // TCTL: EN (1<<1), PSP (1<<3)
            self.write_register(REG_TCTL, (1 << 1) | (1 << 3));
        }
    }

    pub fn can_transmit(&self) -> bool {
        let idx = self.tx_index;
        // Check if Descriptor Done (DD) bit is set or if status is 0 (unused)
        unsafe { core::ptr::read_volatile(&self.tx_ring[idx].status) & 1 == 1 || self.tx_ring[idx].status == 0 }
    }

    pub fn transmit(&mut self, data: &[u8]) {
        if data.len() > BUFFER_SIZE || !self.can_transmit() {
            return;
        }

        let idx = self.tx_index;
        
        self.tx_buffers[idx][..data.len()].copy_from_slice(data);
        
        // Convert virtual to physical address for DMA
        self.tx_ring[idx].addr = (self.tx_buffers[idx].as_ptr() as u64).saturating_sub(self.phys_offset);
        self.tx_ring[idx].length = data.len() as u16;
        self.tx_ring[idx].cmd = (1 << 3) | (1 << 1) | 1; // RS | IFCS | EOP
        self.tx_ring[idx].status = 0;

        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

        self.tx_index = (self.tx_index + 1) % TX_RING_SIZE;
        self.write_register(REG_TDT, self.tx_index as u32);
    }

    pub fn receive(&mut self) -> Option<alloc::vec::Vec<u8>> {
        let idx = self.rx_index;
        
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        
        if unsafe { core::ptr::read_volatile(&self.rx_ring[idx].status) & 1 } == 1 { // Descriptor Done (DD)
            let raw_len = unsafe { core::ptr::read_volatile(&self.rx_ring[idx].length) } as usize;
            let len = core::cmp::min(raw_len, BUFFER_SIZE);
            let mut packet = vec![0; len];
            packet.copy_from_slice(&self.rx_buffers[idx][..len]);
            
            // Zurücksetzen für den NIC
            unsafe { core::ptr::write_volatile(&mut self.rx_ring[idx].status, 0); }
            core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
            
            self.write_register(REG_RDT, idx as u32);
            self.rx_index = (self.rx_index + 1) % RX_RING_SIZE;
            
            Some(packet)
        } else {
            None
        }
    }

    pub fn write_register(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile((self.mmio_base + offset) as *mut u32, value);
        }
    }

    pub fn read_register(&self, offset: usize) -> u32 {
        unsafe {
            read_volatile((self.mmio_base + offset) as *const u32)
        }
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }
}

pub fn init_e1000(devices: &[PciDevice], phys_offset: u64) -> Option<E1000> {
    for dev in devices {
        if dev.vendor_id == E1000_VENDOR_ID && dev.device_id == E1000_DEVICE_ID {
            let mut nic = E1000::new(dev, phys_offset);
            nic.init();
            return Some(nic);
        }
    }
    None
}
