use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use biliup::{client::Client, video::Studio};
use rss::Channel;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    time::sleep,
};
use tracing_subscriber::{
    fmt, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter,
};
use youtube_dl::YoutubeDl;

#[tokio::main]
async fn main() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    if let Err(e) = tokio::select! { v = get_and_upload(rx) => v, v = feed(tx) => v} {
        tracing::error!("{e}");
        return;
    }
}

#[derive(Clone)]
struct Video {
    title: String,
    link: String,
}

async fn get_and_upload(mut rx: UnboundedReceiver<Video>) -> Result<()> {
    loop {
        if let Some(v) = rx.recv().await {
            tokio::fs::create_dir_all("./video").await?;
            let output = YoutubeDl::new(&v.link)
                .output_directory("./video")
                .extra_arg("-o downloaded")
                .socket_timeout("15")
                .download(true)
                .run_async()
                .await?;

            let video = output.into_single_video();
            let title = video.map(|x| x.title);
            tracing::info!("Downloaded: {:?}", title);

            let filename = tokio::fs::read_dir("./video")
                .await?
                .next_entry()
                .await?
                .map(|x| x.file_name())
                .context("Can not get filename")?;

            let filename = filename
                .to_str()
                .map(|x| x.to_string())
                .context("Cab not get filename")?;

            Studio::builder()
                .desc(format!("原视频：{}", &v.link))
                .source(v.link)
                .copyright(2)
                .tag("粤菜,茶餐厅".to_string())
                .tid(76)
                .title(v.title)
                .videos(vec![biliup::video::Video::new(&filename)])
                .build()
                .submit(
                    &Client::new()
                        .login_by_cookies(std::fs::File::open("./cookie.json")?)
                        .await?,
                )
                .await?;

            tracing::info!("Uploaded!");
            tokio::fs::remove_file(format!("./video/{}", filename)).await?;
            sleep(Duration::from_secs(300)).await
        }
    }
}

async fn feed(tx: UnboundedSender<Video>) -> Result<()> {
    let mut video: Option<Video> = None;

    loop {
        tracing::info!("Getting");
        let content = reqwest::get("https://rsshub.app/youtube/user/@singsingkitchen")
            .await?
            .bytes()
            .await?;

        let channel = Channel::read_from(&content[..])?;
        let newest = channel.items().get(0);
        if let Some(newest) = newest {
            if newest.title() != video.as_ref().map(|x| x.title.as_str()) {
                video = Some(Video {
                    title: newest.title.clone().unwrap(),
                    link: newest.link.clone().unwrap(),
                });
                tracing::info!("New video: {:?}", newest.title);
                tx.send(video.clone().unwrap())
                    .map_err(|e| anyhow!("{e}"))?;
            }
        }
        sleep(Duration::from_millis(300)).await;
    }
}
