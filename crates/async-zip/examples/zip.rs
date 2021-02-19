use std::env;

use async_zip::Zipper;
use futures::StreamExt;
use tokio::{fs::File, io::AsyncWriteExt};

#[tokio::main]
async fn main() {
    let dir = env::args().nth(1).expect("missing directory");
    let zip_file = env::args().nth(2).expect("missing directory");

    println!("zipping dir {} to file {}", dir, zip_file);

    let z = Zipper::from_directory(dir)
        .await
        .expect("cannot list directory");
    let mut chunks = z.zipped_stream();
    let mut f = File::create(zip_file)
        .await
        .expect("cannot create zip file");

    while let Some(chunk) = chunks.next().await {
        f.write_all(&chunk.expect("invalid zip read"))
            .await
            .expect("cannot write to file")
    }
}
