use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use lazy_static::lazy_static;
use spin::Mutex;

lazy_static! {
    // Globale Instanz des Dateisystems, auf die alle Apps (Terminal, Notepad, FileManager) Zugriff haben.
    pub static ref RAM_FS: Mutex<RamFs> = Mutex::new(RamFs::new());
}

pub struct File {
    pub name: String,
    pub content: Vec<u8>,
}

pub struct RamFs {
    files: BTreeMap<String, File>,
}

impl RamFs {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    /// Schreibt eine Datei ins RAM-FS (überschreibt bestehende Dateien)
    pub fn write_file(&mut self, path: &str, content: &[u8]) {
        self.files.insert(
            String::from(path),
            File {
                name: String::from(path),
                content: content.to_vec(),
            },
        );
    }

    /// Liest eine Datei aus dem RAM-FS
    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        self.files.get(path).map(|f| f.content.clone())
    }

    /// Listet alle Dateipfade auf
    pub fn list_files(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
    
    /// Löscht eine Datei
    pub fn delete_file(&mut self, path: &str) -> bool {
        self.files.remove(path).is_some()
    }
}
