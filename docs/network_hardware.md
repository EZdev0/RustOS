# Network hardware: what is supported and how detection works

RustOS does not target one network chip. Detection is generic and every driver
speaks the same small interface, so the TCP/IP stack (`kernel/src/network`)
never learns which card it runs on.

## Drivers

| Driver | Chip | Vendor:Device | Where you meet it |
| --- | --- | --- | --- |
| `e1000` | Intel 8254x family | `8086:100E`, `8086:100F`, `8086:1004`, `8086:1015`, `8086:1016`, `8086:1017`, `8086:10D3`, `8086:10D5`, `8086:10D6` | QEMU's default (`-device e1000`), Limbo "e1000", VMware, VirtualBox "Intel PRO/1000" |
| `rtl8139` | Realtek RTL8139 | `10EC:8139`, `10EC:8138`, `10EC:8136` | Limbo's default NIC, older QEMU/VirtualBox setups, many 2000s boards |

Both drivers are polling drivers: they set up DMA rings, transmit when the
stack hands them a frame, and hand received frames back. No interrupts, no
background threads — the network task polls them on every iteration.

### Not supported (yet)

Anything else, including `virtio-net`, `pcnet`, `ne2k_pci` and every USB or
Wi-Fi adapter. Such a card is **not** a crash: the probe logs its IDs and the
machine boots without networking:

```text
[net] unsupported NIC 1022:2000 (class 02.00, bar0=0x00000000) - skipped
```

## Detection order

1. `hardware::pci::scan_pci()` enumerates bus 0–255, device 0–31, function 0–7.
2. `hardware::net::probe()` keeps devices whose **PCI class is `0x02`**
   (network controller) or whose ID is in a driver table.
3. Each driver's `is_supported()` decides whether it wants the device, then
   `probe_one()` brings it up (BAR mapping check, DMA buffers, MAC read).
4. The first driver that reports success wins. Everything after it is ignored.

Because the probe is driven by PCI IDs rather than by a hardcoded "Intel" test,
a Limbo VM with `82545EM` (device `0x100F`) works exactly like QEMU's
`82540EM` (`0x100E`) — a single-ID match used to silently disable networking
there.

## Requirements the drivers rely on

* **Physical memory mapping.** Both drivers program DMA descriptors with
  *physical* addresses. `BOOTLOADER_CONFIG.mappings.physical_memory` is
  `Mapping::Dynamic`, which maps physical memory (including PCI BARs) at
  `BootInfo::physical_memory_offset`. Without that mapping a driver refuses to
  start instead of page faulting — see `docs/dock_buffer_panic_fix.md` for the
  boot-time crash this prevented.
* **BAR sanity.** A BAR of `0` (firmware did not assign one) is treated as
  "unmapped" and skipped, unless the driver knows the emulator default.
* **Page-walked DMA.** `paging::physical_address()` translates every buffer the
  card touches; a buffer that cannot be translated aborts driver start-up.
* **PCI bus mastering must actually be on.** `e1000x_rx_ready()` (and every real
  8254x) gates *reception* on `PCI COMMAND.MASTER`, while DMA reads for the TX
  ring keep working without it. A card with master off therefore transmits
  happily and never receives a single frame. The driver sets the bit, reads the
  register back and logs it:

  ```text
  [net] e1000 pci: command=0x0007 master=yes memory=yes
  ```
* **A NIC needs a backend.** `-net nic` without `-netdev user` gives QEMU a NIC
  with **no peer**: the guest's frames are accepted and dropped, so no reply can
  arrive and the receiver looks dead while all registers are perfect.
  `tools/rustos_lib.qemu_network_args()` always emits
  `-netdev user,id=net0 -device <model>,netdev=net0`.

## Loopback self test

Each e1000 start-up (before the network task runs) transmits one frame to
itself and waits for it in the RX ring:

```text
[net] e1000 dma: rx_ring=0x... rx_buf0=0x... tx_ring=0x... tx_buf0=0x...
[net] e1000 loopback: ok (tdh=1 rdh=1 tx_status=0x01 rx_status=0x01)
```

The loopback is enabled in **PHY register `MII_BMCR` bit 14** via the `MDIC`
register (0x20), not with `RCTL[6:7]`: QEMU's model only implements the PHY
loopback and ignores the RCTL bits, which produces a false "RX is broken".

`rtl8139` has the same test: it sets `TCR` bits 12:11 to `0b11` (internal
loopback) and expects the frame back in its 8 KiB ring.

A failing e1000 loopback is logged together with a note and is **not** fatal:
QEMU delivers the looped frame through `qemu_receive_packet()`, which is
outside the 8254x programming model and can drop it while the NIC is still
cold. On real silicon the line is meaningful; in CI the hard proof is
`[net] rx frame` + `[net] online`.

### rtl8139: bit 13 of `TSD` means "host owns"

The datasheet calls the bit `OWN`, but it is set at reset and the chip sets it
again after transmitting: a descriptor is handed to the chip by writing the
**length without bit 13**. QEMU refuses to transmit while the bit is set
(`TxHostOwns`) and Linux' `8139too` writes `tx_flag | len` for the same reason.
Writing `len | 0x2000` made the card ignore every frame — TX looked "queued"
(`TSD` read back with our bit, `ISR` without `TxOK`) and nothing ever left the
machine.

The test result separates the two failure classes:

* `loopback: failed` → the RX ring, the descriptor layout, the `buffer_addr`
  values or the PCI master bit are wrong (TX status and `tdh`/`rdh` are logged
  alongside).
* `loopback: ok` but no `[net] online` → the ring is fine and the problem is
  outside the card: no backend, wrong static IP, or a dropped ARP/ICMP frame.

## Serial log contract

The boot test and `tools/rustos.py qemu` assert on these lines:

```text
[boot] physical memory mapped at 0x...
[net] e1000 pci: command=0x0007 master=yes memory=yes
[net] e1000 up (device 100e): bar=0xfeb80000 mmio=0x... mac=52:54:00:12:34:56
[net] e1000 dma: rx_ring=0x... rx_buf0=0x... tx_ring=0x... tx_buf0=0x...
[net] e1000 loopback: ok (tdh=1 rdh=1 tx_status=0x01 rx_status=0x01)
[net] rtl8139 up: bar=0xc000 mac=52:54:00:12:34:56 link=up
[net] rtl8139 regs: cr=0x0c isr=0x0000 rcr=0x0000008f tcr=0x00000000 cbr=0 capr=0
[net] rtl8139 dma: rx_ring=0x... tx_buf0=0x... rx_offset=0
[net] rtl8139 loopback: ok (cbr=84 rx_offset=84 isr=0x0001 tx_status=0x80000040)
[net] dhcp: ip=10.0.2.15/24 gw=10.0.2.2 dns=10.0.2.3
[net] ip: 10.0.2.15/24 via dhcp
[net] online: 10.0.2.2 answered ping seq=0 (e1000)
[net] unsupported NIC 1af4:1000 (class 02.00, bar0=0x... ) - skipped
[net] no PCI network controller found
```

* `[net] <driver> up` — a driver claimed the card (`tools/rustos_lib.py`'s
  `NET_DEVICE_RE` matches any driver name, not just `e1000`).
* `[net] online:` — the gateway answered an ICMP echo, i.e. ARP, TX and RX work.
* `network_state()` in `tools/rustos_lib.py` maps these to
  `online` / `device-only` / `absent`.

## Addresses: static immediately, DHCP overrides

`kernel/src/network/mod.rs` configures QEMU's/Limbo's documented user-mode
address (`10.0.2.15/24`, gateway `10.0.2.2`, DNS `10.0.2.3`) **at init**, so the
machine has a source address before the first socket tries to talk.

A DHCP client (smoltcp `socket-dhcpv4`) runs next to it and replaces that setup
as soon as a server answers — address, prefix, gateway and DNS servers all come
from the lease (`[net] dhcp: ip=… gw=… dns=…`). Losing a lease falls back to the
static setup again after two seconds instead of leaving the interface without an
address.

Why in this order: the previous "wait for DHCP, fall back after N seconds"
approach left the interface address-less for the whole discovery phase, which
starved the first ARP exchange and made a perfectly healthy NIC look offline.

## Adding a driver

1. New file in `kernel/src/hardware/`, modelled on `rtl8139.rs`:
   `new(pci_device, phys_offset) -> Option<Self>`, `init`, `mac_address`,
   `can_transmit`, `transmit`, `receive`, plus `is_supported(pci_device)`.
2. Add a variant to `hardware/net.rs::Nic` and a match arm there; add the name
   to `Nic::name()` (it ends up in the serial log).
3. Register the module in `kernel/src/hardware/mod.rs`.
4. Add the device ID to the table above and, when QEMU can emulate the card,
   extend `tools/rustos.py qemu --net` plus a boot-test step in
   `.github/workflows/verify.yml`.

## Test matrix

| `--net` | What it proves |
| --- | --- |
| `none` | the desktop boots on machines without any NIC |
| `e1000` | Intel path: MMIO, DMA rings, DHCP over QEMU user-mode NAT |
| `rtl8139` | Realtek path (Limbo's default NIC): I/O-BAR registers, 8 KiB ring |

Every boot test requires `[net] online`; a driver that is up but never receives
a packet fails the run with the last 20 serial lines attached.

See also: [http_network_indicator.md](http_network_indicator.md) for the HTTP
client on top of this stack, and [limbo.md](limbo.md) for the Android setup.
