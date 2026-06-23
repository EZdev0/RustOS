use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use lazy_static::lazy_static;
use spin::Mutex;
use alloc::format;

#[derive(Debug, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    AlreadyExists,
    IsDirectory,
    NotADirectory,
    NotEmpty,
}

lazy_static! {
    pub static ref RAM_FS: SafeRamFs = SafeRamFs::new();
}

pub struct SafeRamFs {
    inner: Mutex<RamFs>,
}

impl Default for SafeRamFs {
    fn default() -> Self {
        Self::new()
    }
}

impl SafeRamFs {
    pub fn new() -> Self {
        Self { inner: Mutex::new(RamFs::new()) }
    }

    pub fn write_file(&self, path: &str, content: &[u8]) -> Result<(), FsError> {
        x86_64::instructions::interrupts::without_interrupts(|| self.inner.lock().write_file(path, content))
    }

    pub fn mkdir(&self, path: &str) -> Result<(), FsError> {
        x86_64::instructions::interrupts::without_interrupts(|| self.inner.lock().mkdir(path))
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError> {
        x86_64::instructions::interrupts::without_interrupts(|| self.inner.lock().read_file(path))
    }

    pub fn delete_file(&self, path: &str) -> Result<(), FsError> {
        x86_64::instructions::interrupts::without_interrupts(|| self.inner.lock().delete_file(path))
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<(String, bool)>, FsError> {
        x86_64::instructions::interrupts::without_interrupts(|| self.inner.lock().list_dir(path))
    }

    pub fn list_files(&self) -> Vec<String> {
        x86_64::instructions::interrupts::without_interrupts(|| self.inner.lock().list_files())
    }
}

pub enum FsNode {
    File { content: Vec<u8> },
    Directory { children: BTreeMap<String, FsNode> },
}

pub struct RamFs {
    root: FsNode,
}

impl Default for RamFs {
    fn default() -> Self {
        Self::new()
    }
}

impl RamFs {
    pub fn new() -> Self {
        Self {
            root: FsNode::Directory { children: BTreeMap::new() },
        }
    }

    fn resolve_node<'a>(&'a self, path: &str) -> Option<&'a FsNode> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = &self.root;
        
        for part in parts {
            match current {
                FsNode::Directory { children } => {
                    if let Some(child) = children.get(part) {
                        current = child;
                    } else {
                        return None;
                    }
                }
                FsNode::File { .. } => return None,
            }
        }
        Some(current)
    }

    pub fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() { return Err(FsError::NotADirectory); }
        
        let file_name = parts.last().unwrap();
        let dir_parts = &parts[0..parts.len()-1];
        
        let mut current = &mut self.root;
        for part in dir_parts {
            match current {
                FsNode::Directory { children } => {
                    current = children.entry(String::from(*part)).or_insert(FsNode::Directory { children: BTreeMap::new() });
                }
                FsNode::File { .. } => return Err(FsError::NotADirectory),
            }
        }
        
        if let FsNode::Directory { children } = current {
            if let Some(FsNode::Directory { .. }) = children.get(*file_name) {
                return Err(FsError::IsDirectory);
            }
            children.insert(String::from(*file_name), FsNode::File { content: content.to_vec() });
            Ok(())
        } else {
            Err(FsError::NotADirectory)
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() { return Err(FsError::NotADirectory); }
        
        let mut current = &mut self.root;
        for part in parts {
            match current {
                FsNode::Directory { children } => {
                    current = children.entry(String::from(part)).or_insert(FsNode::Directory { children: BTreeMap::new() });
                }
                FsNode::File { .. } => return Err(FsError::NotADirectory),
            }
        }
        Ok(())
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError> {
        if let Some(FsNode::File { content }) = self.resolve_node(path) {
            Ok(content.clone())
        } else {
            Err(FsError::NotFound)
        }
    }
    
    pub fn delete_file(&mut self, path: &str) -> Result<(), FsError> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() { return Err(FsError::NotFound); }
        
        let name = parts.last().unwrap();
        let dir_parts = &parts[0..parts.len()-1];
        
        let mut current = &mut self.root;
        for part in dir_parts {
            match current {
                FsNode::Directory { children } => {
                    if let Some(child) = children.get_mut(*part) {
                        current = child;
                    } else {
                        return Err(FsError::NotFound);
                    }
                }
                FsNode::File { .. } => return Err(FsError::NotADirectory),
            }
        }
        
        if let FsNode::Directory { children } = current {
            if let Some(FsNode::Directory { children: sub }) = children.get(*name) {
                if !sub.is_empty() {
                    return Err(FsError::NotEmpty);
                }
            }
            if children.remove(*name).is_some() {
                Ok(())
            } else {
                Err(FsError::NotFound)
            }
        } else {
            Err(FsError::NotADirectory)
        }
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<(String, bool)>, FsError> {
        if let Some(FsNode::Directory { children }) = self.resolve_node(path) {
            let mut list = Vec::new();
            for (name, node) in children.iter() {
                let is_dir = match node {
                    FsNode::Directory { .. } => true,
                    FsNode::File { .. } => false,
                };
                list.push((name.clone(), is_dir));
            }
            Ok(list)
        } else {
            Err(FsError::NotFound)
        }
    }
    
    pub fn list_files(&self) -> Vec<String> {
        let mut list = Vec::new();
        self.collect_flat(&self.root, "", &mut list);
        list
    }
    
    fn collect_flat(&self, node: &FsNode, current_path: &str, list: &mut Vec<String>) {
        match node {
            FsNode::File { .. } => {
                if !current_path.is_empty() {
                    list.push(String::from(current_path));
                }
            }
            FsNode::Directory { children } => {
                for (name, child) in children.iter() {
                    let next_path = if current_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{}/{}", current_path, name)
                    };
                    self.collect_flat(child, &next_path, list);
                }
            }
        }
    }
}
