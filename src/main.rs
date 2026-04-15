use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use clap::Parser;
use color_eyre::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::info;

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

    let logger_env = env_logger::Env::default().default_filter_or("warn");
    let logger = env_logger::Builder::from_env(logger_env)
        .format_target(false)
        .format_timestamp(None)
        .format_indent(Some("[LEVEL] ".len()))
        .build();
    let level = logger.filter();
    let mpb = MultiProgress::new();
    LogWrapper::new(mpb.clone(), logger).try_init()?;
    log::set_max_level(level);

    let args = Args::parse();

    let path = args.path.canonicalize()?;

    let config = FsTreeConfig {
        force_hash_size: None,
    };
    let mut fs_tree = FsTree::new(config);

    let pb: ProgressBar = mpb.add(
        ProgressBar::new(0).with_style(
            ProgressStyle::with_template(concat!(
                "{spinner:.green} {msg}\n",
                "[{elapsed_precise}] [{bar:40.cyan/blue}] {percent:>2}% ({pos}/{len})"
            ))?
            .progress_chars("##-"),
        ),
    );
    pb.enable_steady_tick(Duration::from_millis(100));

    let t0 = Instant::now();
    fs_tree.add_root(path, pb).unwrap();
    let t1 = Instant::now();

    info!("dt = {:.3}s", (t1 - t0).as_secs_f64());
    info!("nodes = {}", fs_tree.len());

    let terminal = ratatui::init();
    let result = App::new(fs_tree).run(terminal);
    ratatui::restore();
    result
}
