use alloc::vec::Vec;
use x86_64::instructions::port::{Port, PortWriteOnly};

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub bar0: u32,
    pub bar1: u32,
    pub bar2: u32,
    pub bar3: u32,
    pub bar4: u32,
    pub bar5: u32,
}

pub fn pci_read_config_32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16) 
        | ((slot as u32) << 11) 
        | ((func as u32) << 8) 
        | ((offset as u32) & 0xFC);
    
    let mut address_port = PortWriteOnly::<u32>::new(PCI_CONFIG_ADDRESS);
    let mut data_port = Port::<u32>::new(PCI_CONFIG_DATA);
    
    unsafe {
        address_port.write(address);
        data_port.read()
    }
}

pub fn pci_write_config_32(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    let address: u32 = 0x8000_0000
        | ((bus as u32) << 16) 
        | ((slot as u32) << 11) 
        | ((func as u32) << 8) 
        | ((offset as u32) & 0xFC);
    
    let mut address_port = PortWriteOnly::<u32>::new(PCI_CONFIG_ADDRESS);
    let mut data_port = Port::<u32>::new(PCI_CONFIG_DATA);
    
    unsafe {
        address_port.write(address);
        data_port.write(value);
    }
}

pub fn scan_pci() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0..=255 {
        for device in 0..=31 {
            for function in 0..=7 {
                let id_reg = pci_read_config_32(bus, device, function, 0x00);
                let vendor_id = (id_reg & 0xFFFF) as u16;
                let device_id = (id_reg >> 16) as u16;
                
                if vendor_id == 0xFFFF {
                    continue; // Device doesn't exist
                }
                
                let class_reg = pci_read_config_32(bus, device, function, 0x08);
                let class = (class_reg >> 24) as u8;
                let subclass = (class_reg >> 16) as u8;
                let prog_if = (class_reg >> 8) as u8;

                let bar0 = pci_read_config_32(bus, device, function, 0x10);
                let bar1 = pci_read_config_32(bus, device, function, 0x14);
                let bar2 = pci_read_config_32(bus, device, function, 0x18);
                let bar3 = pci_read_config_32(bus, device, function, 0x1C);
                let bar4 = pci_read_config_32(bus, device, function, 0x20);
                let bar5 = pci_read_config_32(bus, device, function, 0x24);

                devices.push(PciDevice {
                    bus,
                    device,
                    function,
                    vendor_id,
                    device_id,
                    class,
                    subclass,
                    prog_if,
                    bar0,
                    bar1,
                    bar2,
                    bar3,
                    bar4,
                    bar5,
                });
            }
        }
    }
    devices
}
