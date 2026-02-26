use std::{fmt, time::SystemTime};

use enum_as_inner::EnumAsInner;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileData {
    pub size: u64,
    pub hash: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct FileNode {
    pub modified: Option<SystemTime>,
    pub data: FileData,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct DirNode {
    pub total_size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct TestNode;

#[derive(Clone, Copy, Debug, EnumAsInner)]
pub enum NodeKind {
    File(FileNode),
    Dir(DirNode),
    Test(TestNode),
}

#[derive(Clone, Debug)]
pub struct FsTreeNode {
    pub name: String,
    pub kind: NodeKind,
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
