use crate::hardware::pci::{pci_read_config_32, pci_write_config_32, PciDevice};
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

const E1000_VENDOR_ID: u16 = 0x8086;

/// Device IDs of the 8254x family. QEMU/Limbo expose 0x100E (82540EM) or
/// 0x100F (82545EM), real 82574L cards report 0x10D3, and older boards use the
/// remaining IDs — all of them share the register layout this driver programs.
const E1000_DEVICE_IDS: [u16; 9] = [
    0x100E, // 82540EM (QEMU `-device e1000`, older Limbo)
    0x100F, // 82545EM (Limbo e1000, VMware e1000)
    0x1004, // 82543GC
    0x1015, // 82540EP
    0x1016, // 82540EP (mobile)
    0x1017, // 82540EP (mobile)
    0x10D3, // 82574L (QEMU `e1000e`, many real boards)
    0x10D5, // 82574IT
    0x10D6, // 82566DM
];

/// True when `device` is an Intel 8254x NIC this driver can program.
pub fn is_supported(device: &PciDevice) -> bool {
    device.vendor_id == E1000_VENDOR_ID && E1000_DEVICE_IDS.contains(&device.device_id)
}

const REG_CTRL: usize = 0x0000;
const REG_STATUS: usize = 0x0008;
const REG_MDIC: usize = 0x0020; // MDI Control (PHY register access)
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

/// The register window this driver touches (RAL/RAH at 0x5400 is the last one).
const MMIO_WINDOW: u64 = 0x6000;

// Boot-time diagnostics: the first few transmit/receive events are logged so a
// dead direction is visible in the serial log instead of being guessed at.
static TX_SEEN: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static TX_DROPPED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static RX_PROBED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

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
    /// Physical address of BAR0 (what the device decodes).
    bar_physical: u64,
    /// Mapped virtual address of the same window (`phys_offset + bar`).
    mmio_base: usize,
    mac_address: [u8; 6],
    rx_ring: Vec<RxDesc>,
    tx_ring: Vec<TxDesc>,
    rx_buffers: Vec<[u8; BUFFER_SIZE]>,
    tx_buffers: Vec<[u8; BUFFER_SIZE]>,
    /// Physical addresses of the buffers above (the NIC DMAs to these).
    rx_physical: Vec<u64>,
    tx_physical: Vec<u64>,
    rx_index: usize,
    tx_index: usize,
    phys_offset: u64,
}

impl E1000 {
    pub fn new(pci_device: &PciDevice, phys_offset: u64) -> Self {
        let mut bar_physical = (pci_device.bar0 & !0xF) as u64;

        // SAFETY FIX: In some QEMU configurations without SeaBIOS, BAR0 is uninitialized (0).
        // If we write to address 0, we corrupt the IDT/GDT, causing #GP or #DF.
        if bar_physical == 0 {
            bar_physical = 0xFEB8_0000;
            pci_write_config_32(
                pci_device.bus,
                pci_device.device,
                pci_device.function,
                0x10,
                bar_physical as u32,
            );
        }

        // The device decodes the *physical* address; the CPU needs the mapping
        // the bootloader installed for physical memory (see BOOTLOADER_CONFIG).
        let mmio_base = (phys_offset + bar_physical) as usize;

        // Aktiviere Bus Mastering und Memory Space.
        //
        // Bus Mastering ist *nicht* optional: QEMU (und echte Hardware)
        // verwerfen jedes eingehende Paket, solange PCI COMMAND.MASTER fehlt
        // (`e1000x_rx_ready()`), während DMA-Reads für den TX-Ring weiter
        // funktionieren. Das Fehlerbild ist dann "TX läuft, RX ist stumm" -
        // exakt das, was ein halb defektes Kabel vortäuscht. Deshalb wird der
        // Wert nach dem Schreiben zurückgelesen und geloggt.
        let cmd_reg =
            pci_read_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x04);
        pci_write_config_32(
            pci_device.bus,
            pci_device.device,
            pci_device.function,
            0x04,
            cmd_reg | 0x04 | 0x02,
        );
        let mut cmd_after =
            pci_read_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x04);
        if cmd_after & 0x04 == 0 {
            // Manche Chipsets ignorieren das obere Halbwort eines 32-Bit
            // Schreibzugriffs auf 0x04. Zweiter Versuch: Status-Bits (oberes
            // Halbwort) ausnullen, nur Command setzen.
            pci_write_config_32(
                pci_device.bus,
                pci_device.device,
                pci_device.function,
                0x04,
                (cmd_after & 0xFFFF) | 0x06,
            );
            cmd_after =
                pci_read_config_32(pci_device.bus, pci_device.device, pci_device.function, 0x04);
        }
        crate::serial_println!(
            "[net] e1000 pci: command={:#06x} master={} memory={}",
            cmd_after & 0xFFFF,
            if cmd_after & 0x04 != 0 { "yes" } else { "no" },
            if cmd_after & 0x02 != 0 { "yes" } else { "no" }
        );

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
            bar_physical,
            mmio_base,
            mac_address: mac,
            rx_ring: Vec::new(),
            tx_ring: Vec::new(),
            rx_buffers: Vec::new(),
            tx_buffers: Vec::new(),
            rx_physical: Vec::new(),
            tx_physical: Vec::new(),
            rx_index: 0,
            tx_index: 0,
            phys_offset,
        }
    }

    /// Physical address of `address`, or `None` when DMA would be unsafe.
    fn physical(address: u64, phys_offset: u64) -> Option<u64> {
        crate::paging::physical_address(address, phys_offset)
    }

    /// Program the device. Returns `false` when DMA buffers cannot be addressed
    /// physically (the caller must then not use the card at all).
    pub fn init(&mut self) -> bool {
        unsafe {
            // Disable all interrupts (Mask Clear) to prevent interrupt storms
            self.write_register(REG_IMC, 0xFFFF_FFFF);

            // Set Link Up
            let mut ctrl = read_volatile((self.mmio_base + REG_CTRL) as *const u32);
            ctrl |= 0x40; // Setze SLU (Set Link Up)
            write_volatile((self.mmio_base + REG_CTRL) as *mut u32, ctrl);

            // Konfiguriere Transmit (TX) und Receive (RX) Ringe im Speicher
            self.rx_ring = vec![
                RxDesc {
                    addr: 0,
                    length: 0,
                    checksum: 0,
                    status: 0,
                    errors: 0,
                    special: 0
                };
                RX_RING_SIZE
            ];
            self.tx_ring = vec![
                TxDesc {
                    addr: 0,
                    length: 0,
                    cso: 0,
                    cmd: 0,
                    status: 0,
                    css: 0,
                    special: 0
                };
                TX_RING_SIZE
            ];
            self.rx_buffers = vec![[0u8; BUFFER_SIZE]; RX_RING_SIZE];
            self.tx_buffers = vec![[0u8; BUFFER_SIZE]; TX_RING_SIZE];

            // The NIC DMAs to physical addresses: resolve every buffer once.
            self.rx_physical.clear();
            self.tx_physical.clear();
            for i in 0..RX_RING_SIZE {
                match Self::physical(self.rx_buffers[i].as_ptr() as u64, self.phys_offset) {
                    Some(address) => self.rx_physical.push(address),
                    None => return false,
                }
            }
            for i in 0..TX_RING_SIZE {
                match Self::physical(self.tx_buffers[i].as_ptr() as u64, self.phys_offset) {
                    Some(address) => self.tx_physical.push(address),
                    None => return false,
                }
            }
            // RX
            for i in 0..RX_RING_SIZE {
                self.rx_ring[i].addr = self.rx_physical[i];
            }

            let Some(rx_addr) = Self::physical(self.rx_ring.as_ptr() as u64, self.phys_offset)
            else {
                return false;
            };
            self.write_register(REG_RDBAL, (rx_addr & 0xFFFF_FFFF) as u32);
            self.write_register(REG_RDBAH, (rx_addr >> 32) as u32);
            self.write_register(
                REG_RDLEN,
                (RX_RING_SIZE * core::mem::size_of::<RxDesc>()) as u32,
            );
            self.write_register(REG_RDH, 0);
            self.write_register(REG_RDT, (RX_RING_SIZE - 1) as u32);

            // RCTL: EN (1<<1) | UPE (1<<3) | MPE (1<<4) | BAM (1<<15) |
            //       SECRC (1<<26, strip the FCS) | BSIZE 2048 (0<<16).
            // UPE/MPE/BAM make the filter fully permissive: a filter mismatch
            // would silently drop every frame and look like a dead link.
            self.write_register(
                REG_RCTL,
                (1 << 1) | (1 << 3) | (1 << 4) | (1 << 15) | (1 << 26),
            );

            // TX
            for i in 0..TX_RING_SIZE {
                self.tx_ring[i].addr = self.tx_physical[i];
            }

            let Some(tx_addr) = Self::physical(self.tx_ring.as_ptr() as u64, self.phys_offset)
            else {
                return false;
            };
            self.write_register(REG_TDBAL, (tx_addr & 0xFFFF_FFFF) as u32);
            self.write_register(REG_TDBAH, (tx_addr >> 32) as u32);
            self.write_register(
                REG_TDLEN,
                (TX_RING_SIZE * core::mem::size_of::<TxDesc>()) as u32,
            );
            self.write_register(REG_TDH, 0);
            self.write_register(REG_TDT, 0);

            // TCTL: EN (1<<1) | PSP (1<<3) | CT = 0x10 (bits 4..11) |
            // COLD = 0x40 (bits 12..21). CT/COLD at their reset value are what
            // Intel's driver programs; leaving them at 0 makes some models
            // refuse to start a transmission.
            self.write_register(REG_TCTL, (1 << 1) | (1 << 3) | (0x10 << 4) | (0x40 << 12));

            // Read back the programming: if a register did not stick, the
            // cause is the mapping, not the network.
            crate::serial_println!(
                "[net] e1000 regs: ctrl={:#010x} status={:#010x} rctl={:#010x} tctl={:#010x} rdt={} tdt={} rdlen={}",
                self.read_register(REG_CTRL),
                self.read_register(REG_STATUS),
                self.read_register(REG_RCTL),
                self.read_register(REG_TCTL),
                self.read_register(REG_RDT),
                self.read_register(REG_TDT),
                self.read_register(REG_RDLEN)
            );
            crate::serial_println!(
                "[net] e1000 dma: rx_ring={:#x} rx_buf0={:#x} tx_ring={:#x} tx_buf0={:#x}",
                rx_addr,
                self.rx_physical[0],
                tx_addr,
                self.tx_physical[0]
            );
        }

        // Prove that TX and RX rings, the descriptor layout and every DMA
        // address work *without* a peer on the wire. Without this test a dead
        // ring and a filtered link look identical from the outside.
        let loopback = self.loopback_test();
        crate::serial_println!(
            "[net] e1000 loopback: {} (tdh={} rdh={} tx_status={:#04x} rx_status={:#04x})",
            if loopback { "ok" } else { "failed" },
            self.read_register(REG_TDH),
            self.read_register(REG_RDH),
            self.tx_ring[0].status,
            self.rx_ring[0].status
        );
        true
    }

    /// Read one PHY register through the MDIC register (offset 0x20).
    ///
    /// Layout: data[15:0], reg[20:16], phy[25:21], op[27:26], ready[28],
    /// error[30]. PHY address 1 is what the 8254x/8257x family uses.
    fn mdic_read(&self, reg: u8) -> Option<u16> {
        const MDIC_PHY: u32 = 1;
        const MDIC_OP_READ: u32 = 0b10 << 26;
        let command = MDIC_OP_READ | (MDIC_PHY << 21) | ((reg as u32 & 0x1F) << 16);
        self.write_register(REG_MDIC, command);
        // A real PHY needs a few microseconds; QEMU answers immediately. The
        // bound keeps a broken card from stalling the boot.
        for _ in 0..1_000 {
            let value = self.read_register(REG_MDIC);
            if value & (1 << 28) != 0 {
                // READY: bit 30 signals an error (no PHY / unimplemented reg).
                return if value & (1 << 30) != 0 {
                    None
                } else {
                    Some((value & 0xFFFF) as u16)
                };
            }
        }
        None
    }

    /// Write one PHY register through MDIC. Returns `false` on error/timeout.
    fn mdic_write(&self, reg: u8, value: u16) -> bool {
        const MDIC_PHY: u32 = 1;
        const MDIC_OP_WRITE: u32 = 0b01 << 26;
        let command =
            MDIC_OP_WRITE | (MDIC_PHY << 21) | ((reg as u32 & 0x1F) << 16) | (value as u32);
        self.write_register(REG_MDIC, command);
        for _ in 0..1_000 {
            let status = self.read_register(REG_MDIC);
            if status & (1 << 28) != 0 {
                return status & (1 << 30) == 0;
            }
        }
        false
    }

    /// Loopback self test: transmit a frame to ourselves and wait for it to
    /// come back through the receive ring.
    ///
    /// Returns `true` when the frame arrived. Restores normal operation either
    /// way, and never blocks the boot for long (bounded poll count).
    ///
    /// The loopback is enabled in PHY register MII_BMCR (bit 14), *not* via
    /// RCTL[6:7]: QEMU's e1000 model implements only the PHY loopback (it
    /// calls `qemu_receive_packet()` when MII_BMCR_LOOPBACK is set) and would
    /// otherwise ignore the test frame completely - a false "RX is broken".
    fn loopback_test(&mut self) -> bool {
        const MII_BMCR: u8 = 0x00;
        const MII_BMCR_LOOPBACK: u16 = 0x4000;
        const TEST_ETHERTYPE: [u8; 2] = [0x88, 0xB5];

        // Keep the TX/RX indices out of the test's way: remember them and put
        // the rings back into the state they were in.
        let saved_rx_index = self.rx_index;
        let saved_tx_index = self.tx_index;

        // Without a readable/writable PHY the test cannot run. Report `false`
        // (the log then says why) instead of pretending the ring is fine.
        let Some(bmcr) = self.mdic_read(MII_BMCR) else {
            return false;
        };
        if !self.mdic_write(MII_BMCR, bmcr | MII_BMCR_LOOPBACK) {
            return false;
        }

        let mac = self.mac_address;
        let mut frame = [0u8; 64];
        frame[0..6].copy_from_slice(&mac);
        frame[6..12].copy_from_slice(&mac);
        frame[12..14].copy_from_slice(&TEST_ETHERTYPE);
        for (index, byte) in frame[14..].iter_mut().enumerate() {
            *byte = index as u8;
        }
        self.transmit(&frame);

        let mut found = false;
        for _ in 0..100_000 {
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

        // Leave loopback mode again. RCTL is deliberately untouched: EN and
        // the permissive filter must survive the test, otherwise the card
        // would come up "successfully" and then drop every frame.
        self.mdic_write(MII_BMCR, bmcr);
        self.rx_index = saved_rx_index;
        self.tx_index = saved_tx_index;
        found
    }

    pub fn can_transmit(&self) -> bool {
        let idx = self.tx_index;
        // Check if Descriptor Done (DD) bit is set or if status is 0 (unused)
        unsafe {
            core::ptr::read_volatile(&self.tx_ring[idx].status) & 1 == 1
                || self.tx_ring[idx].status == 0
        }
    }

    pub fn transmit(&mut self, data: &[u8]) {
        if data.len() > BUFFER_SIZE || !self.can_transmit() {
            let dropped = TX_DROPPED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            if dropped < 6 {
                crate::serial_println!(
                    "[net] e1000 tx dropped: len={} fits={} can_transmit={} idx={} status={:#x}",
                    data.len(),
                    data.len() <= BUFFER_SIZE,
                    self.can_transmit(),
                    self.tx_index,
                    self.tx_ring[self.tx_index].status
                );
            }
            return;
        }

        let idx = self.tx_index;
        let seen = TX_SEEN.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if seen < 6 {
            crate::serial_println!(
                "[net] e1000 tx #{} len={} idx={} tdt={}",
                seen,
                data.len(),
                idx,
                self.read_register(REG_TDT)
            );
        }

        self.tx_buffers[idx][..data.len()].copy_from_slice(data);

        // Physical address computed once in `init`; the buffer never moves.
        self.tx_ring[idx].addr = self.tx_physical[idx];
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

        let status = unsafe { core::ptr::read_volatile(&self.rx_ring[idx].status) };
        let probed = RX_PROBED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // The first polls show whether the chip ever sets the descriptor-done
        // bit at all (the network task polls millions of times per second, so
        // more logging would drown the serial log).
        if probed < 8 {
            crate::serial_println!(
                "[net] e1000 rx probe #{} idx={} status={:#04x} dd={}",
                probed,
                idx,
                status,
                status & 1
            );
        }

        if status & 1 == 1 {
            // Descriptor Done (DD)
            let raw_len = unsafe { core::ptr::read_volatile(&self.rx_ring[idx].length) } as usize;
            let len = core::cmp::min(raw_len, BUFFER_SIZE);
            let mut packet = vec![0; len];
            packet.copy_from_slice(&self.rx_buffers[idx][..len]);

            // Zurücksetzen für den NIC
            unsafe {
                core::ptr::write_volatile(&mut self.rx_ring[idx].status, 0);
            }
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
        unsafe { read_volatile((self.mmio_base + offset) as *const u32) }
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }

    /// Physical base address of the MMIO window (BAR0).
    pub fn bar_address(&self) -> u64 {
        self.bar_physical
    }
}

/// Bring up the NIC behind `dev`, or explain why not.
///
/// Split out of [`init_e1000`] so the generic NIC probe can try drivers one by
/// one and fall through to the next device when one of them refuses.
pub fn probe_one(dev: &PciDevice, phys_offset: u64) -> Option<E1000> {
    if !is_supported(dev) {
        return None;
    }

    // The CPU reaches MMIO through the bootloader's physical-memory mapping;
    // touching an unmapped BAR would page fault, so refuse instead of crashing.
    let mut bar = (dev.bar0 & !0xF) as u64;
    if bar == 0 {
        bar = 0xFEB8_0000; // QEMU's default e1000 BAR
    }
    let mapped = phys_offset.saturating_add(bar);
    if phys_offset == 0 || !crate::paging::is_range_mapped(mapped, MMIO_WINDOW, phys_offset) {
        crate::serial_println!(
            "[net] e1000 at {:#x} is not mapped (phys offset {:#x}) - driver skipped",
            bar,
            phys_offset
        );
        return None;
    }

    let mut nic = E1000::new(dev, phys_offset);
    if !nic.init() {
        crate::serial_println!("[net] e1000 DMA buffers are not addressable - driver skipped");
        return None;
    }
    let mac = nic.mac_address();
    crate::serial_println!(
        "[net] e1000 up (device {:04x}): bar={:#x} mmio={:#x} mac={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        dev.device_id,
        nic.bar_address(),
        mapped,
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5]
    );
    Some(nic)
}

/// First e1000 in `devices` (kept for callers that only care about Intel).
pub fn init_e1000(devices: &[PciDevice], phys_offset: u64) -> Option<E1000> {
    devices
        .iter()
        .find_map(|device| probe_one(device, phys_offset))
}
