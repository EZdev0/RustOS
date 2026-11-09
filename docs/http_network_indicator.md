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

### Device MMIO must be mapped by the bootloader

The kernel runs on the page tables installed by `bootloader_api` 0.11, which map
the kernel, the boot info and the framebuffer — **not** PCI BARs. Reading the
e1000's MAC register (`BAR0 + 0x5400`) therefore raised a page fault on every
boot with a NIC present, and because the panic handler used to write only to the
framebuffer, CI never noticed.

`hardware/e1000.rs::init_e1000` now asks `paging::is_range_mapped(bar, 0x6000)`
first. When the window is not mapped it logs

```text
[net] e1000 at 0xfeb80000 is not mapped by the bootloader - networking disabled
```

and returns `None`, leaving `NETWORK_MANAGER` unset. `network_task` and every
`start_http_request_*` call handle that case, so the desktop boots normally and
the browser shows "Network unavailable" instead of taking the kernel down.

Note that DMA is broken for the same reason: descriptor addresses are computed as
`virtual.saturating_sub(physical_memory_offset)`, which is only correct once the
kernel manages its own address space. Making the NIC fully functional needs the
same piece of work as mapping MMIO — tracked in
[browser_engine_2026.md](browser_engine_2026.md).

### Known limitations
* **No MMIO mapping.** A NIC is only used when its BAR is already mapped (see
  above); otherwise networking is disabled with a log line.
* **No TLS.** Only `http://` URLs are accepted; `https://` shows a clear error.
  A TLS 1.3 client with a pinned root store is on the roadmap (needs an entropy
  source and more heap than the current 16 MiB static heap leaves spare).
* **No DNS.** The browser resolves hosts through a small static table
  (`example.com`, `neverssl.com`, `info.cern.ch`, `rust-lang.org`, …) plus an
  optional `hosts` file from the in-memory filesystem. Unknown hosts produce
  "Unknown host" instead of a silent hang.
* **No redirect across hosts** beyond the 4-redirect budget enforced by
  `PageEngine` (`MAX_REDIRECTS`), which also rejects redirect loops.

## Taskbar / dock limits
The glass dock computes `max_apps_possible = width.saturating_sub(40) / 85;` and
refuses to register more icons, so the bottom bar cannot overflow the framebuffer.
This was the origin of the historic "red screen" panic, see
[dock_buffer_panic_fix.md](dock_buffer_panic_fix.md).
