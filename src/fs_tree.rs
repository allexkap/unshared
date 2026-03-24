//! Filesystem tree model and duplicate detection.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use indicatif::ProgressBar;
use walkdir::{DirEntry, WalkDir};

use crate::utils::hash_file;

pub use self::{
    index::{FileGroup, FileIndex},
    nodes::*,
};

mod index;
mod nodes;

type FsTreeArena = indextree::Arena<FsTreeNode>;

pub type FsTreeNodeId = indextree::NodeId;

/// Configuration for building an [`FsTree`].
#[derive(Clone, Copy, Debug)]
pub struct FsTreeConfig {
    /// Force hashing for files up to this size.
    pub force_hash_size: Option<u64>,
}

/// In-memory filesystem tree with duplicate index.
#[derive(Clone, Debug)]
pub struct FsTree {
    /// Node storage arena.
    arena: FsTreeArena,
    /// Root nodes and their base paths.
    roots: HashMap<FsTreeNodeId, PathBuf>,
    /// File duplicate index.
    index: FileIndex,
    /// Configuration.
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

    pub fn add_root(&mut self, path: impl AsRef<Path>, progress_bar: ProgressBar) {
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

        progress_bar.set_message("Stage 1: Traversing the file system");

        let mut stack: Vec<FsTreeNodeId> = vec![];
        for entry in entries {
            stack.truncate(entry.depth());

            let node = self.process_entry(&entry);
            let node_id = self.arena.new_node(node);
            if let Some(parent) = stack.last() {
                parent.append(node_id, &mut self.arena);
            }

            if let NodeKind::File(file_node) = self.arena[node_id].get().kind {
                match self.index.fast_add(node_id, file_node.data) {
                    FileGroup::Unique(_) => {
                        progress_bar.inc_length(1);
                        progress_bar.inc(1);
                    }
                    FileGroup::Duplicates(node_ids) => {
                        progress_bar.inc_length(1);
                        if node_ids.len() == 2 {
                            progress_bar.dec(1);
                        }
                    }
                }
            }

            stack.push(node_id);
        }

        let root = stack.into_iter().next().unwrap();
        self.resolve(root);
        self.roots
            .insert(root, path.parent().unwrap_or(Path::new("")).to_path_buf());

        let ambiguous_files = self.index.remove_ambiguous();

        progress_bar.set_message("Stage 2: Hashing similar files");

        for (node_id, data) in ambiguous_files.into_iter() {
            let path = self.get_full_path(node_id);
            let new_data = FileData {
                size: data.size,
                hash: Some(hash_file(path).unwrap()),
            };
            let file_node = self.arena[node_id].get_mut().kind.as_file_mut().unwrap();
            file_node.data = new_data;
            self.index.fast_add(node_id, file_node.data);
            progress_bar.inc(1);
        }

        progress_bar.finish_and_clear();

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

    pub fn get_roots(&self) -> Vec<(FsTreeNodeId, &OsStr)> {
        self.roots
            .iter()
            .map(|(&node_id, path)| (node_id, path.as_os_str()))
            .collect()
    }

    pub fn get_parent(&self, node_id: FsTreeNodeId) -> Option<FsTreeNodeId> {
        self.arena[node_id].parent()
    }

    pub fn get_children(&self, node_id: FsTreeNodeId) -> Vec<FsTreeNodeId> {
        node_id.children(&self.arena).collect()
    }

    pub fn get_node(&self, node_id: FsTreeNodeId) -> &FsTreeNode {
        self.arena[node_id].get()
    }

    pub fn get_same_nodes(&self, node_id: FsTreeNodeId) -> Option<&FileGroup> {
        self.arena[node_id]
            .get()
            .kind
            .as_file()
            .and_then(|file_node| self.index.get(file_node.data))
    }

    fn process_entry(&self, entry: &DirEntry) -> FsTreeNode {
        let name = entry.file_name().to_owned();
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

                NodeKind::File(FileNode {
                    modified,
                    data,
                    copies_count: 0,
                })
            }
            ft if ft.is_dir() => NodeKind::Dir(DirNode::default()),
            ft => NodeKind::Error(format!("Unsupported filetype={ft:?}")),
        };
        FsTreeNode { name, kind }
    }

    fn print_subtree(&self, node_id: FsTreeNodeId, depth: usize) {
        println!("{}{:?}", "  ".repeat(depth), self.arena[node_id].get());

        for child in node_id.children(&self.arena) {
            self.print_subtree(child, depth + 1);
        }
    }

    fn resolve(&mut self, node_id: FsTreeNodeId) -> DirNode {
        let kind = match self.arena[node_id].get().kind {
            NodeKind::File(file_node) => NodeKind::File(FileNode {
                copies_count: self.index.get(file_node.data).unwrap().len() as u64,
                ..file_node
            }),
            NodeKind::Dir(_) => NodeKind::Dir(
                node_id
                    .children(&self.arena)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|child_id| self.resolve(child_id))
                    .sum::<DirNode>(),
            ),
            ref other => other.clone(),
        };

        let deer = kind.like_a_deer();

        self.arena[node_id].get_mut().kind = kind;

        deer
    }

    pub fn get_full_path(&self, node_id: FsTreeNodeId) -> PathBuf {
        let node = &self.arena[node_id];
        match node.parent() {
            Some(parent_id) => self.get_full_path(parent_id),
            None => self.roots[&node_id].clone(),
        }
        .join(node.get().name.clone())
    }
}
