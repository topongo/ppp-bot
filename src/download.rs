use std::{fs::create_dir_all, sync::Arc};
use std::path::PathBuf;

use power_pizza_bot::spreaker::{SimpleEpisode, SpreakerDownloader, SpreakerResponse};
use reqwest::Client;
use tokio_stream::StreamExt;
use lazy_static::lazy_static;

lazy_static! {
    static ref OUTPUT_DIR: PathBuf = PathBuf::from("output");
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + 'static>> {
    pretty_env_logger::init();
    let cli = Arc::new(Client::new());

    let mut it = SpreakerResponse::<SimpleEpisode>::request(
        "https://api.spreaker.com/v2/shows/3039391/episodes".to_owned(),
        cli.clone(),
    );

    if !OUTPUT_DIR.exists() {
        create_dir_all(OUTPUT_DIR.clone()).unwrap()
    }

    let downloader = SpreakerDownloader::new(cli, 4, OUTPUT_DIR.clone());
    while let Some(e) = it.next().await {
        downloader.download(e);
    }
    downloader.join().await.unwrap();

    Ok(())
}
