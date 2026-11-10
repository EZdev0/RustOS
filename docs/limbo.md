# Running RustOS in Limbo (Android) - and why it failed before

Limbo PC Emulator is a QEMU port for Android. It boots the same
`my_rust_os_desktop.img` that the CI builds, but it needs the right machine
settings - and the kernel used to be picky about the emulated hardware.

## Quick start

1. Download `my_rust_os_desktop.img` from the latest release
   (`Releases → ✅ RustOS Stable Image Update v…`).
2. Copy it to your phone, e.g. `Download/rustos.img`.
3. Limbo: **Create machine**
   * **Architecture / Machine**: `x86_64` / `pc`
   * **CPU model**: anything 64-bit (`qemu64`, "default (64)", `Core 2 Duo`,
     `EPYC`, `athlon` (64-bit)). **A 32-bit CPU model cannot boot this image.**
   * **RAM**: 256 MB or more (the kernel maps physical memory including PCI
     BARs; 128 MB is the absolute floor)
   * **Hard disk**: `Download/rustos.img`, interface `IDE`, format `raw`
   * **CD-ROM**: none
   * **Boot device**: hard disk
   * **Display**: `std` (VGA) — the compositor writes a plain linear
     framebuffer
4. **Network**
   * **Network mode**: `User` (NAT). `TAP` only works on rooted devices with a
     tun driver and needs a DHCP server on the other side.
   * **Network card**: `e1000` **or** `rtl8139` — both are supported now.
     `ne2k_pci`, `pcnet`, `i82551` and `virtio` are not (yet); the kernel logs
     `[net] unsupported NIC …` and boots without networking.
   * **DNS address**: `10.0.2.3` (QEMU/Limbo user-mode DNS). RustOS asks DHCP
     for the rest and uses this server for name resolution.
5. Start the machine.

## What you should see in Limbo's console log

```text
[boot] physical memory mapped at 0x...
[net] e1000 pci: command=0x0007 master=yes memory=yes
[net] e1000 up (device 100f): bar=0xfeb80000 mmio=... mac=52:54:00:12:34:56
[net] e1000 dma: rx_ring=0x... rx_buf0=0x... tx_ring=0x... tx_buf0=0x...
[net] e1000 loopback: ok (tdh=1 rdh=1 tx_status=0x01 rx_status=0x01)
[net] static ip 10.0.2.15/24 gw=10.0.2.2 (dhcp will override)
[net] default route via 10.0.2.2
[net] dhcp: ip=10.0.2.15/24 gw=10.0.2.2 dns=10.0.2.3
[net] ip: 10.0.2.15/24 via dhcp
[net] rx frame #0 len=110 ethertype=0x0806
[net] online: 10.0.2.2 answered ping seq=0 (e1000)
```

Then open the browser (desktop icon) and type `http://example.com/`.

## Why it did not work before

Three separate problems, all fixed on this branch:

1. **Wrong PCI ID.** The driver only accepted `8086:100E`. Limbo's `e1000`
   emulates the **82545EM** (`8086:100F`), so the kernel found the card, did not
   recognise it and quietly booted without a network device. Now the whole
   8254x family is matched, and `rtl8139` (Limbo's default card) has its own
   driver.
2. **Static address only.** The stack hardcoded `10.0.2.15/24` and gateway
   `10.0.2.2`. That happens to be QEMU/Limbo user-mode NAT — but only if the
   garbage collector of settings lands exactly there, and it breaks on any TAP
   or bridged setup. There is a DHCP client now, with the old static setup as a
   fallback.
3. **No DNS.** The browser could only open literal IPv4 addresses or three
   built-in names. A DNS resolver (smoltcp `socket-dns`) and a "Resolving …"
   state in the browser were added.
4. **No backend on the CI side.** The boot test started QEMU with
   `-net nic,model=e1000` and no `-netdev` backend, so the emulated NIC had no
   peer: every frame the kernel sent was dropped by QEMU and no answer could
   ever arrive. Inside the kernel this looked exactly like a dead receiver
   (TX descriptor `DD` set, `tdh` advancing, `rdh` frozen at 0). The boot test
   now always uses `-netdev user,id=net0 -device <model>,netdev=net0`, i.e. the
   same slirp stack Limbo runs with **Network mode: User**.

Diagnose order when `[net] online` stays missing:

| Log line | Meaning |
| --- | --- |
| `[net] e1000 pci: ... master=no` | PCI bus mastering could not be enabled — reception is impossible |
| `[net] e1000 loopback: failed (tdh=… rdh=0 …)` | RX ring/descriptors never written by the chip |
| `[net] e1000 loopback: ok`, no `[net] online` | Card fine; no backend, wrong IP/gateway or ARP/ICMP dropped |
| `[net] unsupported NIC …` | Limbo picked a card without a driver (use `e1000`/`rtl8139`) |
| `[net] offline: no network device` | No PCI network controller visible at all |

## 32 bit vs 64 bit

RustOS is an **x86_64 only** kernel: it is loaded by the `bootloader` crate as
a 64-bit ELF and switches to long mode immediately. Limbo CPU models without
64-bit support (`486`, `pentium`, `qemu32`) cannot run it — the machine ends up
in a reboot loop or shows a blank screen. The same applies to real hardware
without x86_64 support.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| Reboot loop / black screen | 32-bit CPU model, or less than ~128 MB RAM |
| Desktop boots, no network | NIC model unsupported (`ne2k_pci`, `virtio`); check `[net] unsupported NIC` |
| `[net] offline: no ping reply` | `Network mode` is `none`, or the emulator has no NAT |
| `Unknown host` in the browser | DNS server not set (use `10.0.2.3`), or the name really does not exist |
| Image does not appear in the disk picker | use the `.img` release asset, not the source archive |

Related: [network_hardware.md](network_hardware.md) for the driver matrix,
[ci.md](ci.md) for the boot tests that cover both NIC models.
