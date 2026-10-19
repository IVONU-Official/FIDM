use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use futures_util::StreamExt;
use reqwest::{
    Client,
    header::{ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, RANGE},
};
use tokio::{
    fs::{self, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::{Mutex, mpsc, watch},
};

use crate::{
    db::Database,
    model::{Download, DownloadStatus},
};

#[derive(Debug, Clone)]
pub enum Event {
    Progress {
        id: i64,
        downloaded: u64,
        size: Option<u64>,
        speed: u64,
    },
    State {
        id: i64,
        status: DownloadStatus,
        error: Option<String>,
    },
}

#[derive(Clone)]
pub struct Control {
    pause: watch::Sender<bool>,
    stop: watch::Sender<bool>,
}
impl Control {
    pub fn new() -> Self {
        let (pause, _) = watch::channel(false);
        let (stop, _) = watch::channel(false);
        Self { pause, stop }
    }
    pub fn pause(&self, paused: bool) {
        self.pause.send_replace(paused);
    }
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
}

pub async fn inspect(url: &str) -> anyhow::Result<(String, Option<u64>, bool)> {
    let client = Client::builder().timeout(Duration::from_secs(20)).build()?;
    let response = client.head(url).send().await?;
    let size = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse().ok());
    let ranges = response
        .headers()
        .get(ACCEPT_RANGES)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
    let name = response
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|h| h.to_str().ok())
        .and_then(filename_from_disposition)
        .or_else(|| {
            url::Url::parse(url)
                .ok()
                .and_then(|u| u.path_segments()?.next_back().map(str::to_owned))
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "download.bin".into());
    Ok((name, size, ranges))
}

fn filename_from_disposition(value: &str) -> Option<String> {
    value
        .split(';')
        .find_map(|part| part.trim().strip_prefix("filename="))
        .map(|value| value.trim_matches('"').to_owned())
}

pub fn start(
    handle: tokio::runtime::Handle,
    download: Download,
    controls: Control,
    db: Arc<Database>,
    events: mpsc::UnboundedSender<Event>,
) {
    handle.spawn(async move {
        let id = download.id;
        let stop_workers = controls.clone();
        let result = run(download, controls, db.clone(), events.clone()).await;
        if let Err(err) = result {
            stop_workers.stop();
            let message = format!("{err:#}");
            let _ = db.set_status(id, DownloadStatus::Error, Some(&message));
            let _ = events.send(Event::State {
                id,
                status: DownloadStatus::Error,
                error: Some(message),
            });
        }
    });
}

async fn run(
    download: Download,
    control: Control,
    db: Arc<Database>,
    events: mpsc::UnboundedSender<Event>,
) -> anyhow::Result<()> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .user_agent("FIDM/0.1")
        .build()
        .context("create HTTP client")?;
    let mut pause = control.pause.subscribe();
    let mut stop = control.stop.subscribe();
    let path = PathBuf::from(&download.path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    while *pause.borrow() {
        db.update(
            download.id,
            DownloadStatus::Paused,
            download.downloaded,
            download.size,
            None,
        )?;
        let _ = events.send(Event::State {
            id: download.id,
            status: DownloadStatus::Paused,
            error: None,
        });
        tokio::select! { _ = pause.changed() => {}, _ = stop.changed() => return Err(anyhow!("Download stopped")) }
    }
    if *stop.borrow() {
        return Err(anyhow!("Download stopped"));
    }
    let probe = client
        .get(&download.url)
        .header(RANGE, "bytes=0-0")
        .send()
        .await?;
    let supports_ranges = probe.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let total = probe
        .headers()
        .get("content-range")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.parse::<u64>().ok())
        .or(download.size);
    drop(probe);
    let _ = events.send(Event::State {
        id: download.id,
        status: DownloadStatus::Running,
        error: None,
    });
    db.update(download.id, DownloadStatus::Running, 0, total, None)?;

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(&path)
        .await
        .with_context(|| format!("open output file {}", path.display()))?;
    file.set_len(total.unwrap_or(0)).await?;
    let file = Arc::new(Mutex::new(file));
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<(u64, u64)>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<anyhow::Result<()>>();

    if supports_ranges && total.unwrap_or(0) > 1 {
        let total = total.unwrap();
        let chunks = 8u64.min(total).max(1);
        let chunk_size = total.div_ceil(chunks);
        for index in 0..chunks {
            let start = index * chunk_size;
            let end = (start + chunk_size - 1).min(total - 1);
            if start >= total {
                continue;
            }
            spawn_chunk(
                client.clone(),
                download.url.clone(),
                file.clone(),
                start,
                end,
                progress_tx.clone(),
                done_tx.clone(),
                control.clone(),
            );
        }
    } else {
        let response = client.get(&download.url).send().await?.error_for_status()?;
        spawn_stream(
            response,
            file.clone(),
            0,
            progress_tx.clone(),
            done_tx.clone(),
            control.clone(),
        );
    }
    drop(progress_tx);
    drop(done_tx);

    let mut done = 0usize;
    let expected = if supports_ranges {
        total.unwrap_or(0).min(8) as usize
    } else {
        1
    };
    let mut received = 0_u64;
    let mut speed_bytes = 0u64;
    let mut last_tick = tokio::time::Instant::now();
    loop {
        tokio::select! {
            Some((offset, amount)) = progress_rx.recv() => {
                received = received.saturating_add(amount);
                speed_bytes = speed_bytes.saturating_add(amount);
                let _ = offset;
                if last_tick.elapsed() >= Duration::from_millis(500) {
                    let speed = (speed_bytes as f64 / last_tick.elapsed().as_secs_f64()) as u64;
                    let _ = events.send(Event::Progress { id: download.id, downloaded: received, size: total, speed });
                    let _ = db.update(download.id, DownloadStatus::Running, received, total, None);
                    speed_bytes = 0; last_tick = tokio::time::Instant::now();
                }
            }
            Some(result) = done_rx.recv() => {
                done += 1;
                result?;
                if done >= expected { break; }
            }
            _ = stop.changed() => return Err(anyhow!("Download stopped")),
            _ = pause.changed() => {
                if *pause.borrow() {
                    db.update(download.id, DownloadStatus::Paused, received, total, None)?;
                    let _ = events.send(Event::State { id: download.id, status: DownloadStatus::Paused, error: None });
                } else {
                    db.update(download.id, DownloadStatus::Running, received, total, None)?;
                    let _ = events.send(Event::State { id: download.id, status: DownloadStatus::Running, error: None });
                }
            }
            else => break,
        }
    }
    if let Some(size) = total {
        if received < size {
            return Err(anyhow!(
                "Connection ended early ({received} of {size} bytes)"
            ));
        }
    }
    db.update(
        download.id,
        DownloadStatus::Finished,
        total.unwrap_or(received),
        total,
        None,
    )?;
    let _ = events.send(Event::Progress {
        id: download.id,
        downloaded: total.unwrap_or(received),
        size: total,
        speed: 0,
    });
    let _ = events.send(Event::State {
        id: download.id,
        status: DownloadStatus::Finished,
        error: None,
    });
    Ok(())
}

fn spawn_chunk(
    client: Client,
    url: String,
    file: Arc<Mutex<tokio::fs::File>>,
    start: u64,
    end: u64,
    progress: mpsc::UnboundedSender<(u64, u64)>,
    done: mpsc::UnboundedSender<anyhow::Result<()>>,
    control: Control,
) {
    tokio::spawn(async move {
        let result = async {
            let mut stop = control.stop.subscribe();
            let mut pause = control.pause.subscribe();
            let mut position = start;
            let mut retries = 0;
            while position <= end {
                if *stop.borrow() { return Err(anyhow!("Download stopped")); }
                while *pause.borrow() { tokio::select! { _ = pause.changed() => {}, _ = stop.changed() => return Err(anyhow!("Download stopped")) } }
                let response = client.get(&url).header(RANGE, format!("bytes={position}-{end}")).send().await;
                let response = match response {
                    Ok(response) if response.status() == reqwest::StatusCode::PARTIAL_CONTENT => response,
                    Ok(response) if response.status().is_success() => {
                        return Err(anyhow!("Server ignored the requested byte range"));
                    }
                    Ok(response) => { retries += 1; if retries > 5 { return Err(anyhow!("Server returned {}", response.status())); } tokio::time::sleep(Duration::from_millis(250 * 2u64.pow(retries))).await; continue; }
                    Err(err) => { retries += 1; if retries > 5 { return Err(err.into()); } tokio::time::sleep(Duration::from_millis(250 * 2u64.pow(retries))).await; continue; }
                };
                let mut stream = response.bytes_stream();
                while let Some(next) = stream.next().await {
                    if *stop.borrow() { return Err(anyhow!("Download stopped")); }
                    let bytes = next?;
                    let len = bytes.len() as u64;
                    { let mut file = file.lock().await; file.seek(std::io::SeekFrom::Start(position)).await?; file.write_all(&bytes).await?; }
                    position += len;
                    let _ = progress.send((position, len));
                }
                if position <= end { retries += 1; if retries > 5 { return Err(anyhow!("Incomplete range response")); } }
            }
            Ok(())
        }.await;
        let _ = done.send(result);
    });
}

fn spawn_stream(
    response: reqwest::Response,
    file: Arc<Mutex<tokio::fs::File>>,
    offset: u64,
    progress: mpsc::UnboundedSender<(u64, u64)>,
    done: mpsc::UnboundedSender<anyhow::Result<()>>,
    control: Control,
) {
    tokio::spawn(async move {
        let result = async {
            let mut stream = response.bytes_stream();
            let mut position = offset;
            let mut pause = control.pause.subscribe();
            let mut stop = control.stop.subscribe();
            while let Some(next) = stream.next().await {
                if *stop.borrow() {
                    return Err(anyhow!("Download stopped"));
                }
                while *pause.borrow() {
                    tokio::select! {
                        _ = pause.changed() => {},
                        _ = stop.changed() => return Err(anyhow!("Download stopped")),
                    }
                }
                let bytes = next?;
                let len = bytes.len() as u64;
                {
                    let mut file = file.lock().await;
                    file.seek(std::io::SeekFrom::Start(position)).await?;
                    file.write_all(&bytes).await?;
                }
                position += len;
                let _ = progress.send((position, len));
            }
            Ok(())
        }
        .await;
        let _ = done.send(result);
    });
}
