use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use tqdm::Iter;
use walkdir::{DirEntry, WalkDir};

use self::{index::FileIndex, nodes::*, utils::hash_file};

mod index;
mod nodes;
mod utils;

type FsTreeArena = indextree::Arena<FsTreeNode>;

pub type FsTreeNodeId = indextree::NodeId;

#[derive(Clone, Copy, Debug)]
pub struct FsTreeConfig {
    pub force_hash_size: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct FsTree {
    arena: FsTreeArena,
    roots: HashMap<FsTreeNodeId, PathBuf>,
    index: FileIndex,
    config: FsTreeConfig,
}

impl FsTree {
    pub fn new(config: FsTreeConfig) -> Self {
        Self {
            arena: FsTreeArena::new(),
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

        let mut stack: Vec<FsTreeNodeId> = vec![];
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

    pub fn get_roots(&self) -> Vec<(FsTreeNodeId, String)> {
        self.roots
            .iter()
            .map(|(&node_id, path)| (node_id, path.to_string_lossy().into_owned()))
            .collect()
    }

    pub fn get_parent(&self, node_id: FsTreeNodeId) -> Option<FsTreeNodeId> {
        self.arena[node_id].parent()
    }

    pub fn get_children(&self, node_id: FsTreeNodeId) -> Vec<FsTreeNodeId> {
        node_id.children(&self.arena).collect()
    }

    pub fn get_name(&self, node_id: FsTreeNodeId) -> &str {
        &self.arena[node_id].get().name
    }

    fn process_entry(&self, entry: &DirEntry) -> FsTreeNode {
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
        FsTreeNode { name, kind }
    }

    fn print_subtree(&self, node_id: FsTreeNodeId, depth: usize) {
        println!("{}{:?}", "  ".repeat(depth), self.arena[node_id].get());

        for child in node_id.children(&self.arena) {
            self.print_subtree(child, depth + 1);
        }
    }

    fn resolve_size(&mut self, node_id: FsTreeNodeId) -> u64 {
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

    fn get_full_path(&self, node_id: FsTreeNodeId) -> PathBuf {
        let node = &self.arena[node_id];
        match node.parent() {
            Some(parent_id) => self.get_full_path(parent_id),
            None => self.roots[&node_id].clone(),
        }
        .join(node.get().name.clone())
    }
}
