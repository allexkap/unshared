use std::{
    cmp::Reverse,
    collections::HashMap,
    fmt,
    fs::File,
    hash::Hasher,
    io::{self, Read},
    mem,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    time::Instant,
    u64,
};

use clap::Parser;
use seahash::SeaHasher;
use walkdir::{DirEntryExt, WalkDir};

mod fiemap;

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Clone, Debug)]
struct FileInfo {
    path: PathBuf,
    meta: std::fs::Metadata,
}

impl FileInfo {
    fn new<T: AsRef<Path>>(path: &T, meta: std::fs::Metadata) -> FileInfo {
        FileInfo {
            path: path.as_ref().to_owned(),
            meta,
        }
    }
}

impl fmt::Display for FileInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path.display())
    }
}

#[derive(PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy, Debug)]
struct FileData {
    size: u64,
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

#[derive(Default)]
struct Files {
    all_files: HashMap<FileData, Vec<FileInfo>>,
}

impl Files {
    fn add(&mut self, info: FileInfo, data: FileData) {
        let Some(same_files) = self.all_files.get_mut(&data) else {
            self.all_files.insert(data, vec![info]);
            return;
        };

        if data.hash == None {
            if same_files.len() > 0 {
                for inner_info in mem::take(same_files) {
                    let inner_data = data.with_hash(&inner_info).unwrap();
                    self.add(inner_info, inner_data);
                }
            }
            let data = data.with_hash(&info).unwrap();
            self.add(info, data);
        } else {
            same_files.push(info);
        }
    }
}

fn hash_file<T: AsRef<Path>>(path: &T) -> io::Result<u64> {
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
    let ino = entry.ino();
    let fe_physical = fiemap::read_fiemap(entry.path(), Some(1))?
        .1
        .get(0)
        .map_or(0, |e| e.fe_physical);

    println!(
        "{:offset$}{} {} {}",
        "",
        ino,
        fe_physical,
        entry.file_name().display(),
        offset = entry.depth() * 2
    );

    let info = FileInfo::new(&entry.path(), entry.metadata()?);
    let data = FileData::new(&info)?;
    return Ok((info, data));
}

fn main() {
    let args = Args::parse();

    let t0 = Instant::now();

    let mut files = Files::default();
    for entry in WalkDir::new(args.path).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }

        match process_entry(&entry) {
            Ok((info, data)) => files.add(info, data),
            Err(err) => println!("{}: {}", entry.path().display(), err),
        }
    }

    let t1 = Instant::now();
    println!("{:.3}s", (t1 - t0).as_secs_f64());
    println!("files = {}", files.all_files.len());

    let mut sorted_files: Vec<(FileData, Vec<FileInfo>)> =
        files.all_files.drain().filter(|k| k.1.len() > 0).collect();
    sorted_files.sort_by_key(|k| Reverse((k.0.size * k.1.len() as u64, k.0.hash)));

    for (data, infos) in sorted_files.into_iter() {
        println!("\n{data}");
        for info in infos {
            println!("{info}");
        }
    }
}
