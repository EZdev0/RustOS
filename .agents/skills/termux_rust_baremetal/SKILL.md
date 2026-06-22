---
name: termux_rust_baremetal
description: Cheat-Sheet und Workarounds, um Bare-Metal Rust Kernel und OS-Projekte auf Android in Termux zu kompilieren.
---

# Termux Rust Bare-Metal OS Development

Beim Kompilieren von Rust-OS Projekten (wie `no_std` Kerneln oder Bootloadern) in Termux müssen zwingend folgende Workarounds angewandt werden:

1. **RUSTC_BOOTSTRAP=1**
Termux liefert standardmäßig Stable-Rust. Bare-Metal Projekte benötigen Nightly-Features (z.B. `-Z build-std`). Setze zwingend `RUSTC_BOOTSTRAP=1` als Env-Var, um diese Features auf Stable freizuschalten.

2. **LLVM-Tools Sysroot Symlink**
Der `bootloader` Crate sucht nach `llvm-tools-preview`. Da Termux dies nicht über `rustup` anbietet, verlinke die nativen System-Tools:
```bash
SYSROOT=$(rustc --print sysroot)
TARGET=$(rustc -vV | grep host | cut -d' ' -f2)
mkdir -p "$SYSROOT/lib/rustlib/$TARGET/bin"
ln -sf /data/data/com.termux/files/usr/bin/llvm-objcopy "$SYSROOT/lib/rustlib/$TARGET/bin/llvm-objcopy"
```

3. **Memory Limits**
Termux crasht oft bei zu hoher CPU/RAM-Auslastung durch Rust.
Erstelle IMMER `.cargo/config.toml`:
```toml
[build]
jobs = 4

[profile.release]
codegen-units = 8
```

4. **build-std**
Nutze für das `no_std` Target immer: `cargo build -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem`
