use std::{path::PathBuf, time::Instant};

use clap::Parser;

use rdupl::process_dir;

#[derive(Parser)]
#[command(version)]
struct Args {
    #[arg(default_value = ".")]
    path: PathBuf,
}

fn main() {
    let args = Args::parse();

    let t0 = Instant::now();

    let files = process_dir(args.path);

    let t1 = Instant::now();
    println!("{:.3}s", (t1 - t0).as_secs_f64());
    println!("files = {}", files.len());

    for (data, infos) in files.get_preview() {
        println!("\n{data}");
        for info in infos {
            println!("{info}");
        }
    }
}
