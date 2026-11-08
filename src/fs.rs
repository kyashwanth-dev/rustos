//! A simple in-memory filesystem: a tree of directories and files on the heap.
//! Nothing here touches a disk, so everything is lost on reboot.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone)]
pub enum Node {
    File(Vec<u8>),
    Dir(BTreeMap<String, Node>),
}

impl Node {
    fn size(&self) -> usize {
        match self {
            Node::File(data) => data.len(),
            Node::Dir(map) => map.values().map(|n| n.size()).sum(),
        }
    }
}

pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: usize,
}

pub type FsResult<T> = Result<T, String>;

pub struct FileSystem {
    root: Node,
    cwd: Vec<String>,
}

impl FileSystem {
    pub fn new() -> Self {
        FileSystem { root: Node::Dir(BTreeMap::new()), cwd: Vec::new() }
    }

    pub fn cwd_string(&self) -> String {
        join(&self.cwd)
    }

    /// Turn a user path (absolute or relative, with . and ..) into components.
    pub fn absolute(&self, path: &str) -> Vec<String> {
        let mut comps = if path.starts_with('/') { Vec::new() } else { self.cwd.clone() };
        for part in path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    comps.pop();
                }
                name => comps.push(name.to_string()),
            }
        }
        comps
    }

    pub fn absolute_string(&self, path: &str) -> String {
        join(&self.absolute(path))
    }

    fn get(&self, comps: &[String]) -> Option<&Node> {
        let mut node = &self.root;
        for c in comps {
            node = match node {
                Node::Dir(map) => map.get(c)?,
                Node::File(_) => return None,
            };
        }
        Some(node)
    }

    fn get_mut(&mut self, comps: &[String]) -> Option<&mut Node> {
        let mut node = &mut self.root;
        for c in comps {
            node = match node {
                Node::Dir(map) => map.get_mut(c)?,
                Node::File(_) => return None,
            };
        }
        Some(node)
    }

    fn split_last(comps: &[String]) -> FsResult<(&[String], &String)> {
        match comps.split_last() {
            Some((name, parent)) => Ok((parent, name)),
            None => Err("operation not allowed on the root directory".to_string()),
        }
    }

    fn dir_map_mut(&mut self, comps: &[String]) -> FsResult<&mut BTreeMap<String, Node>> {
        match self.get_mut(comps) {
            Some(Node::Dir(map)) => Ok(map),
            Some(Node::File(_)) => Err("not a directory".to_string()),
            None => Err("no such directory".to_string()),
        }
    }

    pub fn list(&self, path: &str) -> FsResult<Vec<Entry>> {
        let comps = self.absolute(path);
        match self.get(&comps) {
            Some(Node::Dir(map)) => Ok(map
                .iter()
                .map(|(name, node)| Entry {
                    name: name.clone(),
                    is_dir: matches!(node, Node::Dir(_)),
                    size: node.size(),
                })
                .collect()),
            Some(Node::File(data)) => Ok(alloc::vec![Entry {
                name: comps.last().cloned().unwrap_or_default(),
                is_dir: false,
                size: data.len(),
            }]),
            None => Err(format!("no such file or directory: {}", path)),
        }
    }

    pub fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        match self.get(&self.absolute(path)) {
            Some(Node::File(data)) => Ok(data.clone()),
            Some(Node::Dir(_)) => Err(format!("{}: is a directory", path)),
            None => Err(format!("no such file: {}", path)),
        }
    }

    pub fn is_dir(&self, path: &str) -> bool {
        matches!(self.get(&self.absolute(path)), Some(Node::Dir(_)))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.get(&self.absolute(path)).is_some()
    }

    /// Write (or append to) a file, creating it if needed. The parent directory must exist.
    pub fn write(&mut self, path: &str, data: Vec<u8>, append: bool) -> FsResult<()> {
        let comps = self.absolute(path);
        let (parent, name) = Self::split_last(&comps)?;
        let map = self.dir_map_mut(parent)?;
        match map.get_mut(name) {
            Some(Node::File(existing)) => {
                if append {
                    existing.extend_from_slice(&data);
                } else {
                    *existing = data;
                }
            }
            Some(Node::Dir(_)) => return Err(format!("{}: is a directory", path)),
            None => {
                map.insert(name.clone(), Node::File(data));
            }
        }
        Ok(())
    }

    pub fn mkdir(&mut self, path: &str) -> FsResult<()> {
        let comps = self.absolute(path);
        let (parent, name) = Self::split_last(&comps)?;
        let map = self.dir_map_mut(parent)?;
        if map.contains_key(name) {
            return Err(format!("{}: already exists", path));
        }
        map.insert(name.clone(), Node::Dir(BTreeMap::new()));
        Ok(())
    }

    pub fn remove(&mut self, path: &str, recursive: bool) -> FsResult<()> {
        let comps = self.absolute(path);
        let (parent, name) = Self::split_last(&comps)?;
        let map = self.dir_map_mut(parent)?;
        match map.get(name) {
            None => return Err(format!("no such file or directory: {}", path)),
            Some(Node::Dir(children)) if !children.is_empty() && !recursive => {
                return Err(format!("{}: directory not empty (use rm -r)", path));
            }
            _ => {}
        }
        map.remove(name);
        // If we deleted a directory we were standing in, step back out of it.
        if self.get(&self.cwd.clone()).is_none() {
            self.cwd = comps[..comps.len() - 1].to_vec();
        }
        Ok(())
    }

    pub fn chdir(&mut self, path: &str) -> FsResult<()> {
        let comps = self.absolute(path);
        match self.get(&comps) {
            Some(Node::Dir(_)) => {
                self.cwd = comps;
                Ok(())
            }
            Some(Node::File(_)) => Err(format!("{}: not a directory", path)),
            None => Err(format!("no such directory: {}", path)),
        }
    }

    /// Work out where `src` should end up when copied/moved to `dst`:
    /// inside `dst` if it is an existing directory, otherwise AS `dst`.
    fn destination(&self, src_name: &str, dst: &str) -> FsResult<(Vec<String>, String)> {
        let dst_comps = self.absolute(dst);
        if let Some(Node::Dir(_)) = self.get(&dst_comps) {
            return Ok((dst_comps, src_name.to_string()));
        }
        let (parent, name) = Self::split_last(&dst_comps)?;
        Ok((parent.to_vec(), name.clone()))
    }

    fn place(&mut self, parent: &[String], name: String, node: Node) -> FsResult<()> {
        let map = self.dir_map_mut(parent)?;
        match (map.get(&name), &node) {
            (None, _) | (Some(Node::File(_)), Node::File(_)) => {
                map.insert(name, node);
                Ok(())
            }
            _ => Err(format!("{}: destination already exists", name)),
        }
    }

    pub fn copy(&mut self, src: &str, dst: &str) -> FsResult<()> {
        let src_comps = self.absolute(src);
        let node = self.get(&src_comps).cloned().ok_or(format!("no such file or directory: {}", src))?;
        let src_name = src_comps.last().cloned().ok_or("cannot copy the root directory".to_string())?;
        let (parent, name) = self.destination(&src_name, dst)?;
        if matches!(node, Node::Dir(_)) && is_prefix(&src_comps, &parent) {
            return Err("cannot copy a directory into itself".to_string());
        }
        self.place(&parent, name, node)
    }

    pub fn rename(&mut self, src: &str, dst: &str) -> FsResult<()> {
        let src_comps = self.absolute(src);
        let node = self.get(&src_comps).cloned().ok_or(format!("no such file or directory: {}", src))?;
        let src_name = src_comps.last().cloned().ok_or("cannot move the root directory".to_string())?;
        let (parent, name) = self.destination(&src_name, dst)?;
        if matches!(node, Node::Dir(_)) && is_prefix(&src_comps, &parent) {
            return Err("cannot move a directory into itself".to_string());
        }
        let mut target = parent.clone();
        target.push(name.clone());
        if target == src_comps {
            return Ok(()); // moving something onto itself
        }
        self.place(&parent, name, node)?;
        // Remove the original (the place() above already succeeded).
        let (sparent, sname) = Self::split_last(&src_comps)?;
        let sparent = sparent.to_vec();
        let sname = sname.clone();
        self.dir_map_mut(&sparent)?.remove(&sname);
        Ok(())
    }

    /// Flatten a directory tree into (depth, name, is_dir) rows for `tree`.
    pub fn walk(&self, path: &str) -> FsResult<Vec<(usize, String, bool)>> {
        let comps = self.absolute(path);
        let mut out = Vec::new();
        match self.get(&comps) {
            Some(Node::Dir(map)) => walk_map(map, 0, &mut out),
            Some(Node::File(_)) => return Err(format!("{}: not a directory", path)),
            None => return Err(format!("no such directory: {}", path)),
        }
        Ok(out)
    }

    pub fn total_bytes(&self) -> usize {
        self.root.size()
    }
}

fn walk_map(map: &BTreeMap<String, Node>, depth: usize, out: &mut Vec<(usize, String, bool)>) {
    for (name, node) in map {
        match node {
            Node::Dir(children) => {
                out.push((depth, name.clone(), true));
                walk_map(children, depth + 1, out);
            }
            Node::File(_) => out.push((depth, name.clone(), false)),
        }
    }
}

fn join(comps: &[String]) -> String {
    if comps.is_empty() {
        return "/".to_string();
    }
    let mut s = String::new();
    for c in comps {
        s.push('/');
        s.push_str(c);
    }
    s
}

/// Is `prefix` an ancestor of (or equal to) `path`?
fn is_prefix(prefix: &[String], path: &[String]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}
