# 🦀 RustOS — a bare-metal desktop written in Rust

RustOS is a small but complete `no_std` operating system for `x86_64` PCs: it boots
from BIOS, brings up a graphical compositor, a window manager, an asynchronous
task executor, a TCP/IP stack and — since this change — a real **HTML/CSS
rendering engine**.

It is designed to stay readable (≈8 kLOC of Rust) and to run everywhere from a
256 MiB QEMU guest to [Limbo PC Emulator](https://play.google.com/store/apps/details?id=com.kobyl.ai) on Android
phones.

```text
┌──────────────────────────────────────────────────────────────────────┐
│ top bar: clock · network indicator                                   │
│ ┌────────────────────────────┐   ┌───────────────────────────────┐   │
│ │ Web Browser                │   │ Terminal                      │   │
│ │  URL bar / status          │   │  $ ls                         │   │
│ │  rendered HTML + CSS       │   │  $ cat /readme.txt            │   │
│ └────────────────────────────┘   └───────────────────────────────┘   │
│                        [dock: 7 apps]                                │
└──────────────────────────────────────────────────────────────────────┘
```

---

## ✅ What works today

| Area | Status |
| --- | --- |
| Boot | BIOS via `bootloader_api` 0.11 (MBR disk image), `x86_64-unknown-none` |
| CPU | GDT + TSS, IDT, double-fault stack, PIT @ 1 kHz, PS/2 keyboard + mouse |
| Memory | 16 MiB static heap (`linked_list_allocator`) with allocation tracking |
| Scheduling | Custom async executor (`Task`/`Waker`, lock-free ready queue) |
| Graphics | Software compositor, back buffer, damage rectangles, shadows, "glass" dock, glyph cache-free 8×8 font |
| Networking | `smoltcp` + Intel E1000, ICMP echo, HTTP/1.1 client with per-transfer byte budget — **disabled automatically when the bootloader did not map the PCI BAR** (see [docs/http_network_indicator.md](docs/http_network_indicator.md)) |
| Files | In-memory filesystem with a path-safe API (`FsError` instead of panics) |
| Browser | `rustos-web`: URL parsing, HTTP streaming decoder, HTML parser, CSS subset, block/inline layout engine |
| Tooling | `python3 tools/rustos.py …` — no `make`/`bash`/`binutils` required on the host |
| CI | `verify.yml` (hard gate) + `build.yml` (images, releases, AI reporter) |

Deliberately **not** implemented: user mode/ring 3, own page tables/MMIO mapping,
TLS, DNS, JavaScript, images from `<img>`, floats in the renderer. See
[docs/browser_engine_2026.md](docs/browser_engine_2026.md) for why, and for the
roadmap that would remove each limitation.

---

## 🚀 Quick start

```bash
# 1. Toolchain (nightly is required for -Z build-std)
rustup toolchain install nightly --component rust-src llvm-tools-preview
rustup target add x86_64-unknown-none

# 2. Check the machine and build the bootable image
python3 tools/rustos.py doctor
python3 tools/rustos.py build        # -> target/my_rust_os_desktop.img

# 3. Boot it
python3 tools/rustos.py qemu         # headless serial smoke test
#   or: qemu-system-x86_64 -drive format=raw,file=target/my_rust_os_desktop.img
```

Everything the tool does is also plain cargo:

```bash
cargo run --bin builder          # build kernel + MBR image (+ QEMU when not in CI)
cargo test --workspace --exclude kernel
cargo clippy -p rustos-web -p builder --all-targets -- -D warnings
```

### On Android (Termux / Limbo)

```bash
pkg install rust rust-src qemu-system-x86-64-headless python
python3 tools/rustos.py build
# copy target/my_rust_os_desktop.img to /sdcard/Download, then in Limbo:
#   Architecture: x64 · CPU: qemu64 · RAM: 512 MB · HD-A: the .img file
```

Termux ships a stable `rustc`; the builder exports `RUSTC_BOOTSTRAP=1` so the
`-Z build-std` flags used for `x86_64-unknown-none` work there too. Memory
constraints and workarounds are documented in
[docs/build_tooling.md](docs/build_tooling.md).

---

## 📁 Repository layout

```text
kernel/        ring-0 kernel: desktop, drivers, network, scheduler, fs
rustos-web/    browser engine core (no_std, zero dependencies, host-testable)
builder/       host tool: compiles the kernel, links the bootloader, writes the image
tools/         python build/verification tool + its unit tests
docs/          architecture, browser research, CI, performance, tooling
.github/       workflows (verify = hard gate, build = release pipeline)
```

### `kernel/` — the operating system

| File | Purpose |
| --- | --- |
| `src/main.rs` | entry point: GDT/IDT/PIC/PIT, heap, PCI + E1000, boot animation, task spawn, panic handler (red screen **and** serial) |
| `src/desktop/compositor.rs` | window manager + software renderer, damage-rect presentation, **idle frame skipping** |
| `src/desktop/browser.rs` | browser chrome; drives one `PageEngine` and replays its display list |
| `src/desktop/{terminal,notepad,filemanager,taskmanager,settings,updater}.rs` | the pre-installed apps |
| `src/network/mod.rs` | `smoltcp` glue, HTTP client state machine, byte budgets |
| `src/task/` | executor, keyboard/mouse/timer streams, `yield_now` |
| `src/fs/mod.rs` | in-memory filesystem |
| `src/interrupts.rs`, `src/gdt.rs`, `src/allocator.rs`, `src/serial.rs` | low-level plumbing |
| `src/paging.rs` | read-only page-table walk (`is_mapped`), used by drivers to refuse unmapped MMIO |

### `rustos-web/` — the browser engine core

Zero dependencies, `#![forbid(unsafe_code)]`, unit tested on the host (83 tests),
and compiled into the kernel for the bare-metal target:

```text
Url::parse ─► http::request_bytes ─► [ TCP socket ]
                                        │
      http::ResponseParser ◄────────────┘   incremental, chunk-safe, bounded
                  │
                  ▼
            html::Document      forgiving parser, max depth 64 / 20k nodes
                  │
      style::StyleSheet (tag + .class + #id rules)
                  │
                  ▼
      layout::layout_document ─► Layout { Vec<DrawCmd> }
                  │
                  ▼
      compositor draws rects + glyph runs (only the visible ones)
```

Details, supported HTML/CSS subsets and limits: [docs/webcore.md](docs/webcore.md).
Full documentation index: [docs/README.md](docs/README.md).

---

## 🧠 Why the UI does not lag

Three rules, all enforced in code:

1. **Layout once, paint many times.** Parsing, styling and layout run on page
   load or resize; a frame only replays a flat display list.
2. **No idle frames.** `GraphicalCompositor::render_all` returns immediately
   unless a repaint was requested (input, network, animation) or the 2 Hz
   housekeeping tick is due — an idle desktop costs ~0 % CPU instead of a
   full-screen composite 60 times per second.
3. **Damage rectangles.** Only changed regions are copied from the back buffer
   to the framebuffer, and only commands intersecting the viewport are drawn.

Measurements, arithmetic and the tuning knobs: [docs/rendering_performance.md](docs/rendering_performance.md).

---

## 🧪 Testing & CI

```bash
python3 tools/rustos.py verify        # fmt + clippy + rust tests + python tests + docs
python3 tools/rustos.py verify --full # additionally lints the kernel target
```

* `rustos-web`: 83 unit tests (URL, HTTP framing, HTML repair, CSS cascade,
  layout, hostile input, command budget).
* `builder`: CPUID feature detection tests.
* `tools/tests`: 44 tests for the build tool (manifest parsing, image validation,
  serial-log scanning, documentation gate).
* Kernel: boot-tested in QEMU on every push — twice: without network hardware and
  with an explicit e1000 NIC. The serial log must contain the `[boot]` banner and
  no panic markers.

Pipeline documentation, required checks and the read-only AI failure reporter:
[docs/ci.md](docs/ci.md).

---

## 🛡️ Stability & memory rules

* Kernel heap is a static **16 MiB** array; every subsystem has an explicit cap
  (page body 1.5 MiB, HTTP response 4 MiB, 20k layout commands, 20k DOM nodes,
  64 nesting levels).
* No `unwrap()` on user input, no unchecked slicing in the parser, no recursion
  while parsing markup.
* `panic = "abort"` everywhere; the panic handler paints a red screen **and**
  writes to COM1 so CI can see it.
* Build profiles are tuned for a small, fast binary: `opt-level = 3`,
  `lto = "fat"`, `codegen-units = 1`, `strip = "debuginfo"`.

Security policy and threat model: [SECURITY.md](SECURITY.md).

---

## 📜 License

GNU General Public License v3.0 — see [LICENSE](LICENSE).
