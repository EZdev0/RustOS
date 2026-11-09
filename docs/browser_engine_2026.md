# Building a browser in Rust that renders real pages without lag (2026 research)

This document summarises what the 2026 ecosystem offers for "write your own
browser in Rust", what a bare-metal OS can realistically reuse, and why RustOS
took the path it did. It is the design record behind `rustos-web/`.

> Scope note: everything below is either engineering arithmetic on this code base
> or a third-party claim from the linked sources (marked as such). Nothing here is
> a benchmark of RustOS itself.

---

## 1. What exists in 2026

| Project | Stack | Reusable in a `no_std` kernel? |
| --- | --- | --- |
| [Blitz](https://lib.rs/crates/blitz) (Dioxus Labs) | `stylo` (CSS), `html5ever`, `taffy`, `parley`, `vello`/`vello_cpu`, `wgpu` | No — needs `std`, threads, font files and a GPU/CPU rasteriser |
| [Servo](https://servo.org/) | full browser engine, `webrender`, SpiderMonkey | No |
| [Falco](https://github.com/poxk/Falco) | from-scratch HTML/CSS/JS/SVG renderer, PNG output | No — but its architecture notes are useful |
| [h5i](https://medium.com/@Koukyosyumei/building-a-pure-rust-sandboxed-local-first-browser-for-ai-agents-h5i-browser-light-ce4c13f16e5a) | Blitz + Stylo + Boa + Parley + `vello_cpu` | No |
| [BrowserOxide](https://github.com/yfedoseev/browser_oxide) | own CSS engine, deno_core V8, tiny-skia | No |
| Ilmenite | html5ever + Boa, "fast path" for static pages | No |

Two observations matter for an operating system:

1. **Nobody ships a `no_std` layout engine.** Even the "lightweight" Rust
   renderers assume `std`, an allocator that can grow, font files (TrueType),
   and often threads or a rasteriser like `vello_cpu`. Blitz's own slide deck
   lists `stylo`, `html5ever`, `taffy`, `parley` and `wgpu` as its foundation —
   that is around 100 MB of dependencies and a lot of `std`-only code.
2. **The interesting performance ideas are architectural, not crates.** Falco
   advertises exactly what RustOS needs: *"the window only re-paints when state
   actually changes (scroll, input text, focus, hover). On idle frames only the
   overlays are redrawn — the page canvas is cached."* That is the same trick
   used here (see [rendering_performance.md](rendering_performance.md)).

### What 2026 tooling says about "real websites"

* **JavaScript is the line in the sand.** Every project that claims broad
  compatibility (BrowserOxide, h5i, Ilmenite) embeds a JS engine (V8 via
  `deno_core`, or Boa). A JS engine needs `std`, JIT memory (`W^X` mapping) and
  megabytes of heap — impossible on a 16 MiB static heap without paging.
* **Static HTML + CSS is the useful 90 %.** Ilmenite reports that a pure-Rust
  static fast path handles the majority of pages they see, falling back to
  Chromium for the rest. For an OS demo/hacking target, a static renderer is the
  right first milestone.
* **Text shaping is the expensive part.** Real engines use `parley`/`rustybuzz`
  with system fonts. RustOS has a single 8×8 bitmap font `font8x8` (no files, no
  shaping). That makes metrics exactly predictable — every glyph is 8×8 px at
  scale 1 — which removes an entire class of layout bugs and allocations.

---

## 2. The design that RustOS uses

```text
    bytes from TCP (arbitrary boundaries)
              │
   ┌──────────▼──────────┐   incremental, bounded, chunk-safe
   │ http::ResponseParser│   Content-Length / chunked / until-close
   └──────────┬──────────┘
              │ body bytes
   ┌──────────▼──────────┐   forgiving, iterative (explicit stack),
   │  html::Document    │   max depth 64, max 20 000 nodes
   └──────────┬──────────┘
              │ tree + <style> blocks
   ┌──────────▼──────────┐   tag / .class / #id, specificity ordered
   │ style + stylesheet │   integer-only, no floats
   └──────────┬──────────┘
              │ computed styles
   ┌──────────▼──────────┐   one pass, block/inline, word wrap
   │ layout::Layout     │   → flat Vec<DrawCmd> (rects + glyph runs)
   └──────────┬──────────┘
              │ cached display list
   ┌──────────▼──────────┐   only visible commands, damage rects
   │   compositor       │   back buffer → framebuffer
   └────────────────────┘
```

### Why a flat display list instead of a retained box tree

* Painting a frame is a linear walk over commands that intersect the viewport —
  no tree traversal, no style resolution, no allocation while scrolling.
* Layout is *not* incremental yet (see roadmap), but it runs only on navigation or
  resize, which is exactly when a user expects a brief pause.
* The kernel side of the browser is ~400 lines (`kernel/src/desktop/browser.rs`)
  and contains no parsing logic at all — the risky code is host-testable.

### Why the engine is `no_std` + zero dependencies

* `-Z build-std=core,alloc,compiler_builtins` builds `core`/`alloc` from source
  for `x86_64-unknown-none`; anything that needs `std` cannot participate.
* Zero dependencies means: no crates.io resolution in CI for this crate, no
  unexpected `std` activation, and `cargo test -p rustos-web` runs on any host.
* `#![forbid(unsafe_code)]` — the engine cannot corrupt the ring-0 state, and
  the kernel's `unsafe` surface stays where it belongs (framebuffer, ports).

### Deliberate limitations (and why)

| Limitation | Reason | Roadmap |
| --- | --- | --- |
| Network needs mapped MMIO | the kernel does not build page tables, so PCI BARs are unmapped and the NIC is skipped | kernel page tables + MMIO mapping + DMA address translation |
| `http://` only, no TLS | TLS needs a crypto stack + RNG + certificates (megabytes) | TLS 1.3 with `embedded-tls` + RDRAND, after paging |
| No DNS | `smoltcp` supports a DNS socket; needs async wiring + a resolver cache | `socket-dns` + hosts table |
| No JavaScript | needs a JS interpreter + heap/paging | optional Boa in ring 3 |
| No image decoding | PNG/JPEG decoders are large; `<img>` renders a placeholder + alt text | tiny PNG (DEFLATE) first |
| Tables stack cells vertically | avoids a full table layout algorithm | taffy-style table pass |
| No floats | soft-float in ring 0 is slow and bloats the binary | fixed-point later |
| ASCII bitmap font only | one 8×8 font, no shaping | bitmap font with more glyphs |

---

## 3. What a "does not lag" browser needs (checklist)

1. **No work per idle frame** — RustOS: `render_all` early-returns, 2 Hz
   housekeeping tick only.
2. **No allocation on the paint path** — RustOS: display list is built once;
   painting borrows it.
3. **Damage rectangles, not full-screen blits** — RustOS: dirty rect copy.
4. **Exact text metrics** — RustOS: monospace 8 px cells, `chars × scale × 8`.
5. **Bounded, incremental network decoding** — RustOS: `ResponseParser` accepts
   arbitrary TCP slices and never buffers more than the page budget.
6. **Predictable failure** — RustOS: byte budgets abort the transfer instead of
   exhausting the 16 MiB heap.
7. **No blocking in the UI task** — RustOS: fetch progress happens chunk by
   chunk in the executor; the network task requests a repaint when bytes land.

All seven are implemented today and covered by host tests where possible
(`rustos-web/src/http.rs`, `.../layout.rs`, `kernel/src/desktop/compositor.rs`).

---

## 4. Reading list

* Blitz — a modular HTML/CSS renderer built on Stylo/Taffy/Parley/Vello:
  <https://lib.rs/crates/blitz>, architecture slides
  <https://webengineshackfest.org/2024/slides/blitz_a_truly_modular_hackable_web_renderer_by_nico_burns.pdf>
* Falco — a from-scratch engine with an explicit "incremental repaint" note:
  <https://github.com/poxk/Falco>
* h5i — pure-Rust browser for agents (Blitz/Stylo/Boa/Parley/vello_cpu), memory
  and WPT numbers: <https://medium.com/@Koukyosyumei/building-a-pure-rust-sandboxed-local-first-browser-for-ai-agents-h5i-browser-light-ce4c13f16e5a>
* BrowserOxide — own CSS engine + V8 via `deno_core`:
  <https://github.com/yfedoseev/browser_oxide>
* Ilmenite engineering notes on a Rust "fast path" for static pages:
  <https://ilmenite.dev/blog/why-rust-for-browser-engine>
