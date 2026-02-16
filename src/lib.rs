use std::{
    cmp::Reverse,
    collections::{HashMap, hash_map},
    fmt,
    hash::Hasher,
    io::{self, Read},
    path::{Path, PathBuf},
};

use seahash::SeaHasher;
use walkdir::WalkDir;

mod fiemap;

/// A representation of a file in the filesystem.
///
/// It does **not** store the file contents.
#[derive(Clone, Debug)]
pub struct FileInfo {
    /// Full path to the file.
    path: PathBuf,

    /// File metadata
    meta: std::fs::Metadata,

    /// Physical offset of the first extent.
    fe_physical: Option<u64>,
}

impl FileInfo {
    fn new(path: impl AsRef<Path>, meta: std::fs::Metadata, fe_physical: Option<u64>) -> FileInfo {
        FileInfo {
            path: path.as_ref().to_owned(),
            meta,
            fe_physical,
        }
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
#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub struct FileData {
    /// File size in bytes.
    size: u64,

    /// Hash of file contents.
    hash: Option<u64>,
}

impl FileData {
    fn new(info: &FileInfo) -> io::Result<FileData> {
        Ok(FileData {
            size: info.meta.len(),
            hash: None,
        })
    }

    fn with_hash(&self, info: &FileInfo) -> io::Result<FileData> {
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

enum FileGroup {
    Uniq(FileInfo),
    Many(Vec<FileInfo>),
}

#[derive(Default)]
pub struct Files {
    inner: HashMap<FileData, FileGroup>,
}

impl Files {
    pub fn fast_add(&mut self, info: FileInfo, data: FileData) {
        match self.inner.entry(data) {
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
        self.inner.len()
    }

    pub fn remove_ambiguous(&mut self) -> Vec<(FileInfo, FileData)> {
        self.inner
            .iter_mut()
            .filter_map(|entry| match entry.1 {
                FileGroup::Uniq(_) => None,
                FileGroup::Many(file_group) => {
                    Some(file_group.drain(..).zip(std::iter::repeat(*entry.0)))
                }
            })
            .flatten()
            .collect()
    }

    pub fn get_preview(&self) -> Vec<(&FileData, &Vec<FileInfo>)> {
        let mut sorted_files: Vec<_> = self
            .inner
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

fn hash_file(path: impl AsRef<Path>) -> io::Result<u64> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0; 4096];
    let mut hasher = SeaHasher::new();
    loop {
        match file.read(&mut buf)? {
            0 => return Ok(hasher.finish()),
            n => hasher.write(&buf[..n]),
        }
    }
}

fn process_entry(entry: &walkdir::DirEntry) -> io::Result<(FileInfo, FileData)> {
    let fe_physical = fiemap::read_fiemap(entry.path(), Some(1))?
        .1
        .get(0)
        .map(|f| f.fe_physical);

    let info = FileInfo::new(entry.path(), entry.metadata()?, fe_physical);
    let data = FileData::new(&info)?;
    return Ok((info, data));
}

fn skip_file(path: impl AsRef<Path>, err: std::io::Error) {
    println!("{}: {}", path.as_ref().display(), err);
}

pub fn process_dir(path: impl AsRef<Path>) -> Files {
    let mut files = Files::default();

    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }

        match process_entry(&entry) {
            Ok((info, data)) => files.fast_add(info, data),
            Err(err) => skip_file(entry.path(), err),
        }
    }

    let mut ambiguous_files = files.remove_ambiguous();
    ambiguous_files.sort_by_key(|k| k.0.fe_physical);

    for (info, data) in ambiguous_files.into_iter() {
        match data.with_hash(&info) {
            Ok(data) => files.fast_add(info, data),
            Err(err) => skip_file(info.path, err),
        };
    }

    files
}
