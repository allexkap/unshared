use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use clap::Parser;
use color_eyre::Result;
use indicatif::{ProgressBar, ProgressStyle};

use crate::{
    app::App,
    fs_tree::{FsTree, FsTreeConfig},
};

mod app;
mod fs_tree;
mod utils;

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

    let config = FsTreeConfig {
        force_hash_size: None,
    };
    let mut fs_tree = FsTree::new(config);

    let pb = ProgressBar::new(0).with_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg}\n[{elapsed_precise}] [{bar:40.cyan/blue}] {percent:>2}% ({pos}/{len})",
        )
        .unwrap()
        .progress_chars("##-"),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    let t0 = Instant::now();
    fs_tree.add_root(path, pb);
    let t1 = Instant::now();

    println!("{:.3}s", (t1 - t0).as_secs_f64());
    println!("nodes = {}", fs_tree.len());

    let terminal = ratatui::init();
    let result = App::new(fs_tree).run(terminal);
    ratatui::restore();
    result
}
