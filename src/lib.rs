use std::{io, path::Path};

use tqdm::Iter;

use crate::file_index::{FileData, FileIndex, FileInfo, Node, NodeContent};

mod file_index;
mod utils;

fn process_entry(entry: &walkdir::DirEntry) -> io::Result<(FileInfo, FileData)> {
    let info = FileInfo::new(entry.path(), entry.metadata()?);
    let data = FileData::new(&info)?;
    return Ok((info, data));
}

fn skip_file(path: impl AsRef<Path>, err: &std::io::Error) {
    println!("{}: {}", path.as_ref().display(), err);
}

pub fn process(path: impl AsRef<Path>) -> (FileIndex, Option<Node>) {
    let entries = walkdir::WalkDir::new(path)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|res| match res {
            Ok(v) => Some(v),
            Err(e) => {
                println!("{e}");
                None
            }
        });

    let mut file_index = FileIndex::default();
    let mut dir_stack: Vec<Node> = Vec::new();
    for entry in entries {
        let content = if entry.file_type().is_dir() {
            NodeContent::Dir { nodes: vec![] }
        } else {
            match process_entry(&entry) {
                Ok((info, data)) => {
                    file_index.fast_add(info.clone(), data);
                    NodeContent::File { info, data }
                }
                Err(err) => {
                    skip_file(entry.path(), &err);
                    NodeContent::Other {
                        text: err.to_string(),
                    }
                }
            }
        };

        let name = match entry.file_name().to_str() {
            Some(s) => s.to_owned(),
            None => {
                println!("Invalid UTF-8 filename: {:?}", entry.file_name());
                continue;
            }
        };

        let _ = dir_stack.drain(entry.depth()..);

        let node = match dir_stack.last() {
            Some(parent) => parent.add_child(name, content),
            None => Node::new_root(name, content),
        };

        if node.is_dir() {
            dir_stack.push(node);
        }
    }

    let ambiguous_files = file_index.remove_ambiguous();

    for (info, data) in ambiguous_files.into_iter().tqdm() {
        match data.with_hash(&info) {
            Ok(data) => file_index.fast_add(info, data),
            Err(err) => skip_file(info.path, &err),
        };
    }

    (file_index, dir_stack.into_iter().next())
}
