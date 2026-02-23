use std::{path::PathBuf, time::Instant};

use clap::Parser;
use rdupl::{FileTree, FileTreeConfig};

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(short, long)]
    cache: Option<PathBuf>,

    #[arg(default_value = ".")]
    path: PathBuf,
}

fn main() {
    let args = Args::parse();

    let mut file_tree = FileTree::new(FileTreeConfig {
        force_hash_size: None,
    });

    let t0 = Instant::now();

    file_tree.add_root(args.path);

    let t1 = Instant::now();
    println!("{:.3}s", (t1 - t0).as_secs_f64());
    println!("nodes = {}", file_tree.len());

    for (data, paths) in file_tree.get_preview() {
        println!("\n{data}");
        for path in paths {
            println!("{}", path.display());
        }
    }
}
