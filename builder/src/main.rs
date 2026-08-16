use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("================================================================");
    println!("   🦀 VIBECORE OS BUILDER (BIOS PROTOKOL)  ");
    println!("================================================================");

    // 1. Kernel im no_std Bare-Metal Target kompilieren
    println!("[1/3] Rufe Cross-Compiler für x86_64 Kernel auf...");
    let status = Command::new("cargo")
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "build",
            "-Z", "build-std=core,alloc,compiler_builtins",
            "-Z", "build-std-features=compiler-builtins-mem",
            "--manifest-path", "kernel/Cargo.toml",
            "--target", "x86_64-unknown-none",
            "--release"
        ])
        .status()
        .expect("FATAL: Cargo-Build-Prozess konnte nicht gestartet werden.");

    if !status.success() {
        eprintln!("❌ FATAL ERROR: Die Kernel-Kompilierung ist fehlgeschlagen.");
        std::process::exit(1);
    }

    // 2. Pfade definieren & absichern
    let kernel_elf = Path::new("target/x86_64-unknown-none/release/kernel");
    let build_dir = Path::new("../RustOS_Build_Output");
    
    if !build_dir.exists() {
        fs::create_dir_all(build_dir).expect("FATAL: Konnte Build-Ordner nicht erstellen");
    }
    
    let output_disk = build_dir.join("vibecore_desktop.img");
    let output_elf = build_dir.join("kernel.elf");

    // 3. Bootloader verknüpfen
    println!("[2/3] Konvertiere Kernel-ELF in ein bootfähiges MBR-Festplattenimage...");
    let bios_boot = bootloader::BiosBoot::new(kernel_elf);
    bios_boot.create_disk_image(&output_disk)
        .expect("FATAL: Das Erstellen des bootfähigen Festplattenimages ist fehlgeschlagen.");
        
    fs::copy(kernel_elf, &output_elf).expect("FATAL: Konnte kernel.elf nicht kopieren");

    println!("✅ Image erfolgreich generiert: {}", output_disk.display());

    if std::env::var("CI").is_ok() || std::env::var("GITHUB_ACTIONS").is_ok() {
        println!("✅ CI Environment erkannt. Lokale QEMU Ausführung wird übersprungen.");
        return;
    }

    // 4. QEMU sicher starten
    println!("[3/3] Starte QEMU Emulator mit angehängter virtueller Festplatte...");
    let qemu_result = Command::new("qemu-system-x86_64")
        .args([
            "-drive", &format!("format=raw,file={}", output_disk.display()),
            "-serial", "stdio",
            "-m", "256M", // Absicherung: Genug RAM für das OS erzwingen
            "-vga", "std" // Absicherung: Standard VGA für sauberes UI Rendering erzwingen
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
