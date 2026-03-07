//! Filesystem node types and related data structures.

use std::{ffi::OsString, fmt, time::SystemTime};

use enum_as_inner::EnumAsInner;

use crate::utils::bytes_to_string;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileData {
    pub size: u64,
    pub hash: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct FileNode {
    pub modified: Option<SystemTime>,
    pub dupes_count: u64,
    pub data: FileData,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct DirNode {
    pub total_size: u64,
    pub files_count: u64,
    pub dupes_count: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct TestNode;

#[derive(Clone, Copy, Debug, EnumAsInner)]
pub enum NodeKind {
    File(FileNode),
    Dir(DirNode),
    Test(TestNode),
}

impl NodeKind {
    pub fn get_total_size(&self) -> u64 {
        match self {
            NodeKind::File(file_node) => file_node.data.size,
            NodeKind::Dir(dir_node) => dir_node.total_size,
            NodeKind::Test(_) => 0,
        }
    }
}

impl std::ops::Add for NodeKind {
    type Output = NodeKind;

    fn add(self, rhs: Self) -> Self::Output {
        fn convert_to_dir(node_kind: NodeKind) -> DirNode {
            match node_kind {
                NodeKind::File(file_node) => DirNode {
                    total_size: file_node.data.size,
                    files_count: 1,
                    dupes_count: if file_node.dupes_count != 1 { 1 } else { 0 },
                },
                NodeKind::Dir(dir_node) => dir_node,
                NodeKind::Test(_) => DirNode {
                    total_size: 0,
                    files_count: 0,
                    dupes_count: 0,
                },
            }
        }

        let lhs_dir = convert_to_dir(self);
        let rhs_dir = convert_to_dir(rhs);
        NodeKind::Dir(DirNode {
            total_size: lhs_dir.total_size + rhs_dir.total_size,
            files_count: lhs_dir.files_count + rhs_dir.files_count,
            dupes_count: lhs_dir.dupes_count + rhs_dir.dupes_count,
        })
    }
}

impl std::iter::Sum for NodeKind {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(NodeKind::Test(TestNode), |a, b| a + b)
    }
}

#[derive(Clone, Debug)]
pub struct FsTreeNode {
    pub name: OsString,
    pub kind: NodeKind,
}

impl fmt::Display for FileData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = bytes_to_string(self.size);

        let hash_str = match self.hash {
            Some(hash) => format!("{hash:016x}"),
            None => "-".to_owned(),
        };

        write!(f, "FileData({hash_str}: {size})",)
    }
}
