use std::{
    cell::RefCell,
    cmp::Reverse,
    collections::{HashMap, hash_map},
    fmt, fs, hash, io, iter, os,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};

use crate::utils::hash_file;

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct MetadataSnapshot {
    pub len: u64,
    pub accessed: Option<SystemTime>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub ino: Option<u64>,
}

fn get_ino(value: &fs::Metadata) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        return Some(os::unix::fs::MetadataExt::ino(value));
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = value;
        return None;
    }
}

impl From<fs::Metadata> for MetadataSnapshot {
    fn from(value: fs::Metadata) -> Self {
        Self {
            len: value.len(),
            accessed: value.accessed().ok(),
            modified: value.modified().ok(),
            created: value.created().ok(),
            ino: get_ino(&value),
        }
    }
}

/// A representation of a file in the filesystem.
///
/// It does **not** store the file contents.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FileInfo {
    /// Full path to the file.
    pub path: PathBuf,

    /// File metadata
    pub meta: MetadataSnapshot,
}

impl FileInfo {
    pub fn new(path: impl AsRef<Path>, meta: fs::Metadata) -> FileInfo {
        FileInfo {
            path: path.as_ref().to_owned(),
            meta: meta.into(),
        }
    }
}

impl PartialEq for FileInfo {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for FileInfo {}

impl hash::Hash for FileInfo {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state)
    }
}

impl fmt::Display for FileInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}

/// File identification data with LoD.
///
/// Used for comparing and grouping files by their actual content.
#[derive(Clone, Copy, Serialize, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash, Debug)]
pub struct FileData {
    /// File size in bytes.
    size: u64,

    /// Hash of file contents.
    hash: Option<u64>,
}

impl FileData {
    pub fn new(info: &FileInfo) -> io::Result<FileData> {
        Ok(FileData {
            size: info.meta.len,
            hash: None,
        })
    }

    pub fn with_hash(&self, info: &FileInfo) -> io::Result<FileData> {
        Ok(FileData {
            size: self.size,
            hash: Some(hash_file(&info.path)?),
        })
    }
}

impl fmt::Display for FileData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
        let mut unit = UNITS[4];
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

#[derive(Clone, Serialize, Deserialize, Debug)]
enum FileGroup {
    Uniq(FileInfo),
    Many(Vec<FileInfo>),
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct FileIndex {
    grouped_files: HashMap<FileData, FileGroup>,
}

impl FileIndex {
    pub fn load(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(postcard::from_bytes(&fs::read(path)?).expect("failed to load file index"))
    }

    pub fn dump(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let file = fs::File::create(path)?;
        let writer = io::BufWriter::new(file);
        postcard::to_io(&self, writer).expect("failed to dump file index");
        Ok(())
    }

    pub fn fast_add(&mut self, info: FileInfo, data: FileData) {
        match self.grouped_files.entry(data) {
            hash_map::Entry::Occupied(mut entry) => {
                let prev_info = entry.get_mut();
                match prev_info {
                    FileGroup::Uniq(file_info) => {
                        *prev_info = FileGroup::Many(vec![file_info.clone(), info]);
                    }
                    FileGroup::Many(file_group) => file_group.push(info),
                }
            }
            hash_map::Entry::Vacant(entry) => {
                entry.insert(FileGroup::Uniq(info));
            }
        }
    }

    pub fn len(&self) -> usize {
        self.grouped_files.len()
    }

    pub fn remove_ambiguous(&mut self) -> Vec<(FileInfo, FileData)> {
        self.grouped_files
            .iter_mut()
            .filter_map(|entry| match entry.1 {
                FileGroup::Uniq(_) => None,
                FileGroup::Many(file_group) => {
                    Some(file_group.drain(..).zip(iter::repeat(*entry.0)))
                }
            })
            .flatten()
            .collect()
    }

    pub fn get_preview(&self) -> Vec<(&FileData, &Vec<FileInfo>)> {
        let mut sorted_files: Vec<_> = self
            .grouped_files
            .iter()
            .filter_map(|entry| match entry {
                (data, FileGroup::Many(file_group)) if file_group.len() > 1 => {
                    Some((data, file_group))
                }
                _ => None,
            })
            .collect();
        sorted_files.sort_by_key(|k| Reverse((k.0.size * k.1.len() as u64, k.0.hash)));
        sorted_files
    }
}

#[derive(Debug)]
pub enum NodeContent {
    File { info: FileInfo, data: FileData },
    Dir { nodes: Vec<Node> },
    Other { text: String },
}

#[derive(Debug)]
pub struct TreeNode {
    name: String,
    parent: Option<Weak<RefCell<TreeNode>>>,
    content: NodeContent,
}

impl TreeNode {
    fn get_path(&self) -> PathBuf {
        match self.parent {
            Some(ref parent) => parent
                .upgrade()
                .expect("tree node parent was dropped")
                .borrow()
                .get_path(),
            None => PathBuf::new(),
        }
        .join(&self.name)
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    node_ref: Rc<RefCell<TreeNode>>,
}

impl Node {
    pub fn new(
        name: String,
        parent: Option<Weak<RefCell<TreeNode>>>,
        content: NodeContent,
    ) -> Self {
        Node {
            node_ref: Rc::new(RefCell::new(TreeNode {
                name,
                parent,
                content,
            })),
        }
    }

    pub fn new_root(name: String, content: NodeContent) -> Self {
        Self::new(name, None, content)
    }

    pub fn add_child(&self, name: String, content: NodeContent) -> Self {
        let NodeContent::Dir { ref mut nodes } = self.node_ref.borrow_mut().content else {
            panic!(
                "failed to add child: {} is not a dir",
                self.node_ref.borrow().name
            )
        };

        let node = Node::new(name, Some(Rc::downgrade(&self.node_ref)), content);

        nodes.push(node.clone());

        node
    }

    pub fn is_dir(&self) -> bool {
        match self.node_ref.borrow().content {
            NodeContent::Dir { nodes: _ } => true,
            _ => false,
        }
    }

    pub fn get_path(&self) -> PathBuf {
        self.node_ref.borrow().get_path()
    }
}

impl fmt::Display for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn fmt_node(
            node: &Node,
            f: &mut fmt::Formatter<'_>,
            prefix: Option<&str>,
            is_last: bool,
        ) -> fmt::Result {
            let tree_node = node.node_ref.borrow();

            let new_prefix = if let Some(prefix) = prefix {
                write!(f, "{prefix}")?;
                if is_last {
                    write!(f, "└── ")?;
                } else {
                    write!(f, "├── ")?;
                }
                if is_last {
                    format!("{prefix}    ")
                } else {
                    format!("{prefix}│   ")
                }
            } else {
                format!("")
            };

            match &tree_node.content {
                NodeContent::File { info: _, data } => {
                    writeln!(f, "{}: {}", tree_node.name, data)?;
                }
                NodeContent::Dir { nodes } => {
                    writeln!(f, "{}", tree_node.name)?;
                    let last = nodes.len().saturating_sub(1);
                    for (i, child) in nodes.iter().enumerate() {
                        fmt_node(child, f, Some(&new_prefix), i == last)?;
                    }
                }
                NodeContent::Other { text } => writeln!(f, "{} ({})", tree_node.name, text)?,
            }
            Ok(())
        }

        fmt_node(self, f, None, false)
    }
}
