//! Filesystem node types and related data structures.

use std::{ffi::OsString, fmt, time::SystemTime};

use enum_as_inner::EnumAsInner;

use crate::utils::use_si_postfix;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileData {
    pub size: u64,
    pub hash: Option<u128>,
}

#[derive(Clone, Copy, Debug)]
pub struct FileNode {
    pub data: FileData,
    pub modified: Option<SystemTime>,
    pub copies_count: u64,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct DirNode {
    pub total_size: u64,
    pub dirs_count: u64,
    pub files_count: u64,
    pub unique_files_count: u64,
}

#[derive(Clone, Debug, EnumAsInner)]
pub enum NodeKind {
    Dir(DirNode),
    File(FileNode),
    Error(String),
}

impl NodeKind {
    pub fn get_total_size(&self) -> u64 {
        match self {
            NodeKind::Dir(dir_node) => dir_node.total_size,
            NodeKind::File(file_node) => file_node.data.size,
            _ => 0,
        }
    }

    pub fn get_uniqueness(&self) -> f64 {
        let deer = self.like_a_deer();
        return deer.unique_files_count as f64 / (deer.files_count as f64);
    }

    pub fn like_a_deer(&self) -> DirNode {
        match *self {
            NodeKind::Dir(dir_node) => dir_node,
            NodeKind::File(file_node) => DirNode {
                total_size: file_node.data.size,
                dirs_count: 0,
                files_count: 1,
                unique_files_count: (file_node.copies_count == 1) as u64,
            },
            _ => DirNode::default(),
        }
    }
}

impl std::ops::Add for DirNode {
    type Output = DirNode;

    fn add(self, rhs: Self) -> Self::Output {
        DirNode {
            total_size: self.total_size + rhs.total_size,
            dirs_count: self.dirs_count + rhs.dirs_count,
            files_count: self.files_count + rhs.files_count,
            unique_files_count: self.unique_files_count + rhs.unique_files_count,
        }
    }
}

impl std::iter::Sum for DirNode {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(DirNode::default(), |a, b| a + b)
    }
}

#[derive(Clone, Debug)]
pub struct FsTreeNode {
    pub name: OsString,
    pub kind: NodeKind,
}

impl fmt::Display for FileData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = use_si_postfix(self.size);

        let hash_str = match self.hash {
            Some(hash) => format!("{hash:016x}"),
            None => "-".to_owned(),
        };

        write!(f, "FileData({hash_str}: {size})",)
    }
}
