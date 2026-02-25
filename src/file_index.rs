use std::{
    cmp,
    collections::{HashMap, hash_map},
    fmt, iter,
    path::{Path, PathBuf},
    time::SystemTime,
};

use enum_as_inner::EnumAsInner;
use tqdm::Iter;
use walkdir::{DirEntry, WalkDir};

use self::utils::hash_file;

mod utils;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileData {
    size: u64,
    hash: Option<u64>,
}

impl fmt::Display for FileData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
        let mut unit = UNITS[UNITS.len() - 1];
        let mut size = self.size as f64;

        for p in UNITS {
            if size <= 1000.0 {
                unit = p;
                break;
            }
            size /= 1000.0;
        }

        let hash_str = match self.hash {
            Some(hash) => format!("{hash:016x}"),
            None => "-".to_owned(),
        };

        write!(
            f,
            "FileData({hash_str}: {size:.0$}{unit})",
            if unit == UNITS[0] { 0 } else { 1 }
        )
    }
}

#[derive(Clone, Copy, Debug)]
struct FileNode {
    modified: Option<SystemTime>,
    data: FileData,
}

#[derive(Default, Clone, Copy, Debug)]
struct DirNode {
    total_size: u64,
}

#[derive(Clone, Copy, Debug)]
struct TestNode;

#[derive(Clone, Copy, Debug, EnumAsInner)]
enum NodeKind {
    File(FileNode),
    Dir(DirNode),
    Test(TestNode),
}

#[derive(Clone, Debug)]
struct Node {
    name: String,
    kind: NodeKind,
}

pub type FileTreeNodeId = indextree::NodeId;

#[derive(Clone, Debug)]
enum FileGroup {
    Uniq(FileTreeNodeId),
    Many(Vec<FileTreeNodeId>),
}

#[derive(Default, Clone, Debug)]
struct FileIndex {
    grouped_files: HashMap<FileData, FileGroup>,
}

impl FileIndex {
    fn fast_add(&mut self, node_id: FileTreeNodeId, file_data: FileData) {
        match self.grouped_files.entry(file_data) {
            hash_map::Entry::Occupied(mut entry) => {
                let prev_group = entry.get_mut();
                match prev_group {
                    FileGroup::Uniq(prev_node_id) => {
                        *prev_group = FileGroup::Many(vec![*prev_node_id, node_id]);
                    }
                    FileGroup::Many(file_group) => file_group.push(node_id),
                }
            }
            hash_map::Entry::Vacant(entry) => {
                entry.insert(FileGroup::Uniq(node_id));
            }
        }
    }

    fn remove_ambiguous(&mut self) -> Vec<(FileTreeNodeId, FileData)> {
        self.grouped_files
            .iter_mut()
            .filter_map(|entry| match entry {
                (file_data, FileGroup::Many(file_group))
                    if file_data.hash.is_none() && file_data.size != 0 =>
                {
                    Some(file_group.drain(..).zip(iter::repeat(*entry.0)))
                }
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn get_preview(&self) -> Vec<(FileData, &Vec<FileTreeNodeId>)> {
        let mut sorted_files: Vec<_> = self
            .grouped_files
            .iter()
            .filter_map(|entry| match entry {
                (data, FileGroup::Many(file_group)) if file_group.len() > 1 => {
                    Some((*data, file_group))
                }
                _ => None,
            })
            .collect();
        sorted_files.sort_by_key(|k| cmp::Reverse((k.0.size * k.1.len() as u64, k.0.hash)));
        sorted_files
    }
}

type FileTreeArena = indextree::Arena<Node>;

#[derive(Clone, Copy, Debug)]
pub struct FileTreeConfig {
    pub force_hash_size: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct FileTree {
    arena: FileTreeArena,
    roots: HashMap<FileTreeNodeId, PathBuf>,
    index: FileIndex,
    config: FileTreeConfig,
}

impl FileTree {
    pub fn new(config: FileTreeConfig) -> Self {
        Self {
            arena: FileTreeArena::new(),
            roots: HashMap::default(),
            index: FileIndex::default(),
            config,
        }
    }

    pub fn add_root(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref();
        let entries = WalkDir::new(path)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|res| match res {
                Ok(v) => Some(v),
                Err(e) => {
                    println!("{e}");
                    None
                }
            });

        let mut stack: Vec<FileTreeNodeId> = vec![];
        for entry in entries {
            stack.truncate(entry.depth());

            let node = self.process_entry(&entry);
            let node_id = self.arena.new_node(node);
            if let Some(parent) = stack.last() {
                parent.append(node_id, &mut self.arena);
            }

            if let NodeKind::File(file_node) = self.arena[node_id].get().kind {
                self.index.fast_add(node_id, file_node.data)
            }

            stack.push(node_id);
        }

        let root = stack.into_iter().next().unwrap();
        self.resolve_size(root);
        self.roots
            .insert(root, path.parent().unwrap_or(Path::new("")).to_path_buf());

        let ambiguous_files = self.index.remove_ambiguous();

        for (node_id, data) in ambiguous_files.into_iter().tqdm() {
            let path = self.get_full_path(node_id);
            let new_data = FileData {
                size: data.size,
                hash: Some(hash_file(path).unwrap()),
            };
            let file_node = self.arena[node_id].get_mut().kind.as_file_mut().unwrap();
            file_node.data = new_data;
            self.index.fast_add(node_id, file_node.data);
        }

        assert_eq!(self.index.remove_ambiguous().len(), 0);
    }

    pub fn len(&self) -> usize {
        self.arena.count()
    }

    pub fn print_tree(&self) {
        for (root_id, _) in self.roots.clone() {
            self.print_subtree(root_id, 0);
        }
    }

    pub fn get_preview(&self) -> Vec<(FileData, Vec<PathBuf>)> {
        self.index
            .get_preview()
            .into_iter()
            .map(|(file_data, nodes)| {
                (
                    file_data,
                    nodes
                        .iter()
                        .map(|node_id| self.get_full_path(*node_id))
                        .collect(),
                )
            })
            .collect()
    }

    fn process_entry(&self, entry: &DirEntry) -> Node {
        let name = entry.file_name().to_str().unwrap().to_owned();
        let kind = match entry.file_type() {
            ft if ft.is_file() => {
                let meta = entry.metadata().unwrap();

                let size = meta.len();
                let hash = if self
                    .config
                    .force_hash_size
                    .is_some_and(|x| x >= size && size != 0)
                {
                    Some(hash_file(entry.path()).unwrap())
                } else {
                    None
                };
                let data = FileData { size, hash };

                let modified = meta.modified().ok();

                NodeKind::File(FileNode { modified, data })
            }
            ft if ft.is_dir() => NodeKind::Dir(DirNode::default()),
            _ => NodeKind::Test(TestNode),
        };
        Node { name, kind }
    }

    fn print_subtree(&self, node_id: FileTreeNodeId, depth: usize) {
        println!("{}{:?}", "  ".repeat(depth), self.arena[node_id].get());

        for child in node_id.children(&self.arena) {
            self.print_subtree(child, depth + 1);
        }
    }

    fn resolve_size(&mut self, node_id: FileTreeNodeId) -> u64 {
        match self.arena[node_id].get().kind {
            NodeKind::File(file_node) => file_node.data.size,
            NodeKind::Dir(_) => {
                let total_size = node_id
                    .children(&self.arena)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|child_id| self.resolve_size(child_id))
                    .sum();
                self.arena[node_id]
                    .get_mut()
                    .kind
                    .as_dir_mut()
                    .expect("node type has changed unexpectedly during size resolution")
                    .total_size = total_size;
                total_size
            }
            NodeKind::Test(_) => 0,
        }
    }

    fn get_full_path(&self, node_id: FileTreeNodeId) -> PathBuf {
        let node = &self.arena[node_id];
        match node.parent() {
            Some(parent_id) => self.get_full_path(parent_id),
            None => self.roots[&node_id].clone(),
        }
        .join(node.get().name.clone())
    }
}
