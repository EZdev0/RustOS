# HTTP networking and the status indicator

## Overview
This document covers the network stack (`kernel/src/network/mod.rs`, `smoltcp`
over an Intel E1000 NIC), the HTTP client used by the browser, and the desktop
network indicator.

> **History:** the first implementation fetched `http://example.com/` into a
> single heap buffer and handed the raw bytes to the UI. It is now a proper
> streaming HTTP client: `rustos-web::http` decodes the response incrementally and
> `PageEngine` builds the page while bytes arrive.

## Network status indicator
The top-right corner of the `GraphicalCompositor` top bar shows the link state:

* **[INET]** (green) — `NetworkManager` is initialised and has a DHCP/static
  IPv4 configuration.
* **[OFF]** (red) — no link or no address.

The indicator is written by the compositor's own `draw` pass, so it costs nothing
when the desktop is idle (see
[rendering_performance.md](rendering_performance.md)).

## HTTP client

### Public API (`kernel/src/network/mod.rs`)

```rust
pub enum HttpState { Idle, Requesting, Sending, Receiving, Done, Failed }

pub fn start_http_request_raw(ip: Ipv4Address, request: alloc::vec::Vec<u8>, max_bytes: usize) -> bool;
pub fn start_http_request_with(ip: Ipv4Address, host: &str, path: &str, max_bytes: usize) -> bool;
pub fn start_http_request(ip: Ipv4Address) -> bool;          // legacy: GET / HTTP/1.1
pub fn http_state() -> HttpState;
pub fn poll_http_chunk() -> Option<alloc::vec::Vec<u8>>;      // new bytes, or None
pub fn http_failed() -> bool;
pub fn cancel_http_request();
pub fn default_request(host: &str, path: &str) -> alloc::vec::Vec<u8>;
```

`start_http_request_*` returns `false` and leaves the state machine untouched when
a transfer is already in flight or the socket set is full; callers must not retry
blindly.

### Byte budget
`max_bytes` caps the body that will be buffered. The browser passes
`MAX_PAGE_BYTES = 1_500_000`; the legacy default is 4 MiB
(`DEFAULT_MAX_RESPONSE_BYTES`). When the budget is exceeded the transfer is
aborted with `HttpState::Failed` and `http_failed() == true` — the kernel heap is
never allowed to fill up because a server keeps sending.

### Streaming contract
* `poll_http_chunk()` returns a `Vec<u8>` with the bytes received since the last
  call, or `None` when nothing new arrived. It never returns the whole body
  again, so memory usage is bounded by (chunk + decoder buffers).
* The response decoder in `rustos-web/src/http.rs` accepts arbitrary TCP slice
  boundaries; header folding, chunked transfer encoding and `Content-Length` are
  handled incrementally, and the body is exposed only after the header block is
  complete.
* `Content-Type` must be text-ish (`text/*`, `application/xhtml+xml`,
  `application/xml`, or absent). Anything else is rejected instead of being
  rendered as garbage.
* Charset: UTF-8 (with replacement characters) and Latin-1 are decoded; other
  charsets are treated as UTF-8.

### Driver/executor integration
* One `tcp::Socket` lives in the `NetworkManager` socket set.
* `network_task` polls `NetworkManager::poll()`, and whenever new response bytes
  exist it requests a repaint so the browser updates immediately — no polling loop
  in the UI task, no busy waiting.
* `http_state()` drives the browser status text ("Loading…", "Done", "Failed").

### Device MMIO is mapped by the bootloader

The kernel runs on the page tables installed by `bootloader_api` 0.11. With the
default configuration those page tables map the kernel, the boot info and the
framebuffer — **not** PCI BARs, so reading a NIC register (for example the
e1000's MAC at `BAR0 + 0x5400`) raised a page fault on every boot with a NIC
present, and because the panic handler used to write only to the framebuffer,
CI never noticed.

`BOOTLOADER_CONFIG.mappings.physical_memory` is therefore set to
`Mapping::Dynamic`, which maps physical memory — including the PCI BAR windows —
at `BootInfo::physical_memory_offset`. The drivers add that offset to a BAR and
call `paging::is_range_mapped()` before the first register access; when a window
is somehow not mapped they log

```text
[net] e1000 at 0xfeb80000 is not mapped (phys offset 0x0) - driver skipped
```

and fall through to the next driver, leaving `NETWORK_MANAGER` unset.
`network_task` and every `start_http_request_*` call handle that case, so the
desktop boots normally and the browser shows "Network unavailable" instead of
taking the kernel down.

DMA works for the same reason: descriptor and buffer addresses are translated by
`paging::physical_address()` (PML4 → PDPT → PD → PT, huge pages included), and a
buffer that cannot be translated aborts driver start-up instead of pointing the
card at garbage. See [network_hardware.md](network_hardware.md).

### Known limitations
* **Two drivers.** Intel 8254x (`e1000`) and Realtek 8139 (`rtl8139`) only;
  every other NIC is logged as `[net] unsupported NIC …` and the machine boots
  without networking. The matrix and the way to add a driver are in
  [network_hardware.md](network_hardware.md), the roadmap in
  [TODO.md](TODO.md).
* **No TLS.** Only `http://` URLs are accepted; `https://` shows a clear error.
  A TLS 1.3 client with a pinned root store is on the roadmap (needs an entropy
  source and more heap than the current 16 MiB static heap leaves spare).
* **DNS is bounded, not exhaustive.** Names are resolved by a smoltcp
  `socket-dns` client against the servers from the DHCP lease (fallback
  `10.0.2.3`) with a 16-entry cache, four in-flight queries and a
  "Resolving …" browser state; only A records are requested. A small static
  table (`example.com`, `localhost`, …) still short-circuits the query so the
  browser works on a machine without any network configuration. Unknown hosts
  produce "Unknown host" instead of a silent hang. See
  [network_hardware.md](network_hardware.md) for the log contract.
* **No redirect across hosts** beyond the 4-redirect budget enforced by
  `PageEngine` (`MAX_REDIRECTS`), which also rejects redirect loops.

## Taskbar / dock limits
The glass dock computes `max_apps_possible = width.saturating_sub(40) / 85;` and
refuses to register more icons, so the bottom bar cannot overflow the framebuffer.
This was the origin of the historic "red screen" panic, see
[dock_buffer_panic_fix.md](dock_buffer_panic_fix.md).
