//! Utility helpers.

use std::{
    fs,
    io::{Read, Result},
    path::Path,
};

use xxhash_rust::xxh3::Xxh3;

pub fn hash_file(path: impl AsRef<Path>) -> Result<u128> {
    let mut file = fs::File::open(path)?;
    let mut buf = [0; 1 << 20];
    let mut hasher = Xxh3::new();
    loop {
        match file.read(&mut buf)? {
            0 => return Ok(hasher.digest128()),
            n => hasher.update(&buf[..n]),
        }
    }
}

pub fn use_si_postfix(value: u64) -> String {
    let shift = value.checked_ilog10().unwrap_or_default() / 3;
    let shifted_value = (value as f64) / (10_u64.pow(shift * 3) as f64);
    format!(
        "{:.p$}{}",
        shifted_value,
        ["", "K", "M", "G", "T", "P", "E"][shift as usize],
        p = (shifted_value < 10.0 && shift != 0) as usize
    )
}
