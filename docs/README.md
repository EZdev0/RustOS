# RustOS documentation index

Start here, then follow the area you care about.

## Architecture & subsystems

| Document | What it explains |
| --- | --- |
| [webcore.md](webcore.md) | `rustos-web`: URL/HTTP/HTML/CSS/layout modules, supported subsets, limits, testing |
| [rendering_performance.md](rendering_performance.md) | why the compositor no longer lags: repaint requests, 2 Hz housekeeping, scanline fills |
| [http_network_indicator.md](http_network_indicator.md) | the HTTP client state machine, byte budgets, status indicator |
| [network_hardware.md](network_hardware.md) | supported NICs (Intel 8254x, Realtek 8139), PCI probing, log contract, adding a driver |
| [taskmanager_performance_monitor.md](taskmanager_performance_monitor.md) | Task Manager charts, uptime, refresh cadence |
| [dock_buffer_panic_fix.md](dock_buffer_panic_fix.md) | post-mortem: dock buffer overflow (red screen of death) |

## Research & decisions

| Document | What it explains |
| --- | --- |
| [browser_engine_2026.md](browser_engine_2026.md) | 2026 survey of Rust browser engines, what a `no_std` OS can reuse, our design choices and roadmap |
| [TODO.md](TODO.md) | the living to-do list: every open task with the file that would change |

## Process & infrastructure

| Document | What it explains |
| --- | --- |
| [ci.md](ci.md) | workflows, required checks, artifacts, the read-only AI failure reporter |
| [build_tooling.md](build_tooling.md) | `tools/rustos.py` reference, documentation gate, Termux/memory notes |
| [limbo.md](limbo.md) | running the image in Limbo on Android: machine settings, NIC choice, troubleshooting |
| [../SECURITY.md](../SECURITY.md) | threat model and reporting process |
| [../.jules/sentinel.md](../.jules/sentinel.md) | running log of found bugs and their prevention rules |

## Conventions

* Every document is linked from this index (enforced by
  `python3 tools/rustos.py docs`).
* Documents describe the code as it is. Claims that the documentation gate can
  verify (profile values, referenced files, links) are checked automatically.
* Post-mortems (`*_fix.md`) keep the original analysis and add a resolution
  section — never rewrite history.
