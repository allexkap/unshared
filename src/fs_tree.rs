//! Filesystem tree model and duplicate detection.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use indicatif::ProgressBar;
use log::warn;
use walkdir::{DirEntry, WalkDir};

use crate::utils::hash_file;

pub use self::{
    index::{FileGroup, FileIndex},
    nodes::*,
};

mod index;
mod nodes;
mod serde_impl;

type FsTreeArena = indextree::Arena<FsTreeNode>;

pub type FsTreeNodeId = indextree::NodeId;

/// Configuration for building an [`FsTree`].
#[derive(Default, Clone, Debug)]
pub struct FsTreeConfig {
    /// Force hashing for files up to this size.
    pub force_hash_size: Option<u64>,
    /// FsTree instance used as a hash cache.
    pub cache_tree: Option<Box<FsTree>>,
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

    pub fn add_root(
        &mut self,
        path: impl AsRef<Path>,
        progress_bar: ProgressBar,
    ) -> io::Result<()> {
        let root_path = path.as_ref().canonicalize()?;

        progress_bar.set_message("Stage 1: Traversing the file system");

        let mut stack: Vec<FsTreeNodeId> = vec![];
        for entry in WalkDir::new(&root_path).sort_by_file_name() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    let Some(&last_node_id) = stack.last() else {
                        return Err(err.into());
                    };

                    let last_node_path = self.get_full_path(last_node_id);
                    if let Some(err_path) = err.path() {
                        assert_eq!(err_path, last_node_path);
                    }

                    let err_node = err
                        .into_io_error()
                        .expect("WalkDir with follow_links=false return not IO error")
                        .to_string();

                    warn!("{:?}: {}", self.get_full_path(last_node_id), err_node);
                    self.arena[last_node_id].get_mut().kind = NodeKind::Error(err_node);
                    continue;
                }
            };

            stack.truncate(entry.depth());

            let node = FsTreeNode {
                name: entry.file_name().to_owned(),
                kind: self
                    .process_entry(&entry)
                    .unwrap_or_else(|err| NodeKind::Error(err.to_string())),
            };

            let node_id = self.arena.new_node(node);
            if let Some(parent) = stack.last() {
                parent.append(node_id, &mut self.arena);
            } else {
                self.roots.insert(
                    node_id,
                    root_path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or(PathBuf::default()),
                );
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

        progress_bar.set_message("Stage 2: Hashing similar files");
        for (node_id, _) in self.index.remove_ambiguous().into_iter() {
            let path = self.get_full_path(node_id);
            let node = self.arena[node_id].get_mut();
            match hash_file(path) {
                Ok(hash) => {
                    let file_node = node
                        .kind
                        .as_file_mut()
                        .expect("node in ambiguous_files is not a file");
                    file_node.data.hash = Some(hash);
                    self.index.fast_add(node_id, file_node.data);
                    progress_bar.inc(1);
                }
                Err(err) => {
                    node.kind = NodeKind::Error(err.to_string());
                    progress_bar.dec_length(1);
                }
            }
        }
        assert_eq!(self.index.remove_ambiguous().len(), 0);

        self.resolve_roots();
        progress_bar.finish_and_clear();

        Ok(())
    }

    pub fn get_roots(&self) -> Vec<(FsTreeNodeId, &PathBuf)> {
        self.roots
            .iter()
            .map(|(&node_id, path)| (node_id, path))
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

    fn process_entry(&self, entry: &DirEntry) -> io::Result<NodeKind> {
        match entry.file_type() {
            ft if ft.is_file() => {
                let meta = entry.metadata()?;

                let size = meta.len();
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());

                let cached = self.get_file_node_from_cache(entry.path()).and_then(|f| {
                    f.data.hash.filter(|_| match (modified, f.modified) {
                        (Some(m0), Some(m1)) => m0 <= m1,
                        (None, _) => true,
                        _ => false,
                    })
                });
                let hash = if let Some(h) = cached {
                    Some(h)
                } else if self
                    .config
                    .force_hash_size
                    .is_some_and(|x| x >= size && size != 0)
                {
                    Some(hash_file(entry.path())?)
                } else {
                    None
                };

                let data = FileData { size, hash };

                Ok(NodeKind::File(FileNode {
                    modified,
                    data,
                    copies_count: 0,
                }))
            }
            ft if ft.is_dir() => Ok(NodeKind::Dir(DirNode::default())),
            ft if ft.is_symlink() => Ok(NodeKind::SymLink(
                fs::read_link(entry.path())?.to_string_lossy().to_string(),
            )),
            _ => Ok(NodeKind::Error("Unknown filetype".to_string())),
        }
    }

    fn resolve_roots(&mut self) {
        for root in self.roots.keys().cloned().collect::<Vec<_>>() {
            self.resolve(root);
        }
    }

    fn resolve(&mut self, node_id: FsTreeNodeId) -> DirNode {
        let kind = match self.arena[node_id].get().kind {
            NodeKind::File(file_node) => NodeKind::File(FileNode {
                data: file_node.data,
                modified: file_node.modified,
                copies_count: self
                    .index
                    .get(file_node.data)
                    .expect("file in fs_tree is missing in the index")
                    .len() as u64,
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

    pub fn get_node_by_path(&self, node_path: &Path) -> Option<FsTreeNodeId> {
        let Some((mut id, path)) = self
            .roots
            .iter()
            .filter_map(|(&id, root_path)| {
                Some((
                    id,
                    node_path
                        .strip_prefix(root_path.join(self.get_node(id).name.as_os_str()))
                        .ok()?,
                ))
            })
            .next()
        else {
            return None;
        };

        for part in path.components() {
            match self
                .get_children(id)
                .iter()
                .filter(|&&child_id| self.get_node(child_id).name == part.as_os_str())
                .next()
            {
                Some(&child_id) => id = child_id,
                None => {
                    return None;
                }
            }
        }

        Some(id)
    }

    fn get_file_node_from_cache(&self, path: &Path) -> Option<&FileNode> {
        self.config
            .cache_tree
            .as_ref()
            .and_then(|cache| Some((cache, cache.get_node_by_path(path)?)))
            .and_then(|(cache, id)| cache.get_node(id).kind.as_file())
    }
}
