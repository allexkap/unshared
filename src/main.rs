use std::{path::PathBuf, time::Instant};

use clap::Parser;
use color_eyre::Result;

use crate::{
    app::App,
    file_index::{FileTree, FileTreeConfig},
};

mod app;
mod file_index;

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(short, long)]
    cache: Option<PathBuf>,

    #[arg(default_value = ".")]
    path: PathBuf,
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let args = Args::parse();

    let path = args.path.canonicalize()?;

    let config = FileTreeConfig {
        force_hash_size: None,
    };
    let mut file_tree = FileTree::new(config);

    let t0 = Instant::now();
    file_tree.add_root(path);
    let t1 = Instant::now();

    println!("{:.3}s", (t1 - t0).as_secs_f64());
    println!("nodes = {}", file_tree.len());

    let terminal = ratatui::init();
    let result = App::new(file_tree).run(terminal);
    ratatui::restore();
    result
}
