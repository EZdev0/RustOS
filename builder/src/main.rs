use std::path::Path;
use std::process::Command;

fn main() {
    println!("================================================================");
    println!("   🦀 RUST OS WORKSPACE BUILDER (BOOTLOADER v0.11 PROTOKOL)  ");
    println!("================================================================");

    // 1. Kernel im no_std Bare-Metal Target kompilieren
    println!("[1/3] Rufe Cross-Compiler für x86_64 Kernel auf...");
    let status = Command::new("cargo")
        .env("RUSTC_BOOTSTRAP", "1")
        .args(&[
            "build",
            "-Z",
            "build-std=core,compiler_builtins",
            "-Z",
            "build-std-features=compiler-builtins-mem",
            "--manifest-path",
            "kernel/Cargo.toml",
            "--target",
            "x86_64-unknown-none",
            "--release"
        ])
        .status()
        .expect("Fehler: Cargo-Build-Prozess konnte nicht gestartet werden.");

    if !status.success() {
        eprintln!("❌ Fehler: Die Kernel-Kompilierung ist fehlgeschlagen.");
        std::process::exit(1);
    }

    // 2. Pfade definieren
    let kernel_elf = Path::new("target/x86_64-unknown-none/release/kernel");
    let output_disk = Path::new("target/my_rust_os_desktop.img");

    // 3. Verwende das offizielle Bootloader-Crate, um ein startbares BIOS-Image zu verknüpfen
    // BiosBoot wird gewählt, da es nativ auf JEDEM QEMU läuft, ohne UEFI-Firmware-Pfade konfigurieren zu müssen!
    println!("[2/3] Konvertiere Kernel-ELF in ein bootfähiges MBR-Festplattenimage...");
    let mut bios_boot = bootloader::BiosBoot::new(kernel_elf);
    bios_boot.create_disk_image(output_disk)
        .expect("Fehler: Das Erstellen des bootfähigen Festplattenimages ist fehlgeschlagen.");

    println!("✅ Image erfolgreich generiert: {}", output_disk.display());

    // 4. Automatische Ausführung im Emulator
    println!("[3/3] Starte QEMU Emulator mit angehängter virtueller Festplatte...");
    let qemu_result = Command::new("qemu-system-x86_64")
        .args(&[
            "-drive",
            &format!("format=raw,file={}", output_disk.display())
        ])
        .status();

    match qemu_result {
        Ok(s) if s.success() => println!("✅ QEMU wurde sauber beendet."),
        _ => {
            println!("\n💡 HINWEIS: Falls QEMU nicht öffnet, vergewissere dich, dass 'qemu-system-x86_64'");
            println!("   installiert und zu den System-Umgebungsvariablen (PATH) hinzugefügt wurde.");
        }
    }
}
