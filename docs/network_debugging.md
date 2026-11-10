# Why "no internet" happened — the complete chain of evidence

This page documents the reasoning behind the network fixes on this branch, so
the next person does not have to repeat the investigation. Every claim below is
backed by an emulator or kernel source line; the sources are quoted.

## The symptom

Both NIC drivers reported a working card and a successful transmit, but no
packet ever arrived:

```text
[net] e1000 regs: ctrl=0x00140240 status=0x80080783 rctl=0x0400801a ...
[net] e1000 tx #0 len=64 tdt=0
[net] e1000 loopback: failed (tdh=1 rdh=0 tx_status=0x01 rx_status=0x00)
[net] offline: no ping reply from 10.0.2.2
```

`tdh` advanced (`QEMU` had fetched the TX descriptor, `DD` was set) while `rdh`
stayed 0. Registers, MAC and DMA addresses looked perfect. Three obvious
"fixes" (tune `RCTL`, tune `TCTL`, retry the ping) change nothing — which is
itself a strong hint that the driver is not the problem.

## Step 1: how QEMU decides to accept a frame

`hw/net/e1000.c` / `hw/net/e1000x_common.c` (v9.0.0):

```c
bool e1000x_rx_ready(PCIDevice *d, uint32_t *mac) {
    bool link_up = mac[STATUS] & E1000_STATUS_LU;
    bool rx_enabled = mac[RCTL] & E1000_RCTL_EN;
    bool pci_master = d->config[PCI_COMMAND] & PCI_COMMAND_MASTER;
    if (!link_up || !rx_enabled || !pci_master) return false;
    return true;
}
```

So reception needs **link up**, **`RCTL.EN`** and **PCI bus mastering**. Note
what is *not* checked there: the TX path. QEMU's `pci_dma_read()` for the TX
ring works without the master bit, while reception is gated hard. That is the
mechanism behind the misleading "TX fine, RX dead" picture.

The driver now sets the master bit, reads it back and logs it:

```text
[net] e1000 pci: command=0x0107 master=yes memory=yes
```

## Step 2: the real killer — a NIC without a peer

`tools/rustos.py` started QEMU with

```text
-net nic,model=e1000          # no -netdev on purpose? no: by accident
```

`-net nic` without a backend creates a NIC whose `peer` is `NULL`. QEMU accepts
the guest's frames (`e1000_send_packet()` → `qemu_send_packet()`) and drops
them on the floor, so **nothing can ever answer**. From inside the kernel this
is indistinguishable from a dead receiver — the exact symptom above.

Fixed in `tools/rustos_lib.qemu_network_args()`:

```text
-netdev user,id=net0 -device e1000,netdev=net0
```

User-mode networking (slirp, `10.0.2.0/24`, gateway `10.0.2.2`) is also what
Limbo on Android uses with **Network mode: User**, so CI and the phone now test
the same stack.

Result, immediately:

```text
[net] e1000 tx #1 len=304                    ARP/DHCP leaves the machine
[net] rx frame #0 len=64 ethertype=0x0806    ARP reply arrives
[net] rx frame #1 len=74 ethertype=0x0800    ICMP echo reply arrives
[net] online: 10.0.2.2 answered ping seq=0 (e1000)
[net] dhcp: ip=10.0.2.15/24 gw=10.0.2.2 dns=10.0.2.3
```

## Step 3: the test that lied

The rtl8139 boot test checked panic markers only, so it stayed green while the
driver never sent a single frame. Both models with a driver (`e1000`,
`rtl8139`) must now reach `[net] online`, and the two boot tests can no longer
hide each other (`continue-on-error` + summary step, serial logs kept per
model). That is how the next three bugs were found.

## Step 4: rtl8139 — three bugs, all "TX ok, RX dead"

### 4a. `TSD` bit 13 means "host owns"

```c
/* QEMU hw/net/rtl8139.c */
if (s->TxStatus[descriptor] & TxHostOwns) {   /* 0x2000 */
    return 0;                                  /* do not transmit */
}
```

```c
/* Linux drivers/net/ethernet/realtek/8139too.c */
RTL_W32_F (TxStatus0 + (entry * sizeof (u32)), tp->tx_flag | max(len, ETH_ZLEN));
```

Linux writes the length **without** bit 13. The bit is set at reset and set
again by the chip after transmission — the driver wrote `len | 0x2000`, so the
card considered the descriptor still owned by software and ignored every frame.
`TSD` read back with our bit, `ISR` had no `TOK`, `TxStatOK` never appeared.

### 4b. `CAPR` is the read pointer minus 16

```c
/* QEMU hw/net/rtl8139.c */
avail = MOD2(s->RxBufferSize + s->RxBufPtr - s->RxBufAddr, s->RxBufferSize);
return avail == 0 || avail >= 1514 || (s->IntrMask & RxOverflow);
```

```c
s->RxBufPtr = MOD2(val + 0x10, s->RxBufferSize);   /* CAPR write */
```

With `CAPR = 8192` (what the driver wrote) and an 8192-byte ring the emulated
card computes a 16-byte backlog, `rtl8139_can_receive()` returns **false** and
no packet is ever accepted. An empty ring is `CAPR = 8192 - 16`.

### 4c. no PCI bus mastering

The rtl8139 driver never touched `PCI COMMAND`. On real silicon every DMA is
then a no-op — the card cannot read its TX buffer or write the RX ring. It is
set and verified now, exactly like the e1000 path.

## Step 5: why the loopback self tests are informational on the e1000

QEMU routes a looped frame through `qemu_receive_packet()`, which is outside the
8254x programming model (and drops the frame while the NIC is cold), and it
implements loopback only in the PHY (`MII_BMCR` bit 14), not in `RCTL[6:7]`.
The rtl8139 does the same for `TCR`: the datasheet uses bits 12:11, QEMU uses
18:17. Both drivers therefore set the documented bits *and* the emulator's, and
treat a negative result as a hint: the hard proof is `[net] rx frame` plus
`[net] online`.

## Rules of thumb for the next bug

1. **"TX ok, RX dead" is almost never a receiver bug.** Check, in this order:
   PCI bus master, the emulator's `can_receive()` conditions, then the ring
   bookkeeping (descriptor addresses, read pointer).
2. **A NIC needs a backend.** No `-netdev` means no reply, ever.
3. **Read the emulator source, not the datasheet only.** QEMU and the datasheet
   disagree in three places here (loopback bits, `TSD.OWN` polarity,
   `CAPR`/ring size). Linux' driver is the tie-breaker.
4. **A test that cannot fail is worse than no test.** The rtl8139 run proved
   that: it checked for panics and stayed green for months.
