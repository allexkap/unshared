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
