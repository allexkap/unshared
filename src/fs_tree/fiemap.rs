use std::{fs::File, os::fd::AsRawFd, path::Path, u32};

use nix::ioctl_readwrite;

ioctl_readwrite!(fiemap_ioctl, b'f', 11, Fiemap);

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct FiemapExtent {
    pub fe_logical: u64,
    pub fe_physical: u64,
    pub fe_length: u64,
    fe_reserved64: [u64; 2],
    pub fe_flags: u32,
    fe_reserved: [u32; 3],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Fiemap {
    pub fm_start: u64,
    pub fm_length: u64,
    pub fm_flags: u32,
    pub fm_mapped_extents: u32,
    pub fm_extent_count: u32,
    fm_reserved: u32,
}

pub fn read_fiemap<T: AsRef<Path>>(
    path: T,
    max_extent_count: Option<u32>,
) -> std::io::Result<(Fiemap, Box<[FiemapExtent]>)> {
    let path = path.as_ref();

    let file = File::open(path)?;
    let fd = file.as_raw_fd();

    let mut fiemap = Fiemap {
        fm_start: 0,
        fm_length: u64::MAX,
        fm_flags: 0,
        fm_mapped_extents: 0,
        fm_extent_count: max_extent_count.unwrap_or(0),
        fm_reserved: 0,
    };

    if fiemap.fm_extent_count == 0 {
        unsafe { fiemap_ioctl(fd, &mut fiemap) }?;

        if !max_extent_count.is_none() || fiemap.fm_mapped_extents == 0 {
            return Ok((fiemap, Box::default()));
        }

        fiemap.fm_extent_count = fiemap.fm_mapped_extents
    }

    let total_size =
        size_of::<Fiemap>() + size_of::<FiemapExtent>() * fiemap.fm_extent_count as usize;
    let mut buffer = vec![0u8; total_size].into_boxed_slice();

    let buffer_fiemap = unsafe { &mut *(buffer.as_mut_ptr() as *mut Fiemap) };
    *buffer_fiemap = fiemap;

    unsafe { fiemap_ioctl(fd, buffer_fiemap) }?;

    let buffer_extents = unsafe {
        let ptr = buffer.as_ptr().add(size_of::<Fiemap>()) as *const FiemapExtent;

        std::slice::from_raw_parts(ptr, fiemap.fm_extent_count as usize)
    };

    return Ok((*buffer_fiemap, buffer_extents.to_vec().into_boxed_slice()));
}
