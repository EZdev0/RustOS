# Rendering performance: how RustOS keeps a software framebuffer smooth

The compositor is a pure software rasteriser: no GPU, no SIMD in the kernel
(`x86_64-unknown-none` disables SSE/AVX register use here), one core, a
back buffer in the kernel heap. Everything below is about doing *less work per
frame*, not about doing the same work faster.

All numbers are arithmetic for an 800×600 framebuffer (`bytes_per_pixel = 3`,
i.e. 1 440 000 bytes per full-screen blit), not benchmark results. They are here
to justify the thresholds that are hard-coded in the code.

---

## The problem before the change

`task::timer::timer_task` renders every 16 ticks (1 kHz PIT → ~60 Hz), and
`render_all()` did all of this unconditionally:

1. repaint the desktop gradient over every dirty rectangle
   (`width × height` = 480 000 pixel writes),
2. draw shadows/glow for every icon and window (per-pixel `isqrt` distance
   fields),
3. draw the "glass dock" with a per-pixel blur of the 440×60 buffer,
4. call `update()` + `draw()` on every app,
5. copy dirty rectangles (or the whole 1.44 MB buffer) into the framebuffer.

On an idle desktop that is ~60 full-screen gradient fills and ~60 dock blurs per
second, i.e. tens of millions of pixel writes per second — with no visible change
at all. On a phone running Limbo (no nested paging acceleration) that shows up as
heat and fan noise on a laptop.

## What changed

### 1. Repaint requests instead of unconditional frames

`kernel/src/desktop/mod.rs`:

```rust
static REPAINT_REQUESTED: AtomicBool = AtomicBool::new(true);
pub fn request_repaint();          // called by input, network, window creation
pub fn take_repaint_request() -> bool;
```

`render_all()` now starts with:

```rust
self.ticks = self.ticks.wrapping_add(1);
let housekeeping = self.ticks % 30 == 0;                  // 2 Hz
let requested    = crate::desktop::take_repaint_request();
if !(requested || housekeeping || theme_change || self.full_redraw || self.animations_active()) {
    return;                                               // nothing to do
}
```

Repaint requests are issued by:

| Source | Where |
| --- | --- |
| keyboard input | `dispatch_keyboard_event`, `dispatch_keycode_event` |
| mouse movement/clicks | `handle_mouse_event` |
| window creation | `add_window` |
| network progress | `network::network_task` when new bytes were received |
| theme switch | `THEME_CHANGE_REQUESTED` (checked in the guard) |
| animations | `animations_active()` (window open animation, dock slide, drag) |

`animations_active()` keeps the 60 Hz path while a transition is running, so the
opening animation and the dock bounce are unchanged.

### 2. A 2 Hz housekeeping tick

Clocks, the uptime counter, the CPU/RAM charts and the network indicator still
need to be truthful on a completely idle desktop. `ticks % 30 == 0` forces a
frame twice per second — cheap (2 full composites per second instead of 60) and
enough for a wall clock that only shows `HH:MM`.

### 3. Warp-speed scanline fills

`draw_hline` (the primitive behind gradient fills, window bodies, the dock and
every rectangle) used to call `self.backbuffer.get_mut(offset..offset + 3)` per
pixel. It now resolves **one** mutable slice for the whole run and writes it in
`bytes_per_pixel` chunks:

```rust
let row = self.backbuffer.get_mut(offset..offset + len)?;
for pixel in row.chunks_exact_mut(bpp) {
    pixel.get_mut(0..3)?.copy_from_slice(&[r, g, b]);
}
```

One bounds check per scanline instead of 480 000 per frame, plus a
`copy_from_slice` that LLVM turns into wider stores. The offset arithmetic is
`checked_mul`/`checked_add`, so a bogus window geometry cannot wrap into another
buffer.

### 4. Browser painting is O(visible), not O(document)

`Layout::cmds_in_view(scroll, height)` filters by vertical intersection, so a
2 000-line page paints exactly the ~40 lines on screen. Layout itself runs only
on navigation or resize (`PageEngine::layout` caches by width + theme).

---

## Expected effect (arithmetic)

| Scenario | Before | After |
| --- | --- | --- |
| Idle desktop, 60 s | 3 600 full composites + 3 600 dock blurs | 120 housekeeping composites |
| Mouse moving, page open | 60 fps of full-screen work | 60 fps, but only damaged rects are copied |
| Page loading | 60 fps of full-screen work | frames only when bytes arrive |
| Scrolling | 60 fps full composite | 60 fps, O(visible commands) + dirty rect |

The dock blur alone is 440 × 60 × 60 = 1.58 M pixel reads per second at 60 fps;
that cost now only exists while something actually changes.

## How to verify on your own machine

```bash
python3 tools/rustos.py build
python3 tools/rustos.py qemu --seconds 20      # serial log in target/qemu_serial.log
```

Then watch the host CPU usage of the QEMU process with nothing happening
(should hover near idle after the boot animation, with a 2 Hz blip), then move
the mouse (60 Hz). If you want a kernel-side counter, `GraphicalCompositor.ticks`
already counts frames; a follow-up task could expose "frames skipped" in the Task
Manager app (tracked in the roadmap in
[browser_engine_2026.md](browser_engine_2026.md)).

## Trade-offs and known limitations

* Apps that animate *themselves* (e.g. the Task Manager charts) refresh at 2 Hz
  while the desktop is idle. They call `request_repaint()` from `update()` if they
  need a faster cadence — a one-line change per app.
* Damage rectangles are conservative (whole window + 20 px shadow margin). Finer
  granularity would need per-widget invalidation.
* The desktop gradient is recomputed for the damaged area instead of being cached
  in a texture; caching 1.44 MB would triple the heap usage of the compositor for
  a benefit that only matters when the whole screen is invalidated.
