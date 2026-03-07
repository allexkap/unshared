//! Utility helpers.

use std::{
    fs,
    hash::Hasher,
    io::{self, Read},
    path::Path,
};

use seahash::SeaHasher;

pub fn hash_file(path: impl AsRef<Path>) -> io::Result<u64> {
    let mut file = fs::File::open(path)?;
    let mut buf = [0; 4096];
    let mut hasher = SeaHasher::new();
    loop {
        match file.read(&mut buf)? {
            0 => return Ok(hasher.finish()),
            n => hasher.write(&buf[..n]),
        }
    }
}

pub fn bytes_to_string(value: u64) -> String {
    const UNITS: [&str; 7] = ["", "k", "M", "G", "T", "P", "E"];

    let mut size = value as f64;
    let mut unit_idx = 0;

    while size >= 1000.0 && unit_idx < UNITS.len() - 1 {
        size /= 1000.0;
        unit_idx += 1;
    }

    let precision = if size >= 10.0 || unit_idx == 0 { 0 } else { 1 };
    format!("{size:.precision$}{}", UNITS[unit_idx])
}
