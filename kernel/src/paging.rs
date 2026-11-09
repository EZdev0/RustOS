//! Minimal page-table introspection.
//!
//! RustOS does not build its own address space yet: it runs on the page tables
//! the bootloader installed. Those tables map the kernel, the boot info and the
//! framebuffer — but **not** device MMIO such as a PCI BAR. Touching an
//! unmapped BAR raises a page fault, and the fault handler turns that into a
//! red screen (previously without any serial output, so CI stayed green).
//!
//! This module answers one question without side effects:
//! "is this virtual address mapped right now?". Drivers can then refuse to talk
//! to a device instead of taking the kernel down.

use x86_64::registers::control::Cr3;

/// Physical-address mask for a 4 KiB-aligned page-table entry (bits 12..=51).
const ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;
/// "Present" bit of a page-table entry.
const PRESENT: u64 = 1;
/// "Page size" bit: 1 GiB pages in a PDPT, 2 MiB pages in a PD.
const HUGE_PAGE: u64 = 1 << 7;

fn read_entry(table_physical: u64, offset: u64, index: u64) -> Option<u64> {
    let address = table_physical.checked_add(offset)?.checked_add(index * 8)?;
    // SAFETY: `offset` is the bootloader's physical-memory mapping, so every
    // page-table frame is readable through it. The read is volatile to keep the
    // compiler from caching a table that the CPU may have changed.
    Some(unsafe { core::ptr::read_volatile(address as *const u64) })
}

/// True when `virtual_address` is backed by a present page-table entry.
///
/// Returns `false` when the address space cannot be inspected (no physical
/// memory offset, unmapped table) — callers must treat that as "not usable".
pub fn is_mapped(virtual_address: u64, physical_memory_offset: u64) -> bool {
    if physical_memory_offset == 0 {
        return false;
    }

    let (level_4, _) = Cr3::read();
    let mut table = level_4.start_address().as_u64();
    if table == 0 {
        return false;
    }

    let indices = [
        (virtual_address >> 39) & 0x1ff, // PML4
        (virtual_address >> 30) & 0x1ff, // PDPT
        (virtual_address >> 21) & 0x1ff, // PD
        (virtual_address >> 12) & 0x1ff, // PT
    ];

    for (level, index) in indices.iter().enumerate() {
        let Some(entry) = read_entry(table, physical_memory_offset, *index) else {
            return false;
        };
        if entry & PRESENT == 0 {
            return false;
        }
        // A huge page (1 GiB at level 1, 2 MiB at level 2) covers the address.
        let huge_here = matches!(level, 1 | 2) && (entry & HUGE_PAGE) != 0;
        if huge_here {
            return true;
        }
        table = entry & ADDRESS_MASK;
        if table == 0 {
            return false;
        }
    }

    true
}

/// True when every byte of `[start, start + length)` is mapped.
pub fn is_range_mapped(start: u64, length: u64, physical_memory_offset: u64) -> bool {
    let Some(end) = start.checked_add(length) else {
        return false;
    };
    let mut page = start & !0xfff;
    while page < end {
        if !is_mapped(page, physical_memory_offset) {
            return false;
        }
        match page.checked_add(0x1000) {
            Some(next) => page = next,
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    // The page-table walk needs a running MMU and real CR3, so only the pure
    // arithmetic is covered here.
    use super::ADDRESS_MASK;

    #[test]
    fn address_mask_keeps_physical_bits() {
        assert_eq!(0x1234_5678_9abc_d001 & ADDRESS_MASK, 0x1234_5678_9abc_d000);
        assert_eq!(0x0000_0000_0000_0fff & ADDRESS_MASK, 0);
    }
}
