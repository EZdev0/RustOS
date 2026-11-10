//! Hardware-independent NIC layer.
//!
//! The kernel must not care which Ethernet chip the machine (or emulator) has:
//! Limbo and older QEMU setups default to a Realtek RTL8139, QEMU's PC target
//! defaults to an Intel e1000, and a real machine may have either of them or
//! something else entirely.
//!
//! So detection walks the PCI bus, filters network controllers (class `0x02`)
//! and dispatches to a driver by vendor/device ID. Everything the network stack
//! needs is expressed on [`Nic`] — a plain enum, not a trait object: this is a
//! `no_std` kernel, and a `dyn` dispatch would drag in allocation and vtable
//! indirection for no benefit.
//!
//! Adding a driver:
//!
//! 1. implement `new(pci_device, phys_offset) -> Option<Self>`, `init`,
//!    `transmit`, `receive`, `can_transmit` exactly like the existing ones,
//! 2. add a variant to [`Nic`] and a match arm in [`probe`],
//! 3. add the device ID to `docs/network_hardware.md` and to the boot test
//!    matrix in `tools/rustos.py` if QEMU can emulate it.

use alloc::vec::Vec;

use crate::hardware::e1000::{self, E1000};
use crate::hardware::pci::PciDevice;
use crate::hardware::rtl8139::{self, Rtl8139};

/// PCI class code for network controllers.
const PCI_CLASS_NETWORK: u8 = 0x02;

/// One NIC, whatever the chip behind it is.
pub enum Nic {
    /// Intel 8254x family (QEMU's default `-device e1000`).
    E1000(E1000),
    /// Realtek RTL8139 (Limbo's default, VirtualBox's "PCnet-FAST III" sibling).
    Rtl8139(Rtl8139),
}

impl Nic {
    /// Short driver name for the serial log.
    pub fn name(&self) -> &'static str {
        match self {
            Nic::E1000(_) => "e1000",
            Nic::Rtl8139(_) => "rtl8139",
        }
    }

    pub fn mac_address(&self) -> [u8; 6] {
        match self {
            Nic::E1000(nic) => nic.mac_address(),
            Nic::Rtl8139(nic) => nic.mac_address(),
        }
    }

    pub fn bar_address(&self) -> u64 {
        match self {
            Nic::E1000(nic) => nic.bar_address(),
            Nic::Rtl8139(nic) => nic.bar_address(),
        }
    }

    pub fn can_transmit(&self) -> bool {
        match self {
            Nic::E1000(nic) => nic.can_transmit(),
            Nic::Rtl8139(nic) => nic.can_transmit(),
        }
    }

    pub fn transmit(&mut self, data: &[u8]) -> bool {
        match self {
            Nic::E1000(nic) => {
                if !nic.can_transmit() {
                    return false;
                }
                nic.transmit(data);
                true
            }
            Nic::Rtl8139(nic) => nic.transmit(data),
        }
    }

    pub fn receive(&mut self) -> Option<Vec<u8>> {
        match self {
            Nic::E1000(nic) => nic.receive(),
            Nic::Rtl8139(nic) => nic.receive(),
        }
    }
}

/// Find the first NIC we have a driver for.
///
/// Devices without a driver are logged once with their IDs, so a user can tell
/// "no NIC at all" apart from "your NIC is not supported yet" — the boot test
/// and the serial log stay honest either way.
pub fn probe(devices: &[PciDevice], phys_offset: u64) -> Option<Nic> {
    let mut network_devices = 0usize;
    let mut tried = 0usize;

    for device in devices {
        if device.class != PCI_CLASS_NETWORK && !is_known_nic(device) {
            continue;
        }
        network_devices += 1;

        if is_known_nic(device) {
            tried += 1;
            if let Some(nic) = e1000::probe_one(device, phys_offset) {
                return Some(Nic::E1000(nic));
            }
            if let Some(nic) = rtl8139::probe_one(device, phys_offset) {
                return Some(Nic::Rtl8139(nic));
            }
        }

        crate::serial_println!(
            "[net] unsupported NIC {:04x}:{:04x} (class {:02x}.{:02x}, bar0={:#010x}) - skipped",
            device.vendor_id,
            device.device_id,
            device.class,
            device.subclass,
            device.bar0
        );
    }

    if network_devices == 0 {
        crate::serial_println!("[net] no PCI network controller found");
    } else if tried == 0 {
        crate::serial_println!(
            "[net] {} network controller(s) found, none supported by this build",
            network_devices
        );
    }
    None
}

/// True for the vendor/device pairs a driver exists for.
fn is_known_nic(device: &PciDevice) -> bool {
    e1000::is_supported(device) || rtl8139::is_supported(device)
}
