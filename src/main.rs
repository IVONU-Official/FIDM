mod db;
mod downloader;
mod model;
mod ui;

use std::sync::Arc;

use anyhow::Context;
use eframe::egui;
use tokio::runtime::Runtime;

fn main() -> anyhow::Result<()> {
    let data_dir = dirs_fallback();
    std::fs::create_dir_all(&data_dir)?;
    let database = Arc::new(db::Database::open(data_dir.join("fidm.sqlite3"))?);
    let runtime = Arc::new(Runtime::new().context("create Tokio runtime")?);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("FIDM — Free Internet Download Manager"),
        ..Default::default()
    };

    eframe::run_native(
        "FIDM",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::FidmApp::new(
                cc,
                database.clone(),
                runtime.clone(),
            )))
        }),
    )
    .map_err(|err| anyhow::anyhow!(err.to_string()))
}

fn dirs_fallback() -> std::path::PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
        .unwrap_or_else(std::env::temp_dir)
        .join("fidm")
}
