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
- **`src/main.rs`**: The main entry point. Sets up the IDT, enables interrupts, initializes the graphical compositor, and enters a smart Event Loop that dynamically updates the Terminal on keystrokes.
- **`src/interrupts.rs`**: Manages Hardware Interrupts! Implements the x86_64 Interrupt Descriptor Table (IDT), configures the 8259 Programmable Interrupt Controller (PIC), and safely parses raw PS/2 Keyboard scancodes.
- **`src/desktop/compositor.rs`**: A custom software-rendering engine! It features:
  - Linear Framebuffer abstraction for drawing pixels (`draw_pixel`, `draw_rect`).
  - Font rendering utilizing the `font8x8` crate to draw text onto the GUI (`draw_char`, `draw_terminal_text`).
  - A beautiful Anthracite-Blue desktop, an interactive Dock, and a macOS-style graphical window.
- **`src/desktop/terminal.rs`**: A thread-safe global `TEXT_BUFFER` (`heapless::String`) that seamlessly links the Keyboard Interrupts (ISR) with the Graphical Compositor.
- **`Cargo.toml`**: Includes crates like `x86_64`, `pic8259`, `pc-keyboard`, `font8x8`, and `spin`.

### 2. The Builder (`builder/`)
A custom Rust host application that acts as a wrapper around the build system.
- Compiles the kernel for the `x86_64-unknown-none` target using Cargo.
- Takes the compiled ELF binary and converts it into a bootable MBR Disk Image using `bootloader::BiosBoot::new()`.
- Automatically invokes `qemu-system-x86_64` with the generated image.

## 🤖 Smart CI/CD Pipeline (AI Integrated)

The repository features an ultra-modern GitHub Actions workflow inspired by systems like *JulesOS* and *WebEngine2.0*. 
- **Auto-Compilation & Release**: Automatically builds the `.img` and creates GitHub Releases.
- **Local AI Bug Reporter (Ollama)**: If the kernel compilation fails, the pipeline automatically installs Ollama, pulls the `qwen2.5-coder:3b` LLM directly onto the GitHub Runner, feeds it the compiler logs, and automatically opens a GitHub Issue with an AI-generated fix!

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
