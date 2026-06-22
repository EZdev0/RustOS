# 🦀 RustOS (Limbo & QEMU Compatible)

A highly robust, lightweight, bare-metal operating system written entirely in Rust. Designed to be fast, memory-efficient, and compatible with various emulators like **Limbo PC Emulator (Android)** and **QEMU**.

## 🚀 Architecture & Compatibility

This OS kernel is designed to be **architecture-agnostic** in its core logic (`no_std`, `no_main`), but currently utilizes the `bootloader` crate for seamless `x86_64` (64-bit PC) booting. 

- **Primary Target**: `x86_64-unknown-none`
- **Boot Protocol**: `bootloader_api` (v0.11.x) - Supports both legacy BIOS (MBR) and modern UEFI booting natively!
- **Limbo PC Emulator**: The generated `.img` file is 100% compatible with Limbo PC Emulator (x86_64 architecture setting). Just mount the `target/my_rust_os_desktop.img` as a Hard Disk (IDE/SATA) in Limbo and boot!
- **Cross-Architecture Scalability**: Because the GUI logic (in `kernel/src/desktop`) interacts strictly with a generic linear `Framebuffer`, the kernel can easily be extended to support `aarch64` (ARM) in the future by simply swapping the bootloader layer (e.g., to u-boot or direct aarch64 UEFI).

## 📁 Deep Analysis of the Project Structure

### 1. The Kernel (`kernel/`)
The heart of the OS. Runs entirely in ring-0 (supervisor mode) without any underlying operating system.
- **`src/main.rs`**: The main entry point. Retrieves the framebuffer from the bootloader and passes it to the graphical compositor. Then it halts the CPU (`core::hint::spin_loop()`) to save power.
- **`src/desktop/compositor.rs`**: A custom software-rendering engine! It features:
  - Linear Framebuffer abstraction for drawing pixels (`draw_pixel`, `draw_rect`).
  - Automatic pixel format matching (RGB, BGR).
  - A rendering loop that draws a beautiful Anthracite-Blue background, an interactive-looking Dock, a top Menu Bar, and a macOS-style graphical window mimicking a Kernel Terminal.
  - A precise hardware-styled mouse cursor.
- **`Cargo.toml`**: Specifies `bootloader_api` for the standardized boot protocol and `spin` for lock-free synchronisation.

### 2. The Builder (`builder/`)
A custom Rust host application that acts as a wrapper around the build system.
- Compiles the kernel for the `x86_64-unknown-none` target using Cargo.
- Takes the compiled ELF binary and converts it into a bootable MBR Disk Image using `bootloader::BiosBoot::new()`.
- Automatically invokes `qemu-system-x86_64` with the generated image.

## 🛡️ Stability, Multi-Threading & RAM Optimizations

Building operating systems from source is extremely memory-intensive (often crashing mobile environments like Termux due to RAM limits). To guarantee stability and prevent Out-Of-Memory (OOM) crashes on 12GB/8GB devices:

1. **Strict Thread Capping (`.cargo/config.toml`)**:
   We restricted Cargo to `jobs = 4`. This ensures multi-threading is used for speed, but the parallel spawned `rustc` processes won't consume more than 4-6 GB of RAM collectively.
2. **Codegen-Unit Tuning**:
   We set `codegen-units = 8` for release builds. This dramatically reduces the compiler's memory footprint during the optimization and LLVM generation phases.
3. **Compiler Trick (`RUSTC_BOOTSTRAP=1`)**:
   The `Makefile` dynamically injects `RUSTC_BOOTSTRAP=1` to allow building the required Nightly Rust features (`-Z build-std`) using the stable compiler provided by Termux! This guarantees zero crashes and high compatibility across toolchains.

## 🛠️ How to Build and Run

### Prerequisites
Make sure you have Rust and an Emulator installed:
```bash
# On Termux
pkg install rust rust-src qemu-system-x86-64-headless make
```

### Build & Run (QEMU)
```bash
make run
```
This will safely compile the OS across multiple threads, link the bootloader, and boot it headlessly or in an X11 window.

### Run on Limbo PC Emulator (Android)
1. Run the build step above.
2. Copy `target/my_rust_os_desktop.img` to your Android internal storage (e.g., `/sdcard/Download/`).
3. Open Limbo PC Emulator.
4. Create a new Machine (Architecture: `x64`, CPU Model: `qemu64`, RAM: `512MB`).
5. Under Hard Disk A, select the copied `.img` file.
6. Press ▶️ Play!

## 📜 License
Private & Proprietary.
