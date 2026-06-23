use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use crate::hardware::pci::{PciDevice, pci_read_config_32, pci_write_config_32};
use x86_64::VirtAddr;

const E1000_VENDOR_ID: u16 = 0x8086;
const E1000_DEVICE_ID: u16 = 0x100E;

const REG_CTRL: usize = 0x0000;
const REG_RCTL: usize = 0x0100;
const REG_TCTL: usize = 0x0400;

pub struct E1000 {
    mmio_base: usize,
    mac_address: [u8; 6],
}

impl E1000 {
    pub fn new(pci_device: &PciDevice) -> Self {
        let mmio_base = (pci_device.bar0 & !0xF) as usize;
        
        // Aktiviere Bus Mastering (PCI Command Register Offset 0x04)
        let cmd_reg = pci_read_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x04);
        pci_write_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x04, cmd_reg | 0x04);

        // Wir lesen die MAC Adresse rudimentär via Eeprom oder MMIO Registers
        // Einfache Variante: MAC steht in den Registern 0x5400 (RAL0) und 0x5404 (RAH0)
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
        }
    }

    pub fn init(&mut self) {
        unsafe {
            // Set Link Up
            let mut ctrl = read_volatile((self.mmio_base + REG_CTRL) as *const u32);
            ctrl |= 0x40; // Setze SLU (Set Link Up)
            write_volatile((self.mmio_base + REG_CTRL) as *mut u32, ctrl);
            
            // TODO: Konfiguriere Transmit (TX) und Receive (RX) Ringe im Speicher
            // Für echte Netzwerkkommunikation brauchen wir DMA-Speicherbereiche
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

pub fn init_e1000(devices: &[PciDevice]) -> Option<E1000> {
    for dev in devices {
        if dev.vendor_id == E1000_VENDOR_ID && dev.device_id == E1000_DEVICE_ID {
            let mut nic = E1000::new(dev);
            nic.init();
            return Some(nic);
        }
    }
    None
}
