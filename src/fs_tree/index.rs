//! File indexing and duplicate grouping logic.

use std::{
    collections::{HashMap, hash_map},
    iter,
};

use super::{FsTreeNodeId, nodes::FileData};

/// Group of files that share identical [`FileData`].
#[derive(Clone, Debug)]
pub enum FileGroup {
    /// Exactly one file with this data.
    Unique(FsTreeNodeId),
    /// Two or more files with identical data.
    Duplicates(Vec<FsTreeNodeId>),
}

/// Index mapping file data to nodes.
#[derive(Default, Clone, Debug)]
pub struct FileIndex {
    /// Grouped files by identity.
    grouped_files: HashMap<FileData, FileGroup>,
}

impl FileGroup {
    pub fn len(&self) -> usize {
        match self {
            FileGroup::Unique(_) => 1,
            FileGroup::Duplicates(node_ids) => node_ids.len(),
        }
    }
}

impl FileIndex {
    pub fn fast_add(&mut self, node_id: FsTreeNodeId, file_data: FileData) -> &FileGroup {
        if file_data.hash.is_some() {
            let prev_group = self
                .grouped_files
                .entry(FileData {
                    hash: None,
                    ..file_data
                })
                .or_insert_with(|| FileGroup::Duplicates(vec![]));

            if let FileGroup::Unique(prev_node_id) = prev_group {
                *prev_group = FileGroup::Duplicates(vec![*prev_node_id]);
            }
        }

        match self.grouped_files.entry(file_data) {
            hash_map::Entry::Occupied(mut entry) => {
                let prev_group = entry.get_mut();
                match prev_group {
                    FileGroup::Unique(prev_node_id) => {
                        *prev_group = FileGroup::Duplicates(vec![*prev_node_id, node_id]);
                    }
                    FileGroup::Duplicates(file_group) => file_group.push(node_id),
                }
                entry.into_mut()
            }
            hash_map::Entry::Vacant(entry) => entry.insert(FileGroup::Unique(node_id)),
        }
    }

    pub fn remove_ambiguous(&mut self) -> Vec<(FsTreeNodeId, FileData)> {
        self.grouped_files
            .iter_mut()
            .filter_map(|entry| match entry {
                (file_data, FileGroup::Duplicates(file_group))
                    if file_data.hash.is_none() && file_data.size != 0 =>
                {
                    Some(file_group.drain(..).zip(iter::repeat(*entry.0)))
                }
                _ => None,
            })
            .flatten()
            .collect()
    }

    pub fn get(&self, file_data: FileData) -> Option<&FileGroup> {
        self.grouped_files.get(&file_data)
    }
}
