use clap::{Parser, Subcommand};
use collection::{common::CollectionTrait, CollectionOptions};
use std::path::PathBuf;

fn default_db() -> String {
    let home = std::env::var("HOME").expect("Cannot get HOME for default db-path arg");
    return home + "/.audioserve/col_db";
}

#[derive(Parser)]
struct Args {
    #[arg(short, long, value_name = "PATH")]
    collection: PathBuf,
    #[arg(long, default_value_t=default_db())]
    db_path: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    List {
        #[arg(short, long)]
        prefix: Option<String>,
    },
    Search {
        query: String,
    },
    Get {
        path: String,
    },
}

macro_rules! exit {
    ($msg:literal, $($arg:expr),*) => {
        eprintln!($msg, $($arg),*);
        std::process::exit(1);
    };
}
#[cfg(unix)]
fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_sigpipe() {
    // no-op
}
pub fn main() -> anyhow::Result<()> {
    // restore C like sigpipe handling
    reset_sigpipe();
    env_logger::init();

    let args = Args::parse();

    if !args.collection.is_dir() || !args.collection.exists() {
        exit!("Collection directory {:?} does not exists", args.collection);
    }

    let mut col_opts = CollectionOptions::default();
    col_opts.read_only = true;
    let col = collection::cache::CollectionCache::new(args.collection, args.db_path, col_opts)
        .expect("Cannot open collection");

    match args.command {
        Commands::List { ref prefix } => {
            for folder in col.list_keys() {
                let can_output = prefix
                    .as_ref()
                    .map(|p| folder.find(p).map(|i| i == 0).unwrap_or(false))
                    .unwrap_or(true);
                if can_output {
                    println!("{}", folder);
                }
            }
        }
        Commands::Search { query } => {
            let res = col.search(query, None);
            for folder in res {
                println!("{}", folder.path.to_str().unwrap_or("<NOT_UTF8>"));
            }
        }
        Commands::Get { path } => {
            if let Some(f) = col.get(path) {
                println!("{:?}", f);
            }
        }
    }

    Ok(())
}
