# RustOS To-Do

Living list. Order is "next first"; every item names the file that would change.

## Done on this branch (for reference)

- [x] e1000 family instead of one hardcoded Intel PCI ID
      (`kernel/src/hardware/e1000.rs`)
- [x] Realtek RTL8139 driver + generic PCI probe
      (`kernel/src/hardware/{net,rtl8139}.rs`)
- [x] DHCP client with static fallback (`kernel/src/network/mod.rs`)
- [x] DNS resolver + "Resolving …" state in the browser
      (`kernel/src/network/mod.rs`, `kernel/src/desktop/browser.rs`)
- [x] Browser layout bugs: hard word breaks, real table grid, trailing-slash
      URLs, IPv6 `Host:`, redirect handling (`rustos-web/src/*`)
- [x] 35 public-API contract tests + Python render probe
      (`rustos-web/tests/contract.rs`, `tools/webprobe.py`)
- [x] Boot tests for `-net e1000`, `-net rtl8139` and `-net none`
      (`.github/workflows/verify.yml`)

## P0 - network

- [ ] `virtio-net` driver — the fast path on QEMU/KVM and the only NIC in many
      cloud VMs (`kernel/src/hardware/virtio_net.rs`)
- [ ] `pcnet` and `ne2k_pci` drivers — two more Limbo/VirtualBox options
      (`kernel/src/hardware/`)
- [ ] TLS 1.3 client for HTTPS (currently `https://` is rejected by the URL
      parser); start with ECDHE-X25519 + AES-GCM + SHA-256, no certificate
      validation beyond pinning a small root set (`kernel/src/network/tls.rs`)
- [ ] IPv6: the stack is IPv4-only, though smoltcp supports v6
- [ ] Wi-Fi guard: detect wireless class-`0x0D` devices and say so in the log
      instead of a bare "unsupported NIC"

## P1 - browser

- [ ] Link clicking on the rendered page (hit-test the display list, then
      `BrowserApp::open`) (`kernel/src/desktop/browser.rs`)
- [ ] Image decoding: PNG (zlib + filters) and baseline JPEG, drawn as
      `DrawCmd::Image` (`rustos-web/src/layout.rs`, `kernel/src/desktop/`)
- [ ] Incremental layout: re-layout only the dirty subtree when a page streams
      in, instead of once after `Complete` (`rustos-web/src/layout.rs`)
- [ ] Favicon + real document title in the tab bar
- [ ] `POST` forms and a text input widget
- [ ] Scrolling with the keyboard/mouse wheel inside a link-dense page
      (coalescing is in place, verify on hardware)

## P2 - tooling / CI

- [ ] Offline `cargo` mirror in the sandbox so the kernel can be compiled
      locally without the network (CI is currently the only kernel compiler)
- [ ] Cache the compiled render probe between CI runs (`tools/webprobe.py`)
- [ ] Perf budget: assert `layout()` time for a 100 KiB page stays under N ms in
      the probe (needs a `std::time` shim in `render_dump`)
- [ ] Publish an ISO asset next to the raw image (Limbo, VirtualBox, VMware all
      accept ISOs directly). **Note:** the release history only ever contained
      `my_rust_os_desktop.img`, `rustos-analysis-report.txt` and
      `update_manifest.json` — there never was a "universal" asset; see
      [ci.md](ci.md) for what the pipeline uploads.
- [ ] UEFI boot: `bootloader` 0.11 produces a BIOS image only
      (`Cargo.toml`, `kernel/src/main.rs`)

## P3 - polish

- [ ] Localised UI strings (German/English switch)
- [ ] `docs/limbo.md` screenshots
- [ ] Save browser bookmarks to the ramdisk filesystem
