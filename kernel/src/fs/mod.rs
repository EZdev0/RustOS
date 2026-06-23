use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use lazy_static::lazy_static;
use spin::Mutex;
use alloc::format;

lazy_static! {
    pub static ref RAM_FS: Mutex<RamFs> = Mutex::new(RamFs::new());
}

pub enum FsNode {
    File { content: Vec<u8> },
    Directory { children: BTreeMap<String, FsNode> },
}

pub struct RamFs {
    root: FsNode,
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

    pub fn write_file(&mut self, path: &str, content: &[u8]) {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() { return; }
        
        let file_name = parts.last().unwrap();
        let dir_parts = &parts[0..parts.len()-1];
        
        let mut current = &mut self.root;
        for part in dir_parts {
            match current {
                FsNode::Directory { children } => {
                    current = children.entry(String::from(*part)).or_insert(FsNode::Directory { children: BTreeMap::new() });
                }
                FsNode::File { .. } => return,
            }
        }
        
        if let FsNode::Directory { children } = current {
            children.insert(String::from(*file_name), FsNode::File { content: content.to_vec() });
        }
    }

    pub fn mkdir(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() { return; }
        
        let mut current = &mut self.root;
        for part in parts {
            match current {
                FsNode::Directory { children } => {
                    current = children.entry(String::from(part)).or_insert(FsNode::Directory { children: BTreeMap::new() });
                }
                FsNode::File { .. } => return,
            }
        }
    }

    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        if let Some(FsNode::File { content }) = self.resolve_node(path) {
            Some(content.clone())
        } else {
            None
        }
    }
    
    pub fn delete_file(&mut self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() { return false; }
        
        let name = parts.last().unwrap();
        let dir_parts = &parts[0..parts.len()-1];
        
        let mut current = &mut self.root;
        for part in dir_parts {
            match current {
                FsNode::Directory { children } => {
                    if let Some(child) = children.get_mut(*part) {
                        current = child;
                    } else {
                        return false;
                    }
                }
                FsNode::File { .. } => return false,
            }
        }
        
        if let FsNode::Directory { children } = current {
            children.remove(*name).is_some()
        } else {
            false
        }
    }

    pub fn list_dir(&self, path: &str) -> Option<Vec<(String, bool)>> {
        if let Some(FsNode::Directory { children }) = self.resolve_node(path) {
            let mut list = Vec::new();
            for (name, node) in children.iter() {
                let is_dir = match node {
                    FsNode::Directory { .. } => true,
                    FsNode::File { .. } => false,
                };
                list.push((name.clone(), is_dir));
            }
            Some(list)
        } else {
            None
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
