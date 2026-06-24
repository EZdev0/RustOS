# Task Manager Performance Monitor

## Overview
The `TaskManagerApp` in `kernel/src/desktop/taskmanager.rs` has been refactored to provide a dynamic, real-time performance monitoring dashboard. This ensures that the UI updates continuously and smoothly reflects the system state.

## Key Changes
1. **Update Frequency:** The `update()` tick interval has been decreased from every 100 ticks to every 20 ticks. This provides a more responsive and less flickering update to the CPU and RAM history charts.
2. **CPU and RAM Simulation:** The mock performance values have been tuned using prime and semi-prime modulos (e.g. `% 83` for CPU and `% 20` for RAM) combined with the tick counter to generate pseudo-random fluctuations that mimic a real system under variable load.
3. **Uptime Tracking:** The `OsAnalysis` section now displays a live Uptime counter, derived from the `ticks` divided by 60 (assuming a 60FPS continuous loop).
4. **Network and CPU Ticks:** The `DeviceInfo` section now correctly reports live `CPU Ticks` and pseudo-random live `Network` bandwidth usage in kbps.
5. **Memory Optimization:** Redundant `format!()` macros for static string slices have been replaced with `alloc::string::String::from()` to avoid unnecessary formatting overhead and silence `clippy` warnings.

## Future Work
- Connect these visual components to actual hardware performance counters and network interfaces once implemented in the kernel architecture.
- Optimize the `draw()` methods to cache strings instead of allocating them dynamically on every frame to prevent memory fragmentation and GC/allocation overhead in the long run.
