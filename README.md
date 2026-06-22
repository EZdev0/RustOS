# 🦀 RustOS Workspace

Ein minimalistisches, in Rust geschriebenes Bare-Metal-Betriebssystem. Dieses Projekt verwendet das moderne `bootloader_api` (v0.11.7) und verfügt über einen grafischen Compositor, der einen Desktop, eine Taskleiste (Dock) und ein simuliertes Fenster rendert.

## 📁 Projektstruktur

Das Projekt ist als Cargo-Workspace strukturiert und besteht aus zwei Hauptkomponenten:

- **`kernel/`**: Der eigentliche Betriebssystemkern. Er ist `#![no_std]` und `#![no_main]`. Er nutzt das Framebuffer-Feature des Bootloaders, um Pixel auf den Bildschirm zu zeichnen (siehe `kernel/src/desktop/compositor.rs`).
- **`builder/`**: Ein kleines Host-Programm, das den Build-Prozess automatisiert. Es kompiliert den Kernel für die Zielarchitektur `x86_64-unknown-none`, verknüpft ihn mit dem Bootloader zu einem startfähigen MBR-Festplattenimage (`.img`) und startet dieses anschließend im QEMU-Emulator.

## 🛠️ Voraussetzungen & Installation (Termux)

Da dieses Projekt auf Termux (Android) ausgeführt werden soll, müssen folgende Werkzeuge installiert sein:

1. **Rust & Cargo**:
   Installiere Rust über das Termux-Paketmanagement (falls `rustup` nicht verwendet wird):
   ```bash
   pkg install rust
   ```
2. **QEMU Emulator**:
   Um das generierte Image auszuführen, wird der x86_64-Emulator benötigt:
   ```bash
   pkg install qemu-system-x86_64-headless
   ```
3. **Cross-Compilation Target**:
   Das Target `x86_64-unknown-none` muss für den Rust-Compiler verfügbar sein. Falls `rustup` genutzt wird:
   ```bash
   rustup target add x86_64-unknown-none
   ```
   *(Hinweis: Wenn das offizielle `rust`-Paket von Termux verwendet wird, prüfe, ob es das Target unterstützt oder ob zusätzliche Pakete bzw. ein benutzerdefinierter Build benötigt werden).*

## 🚀 Kompilieren und Ausführen

Um das Betriebssystem zu bauen und direkt im Emulator zu starten, nutze das mitgelieferte Makefile:

```bash
make run
```
Dies führt intern `cargo run --bin builder` aus.

## ⚡ Performance & RAM-Optimierung

Das Kompilieren von Rust-Projekten ist extrem RAM-intensiv. Da dieses Projekt auf einem System mit ca. 12 GB RAM (davon ca. 4-8 GB effektiv frei) läuft, wurde die Konfiguration in `.cargo/config.toml` speziell angepasst:

- **Parallelisierung (`jobs = 4`)**: Die maximale Anzahl gleichzeitiger Threads wurde auf 4 begrenzt. Das sorgt für einen schnellen Multithread-Build, ohne die kritische 8-GB-RAM-Marke zu überschreiten (verhindert Out-Of-Memory-Abstürze).
- **Codegen-Units**: In den Profilen (`dev` und `release`) wurde die Anzahl der Codegen-Units reduziert (`16` bzw. `8`), um den Speicherbedarf des Compilers pro Thread weiter zu senken.

## 📄 Lizenz
Privates Projekt.
