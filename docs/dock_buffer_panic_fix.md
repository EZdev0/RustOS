# Compositor Dock Buffer Panic Fix

## Overview
A critical system crash (Red Screen of Death) was identified in the `kernel/src/desktop/compositor.rs` during the execution of `render_glass_dock()`.
The kernel panicked with `index out of bounds: the len is 79200 but the index is 79200`.

## Root Cause
The `dock_buffer` inside the `GraphicalCompositor` struct was statically allocated with `alloc::vec![0; 440 * 60 * 3]`, which totals `79200` elements.
However, the width of the glass dock (`dock_w`) is calculated dynamically based on the number of available applications (`apps_to_draw`):
```rust
let max_apps_possible = width.saturating_sub(40) / 85;
let apps_to_draw = self.apps.len().min(max_apps_possible);
let dock_w = apps_to_draw * 85 + 15;
```
If the number of applications exceeds the space of 440 pixels (around 5 apps), `dock_w` evaluates to a width larger than 440, requiring a larger buffer. The index calculation `(dy * dock_w + dx) * 3` subsequently evaluated to an index `>= 79200`, causing the out of bounds panic.

## Resolution
The resolution ensures that the `dock_buffer` dynamically scales to accommodate the required length of `dock_w * dock_h * 3`.
The following logic was injected before the rendering loop in `render_glass_dock()`:
```rust
let required_len = dock_w * dock_h * 3;
if self.dock_buffer.len() < required_len {
    self.dock_buffer.resize(required_len, 0);
}
```
This guarantees that the heap-allocated buffer always fits the pixels for the dynamically expanding taskbar dock, fully eliminating the panic without causing memory starvation, since it reuses the vector capacity.
