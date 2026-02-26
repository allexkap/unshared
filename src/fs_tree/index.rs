use std::{
    cmp,
    collections::{HashMap, hash_map},
    iter,
};

use super::{FsTreeNodeId, nodes::FileData};

#[derive(Clone, Debug)]
enum FileGroup {
    Uniq(FsTreeNodeId),
    Many(Vec<FsTreeNodeId>),
}

#[derive(Default, Clone, Debug)]
pub struct FileIndex {
    grouped_files: HashMap<FileData, FileGroup>,
}

impl FileIndex {
    pub fn fast_add(&mut self, node_id: FsTreeNodeId, file_data: FileData) {
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

    pub fn remove_ambiguous(&mut self) -> Vec<(FsTreeNodeId, FileData)> {
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

    pub fn get_preview(&self) -> Vec<(FileData, &Vec<FsTreeNodeId>)> {
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
