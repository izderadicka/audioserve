use std::path::PathBuf;

use clap::{Parser, Subcommand};
use collection::{common::CollectionTrait, CollectionOptions};

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
    List { prefix: Option<String> },
    Search { query: String },
}

macro_rules! exit {
    ($msg:literal, $($arg:expr),*) => {
        eprintln!($msg, $($arg),*);
        std::process::exit(1);
    };
}

pub fn main() -> anyhow::Result<()> {
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
        Commands::List { prefix } => {
            println!("Listing collection");
            for folder in col.list_keys() {
                println!("{}", folder);
            }
        }
        Commands::Search { query } => {
            println!("Searching collection for {}", query);
            let res = col.search(query, None);
            for folder in res {
                println!("{:?}", folder.path);
            }
        }
    }

    Ok(())
}
