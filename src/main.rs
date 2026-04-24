use std::{fs, path::PathBuf, time::Duration};

use clap::Parser;
use color_eyre::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;

use crate::{
    app::App,
    fs_tree::{FsTree, FsTreeConfig},
};

mod app;
mod fs_tree;
mod utils;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Path to the cache file with hashes.
    #[arg(short, long, value_name = "FILE")]
    cache: Option<PathBuf>,

    /// Use cache data only and skip the filesystem scan.
    #[arg(short, long, requires = "cache")]
    fast: bool,

    /// Do not write any changes back to the cache file.
    #[arg(short, long, requires = "cache")]
    readonly: bool,

    /// Root directory to scan.
    #[arg(value_name = "DIR", default_value = ".")]
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

    let fs_tree_cache = match &args.cache {
        Some(cache) if args.fast || cache.exists() => {
            serde_json::from_str(&fs::read_to_string(cache)?)?
        }
        _ => None,
    };

    let fs_tree = if args.fast {
        fs_tree_cache.unwrap()
    } else {
        let fs_tree_config = FsTreeConfig {
            force_hash_size: None,
            cache_tree: fs_tree_cache.map(Box::new),
        };
        let mut fs_tree = FsTree::new(fs_tree_config);

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

        let path = args.path.canonicalize()?;

        fs_tree.add_root(path, pb)?;

        if let Some(cache) = args.cache
            && !args.readonly
        {
            fs::write(cache, serde_json::to_string(&fs_tree)?)?;
        }

        fs_tree
    };

    let terminal = ratatui::init();
    let result = App::new(fs_tree).run(terminal);
    ratatui::restore();
    result
}
