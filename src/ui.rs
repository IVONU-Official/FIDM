use std::{collections::HashMap, path::PathBuf, sync::Arc};

use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use tokio::{runtime::Runtime, sync::mpsc};

use crate::{
    db::Database,
    downloader::{self, Control, Event},
    model::{Download, DownloadStatus, category_for, format_bytes},
};

const CATEGORIES: [&str; 10] = [
    "All Downloads",
    "Unfinished",
    "Finished",
    "Compressed",
    "Documents",
    "Music",
    "Programs",
    "Video",
    "Queues",
    "Grabber projects",
];

pub struct FidmApp {
    db: Arc<Database>,
    runtime: Arc<Runtime>,
    items: Vec<Download>,
    filter: String,
    selected: Option<i64>,
    controls: HashMap<i64, Control>,
    event_tx: mpsc::UnboundedSender<Event>,
    event_rx: mpsc::UnboundedReceiver<Event>,
    add_open: bool,
    url: String,
    save_dir: String,
    category: String,
    add_error: Option<String>,
    speed: HashMap<i64, u64>,
    speed_history: Vec<f32>,
    status_message: String,
}

impl FidmApp {
    pub fn new(cc: &eframe::CreationContext<'_>, db: Arc<Database>, runtime: Arc<Runtime>) -> Self {
        // Keep the utility layout readable even when the desktop prefers a dark theme.
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let _ = db.recover_interrupted();
        let mut app = Self {
            db,
            runtime,
            items: Vec::new(),
            filter: "All Downloads".into(),
            selected: None,
            controls: HashMap::new(),
            event_tx,
            event_rx,
            add_open: false,
            url: String::new(),
            save_dir: default_download_dir(),
            category: "All Downloads".into(),
            add_error: None,
            speed: HashMap::new(),
            speed_history: vec![0.0; 64],
            status_message: "Ready".into(),
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        if let Ok(items) = self.db.list() {
            self.items = items;
        }
    }
    fn begin_download(&mut self, id: i64) {
        if self.controls.contains_key(&id) {
            if let Some(control) = self.controls.get(&id) {
                control.pause(false);
            }
            return;
        }
        let Some(item) = self.items.iter().find(|item| item.id == id).cloned() else {
            return;
        };
        let control = Control::new();
        self.controls.insert(id, control.clone());
        downloader::start(
            self.runtime.handle().clone(),
            item,
            control,
            self.db.clone(),
            self.event_tx.clone(),
        );
        self.status_message = "Download started".into();
    }
    fn toggle_pause(&mut self, id: i64) {
        if let Some(control) = self.controls.get(&id) {
            let paused = self
                .items
                .iter()
                .find(|item| item.id == id)
                .is_some_and(|item| item.status == DownloadStatus::Running);
            control.pause(paused);
        } else {
            self.begin_download(id);
        }
    }
    fn delete_selected(&mut self) {
        if let Some(id) = self.selected {
            if let Some(control) = self.controls.remove(&id) {
                control.stop();
            }
            let _ = self.db.delete(id);
            self.selected = None;
            self.refresh();
        }
    }
    fn add_download(&mut self, later: bool) {
        let parsed = match url::Url::parse(self.url.trim()) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => url,
            _ => {
                self.add_error = Some("Enter a valid HTTP or HTTPS URL.".into());
                return;
            }
        };
        let name = parsed
            .path_segments()
            .and_then(|mut parts| parts.next_back())
            .filter(|part| !part.is_empty())
            .unwrap_or("download.bin")
            .to_owned();
        let category = if self.category == "Auto detect" {
            category_for(&name).to_owned()
        } else {
            self.category.clone()
        };
        let path = PathBuf::from(&self.save_dir)
            .join(&name)
            .to_string_lossy()
            .into_owned();
        match self.db.add(parsed.as_str(), &name, &path, &category, None) {
            Ok(id) => {
                self.refresh();
                self.selected = Some(id);
                self.add_open = false;
                self.status_message = "Added to download queue".into();
                if !later {
                    self.begin_download(id);
                }
            }
            Err(err) => self.add_error = Some(err.to_string()),
        }
    }

    fn consume_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::Progress {
                    id,
                    downloaded,
                    size,
                    speed,
                } => {
                    self.speed.insert(id, speed);
                    let _ = self
                        .db
                        .update(id, DownloadStatus::Running, downloaded, size, None);
                    if self.selected == Some(id) {
                        self.speed_history.push(speed as f32);
                        if self.speed_history.len() > 64 {
                            self.speed_history.remove(0);
                        }
                    }
                }
                Event::State { id, status, error } => {
                    let _ = self.db.set_status(id, status, error.as_deref());
                    if matches!(status, DownloadStatus::Finished | DownloadStatus::Error) {
                        self.controls.remove(&id);
                    }
                    self.status_message =
                        error.unwrap_or_else(|| format!("{}: {}", id, status.as_str()));
                }
            }
        }
        self.refresh();
    }

    fn visible_items(&self) -> Vec<&Download> {
        self.items
            .iter()
            .filter(|item| match self.filter.as_str() {
                "Unfinished" => item.status != DownloadStatus::Finished,
                "Finished" => item.status == DownloadStatus::Finished,
                "All Downloads" | "Queues" | "Grabber projects" => true,
                category => item.category.eq_ignore_ascii_case(category),
            })
            .collect()
    }
}

impl eframe::App for FidmApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Color32::from_rgb(242, 245, 248).to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.consume_events();
        ctx.request_repaint_after(std::time::Duration::from_millis(350));
        egui::TopBottomPanel::top("menu")
            .exact_height(25.0)
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    for menu in ["File", "Edit", "Downloads", "View", "Configuration", "Help"] {
                        ui.menu_button(menu, |_ui| {});
                    }
                });
            });
        egui::TopBottomPanel::top("toolbar")
            .exact_height(76.0)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    toolbar_button(ui, "＋", "Add URL", "Add URL")
                        .clicked()
                        .then(|| self.add_open = true);
                    ui.separator();
                    let selected_status = self
                        .selected
                        .and_then(|id| self.items.iter().find(|i| i.id == id))
                        .map(|i| i.status);
                    if toolbar_button(ui, "↓", "Resume", "Resume selected download").clicked() {
                        if let Some(id) = self.selected {
                            self.begin_download(id);
                        }
                    }
                    if toolbar_button(ui, "Ⅱ", "Pause", "Pause selected download").clicked() {
                        if let Some(id) = self.selected {
                            self.toggle_pause(id);
                        }
                    }
                    if toolbar_button(ui, "■", "Stop", "Stop selected download").clicked() {
                        if let Some(id) = self.selected {
                            if let Some(c) = self.controls.remove(&id) {
                                c.stop();
                            }
                        }
                    }
                    if toolbar_button(ui, "■", "Stop All", "Stop all active downloads").clicked()
                    {
                        for c in self.controls.values() {
                            c.stop();
                        }
                        self.controls.clear();
                    }
                    if toolbar_button(ui, "▤", "Delete", "Delete selected row").clicked() {
                        self.delete_selected();
                    }
                    if toolbar_button(ui, "⚙", "Options", "Options").clicked() {
                        self.status_message = "Options will be available in a later release".into();
                    }
                    let _ = selected_status;
                });
            });
        egui::TopBottomPanel::bottom("status")
            .exact_height(27.0)
            .show(ctx, |ui| {
                let total_speed: u64 = self.speed.values().copied().sum();
                let active = self
                    .items
                    .iter()
                    .filter(|i| i.status == DownloadStatus::Running)
                    .count();
                let queued = self
                    .items
                    .iter()
                    .filter(|i| i.status == DownloadStatus::Queued)
                    .count();
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Total speed: {}/s",
                        format_bytes(Some(total_speed))
                    ));
                    ui.separator();
                    ui.label(format!("Active downloads: {active}"));
                    ui.separator();
                    ui.label(format!("Queued: {queued}"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(&self.status_message);
                    });
                });
            });
        egui::TopBottomPanel::bottom("details")
            .resizable(true)
            .default_height(170.0)
            .min_height(110.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_label(true, "Download details");
                    ui.selectable_label(false, "Speed limiter");
                });
                ui.separator();
                if let Some(item) = self.items.iter().find(|i| Some(i.id) == self.selected) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("URL: {}", item.url));
                        ui.separator();
                        ui.label(format!(
                            "Downloaded: {} / {}",
                            format_bytes(Some(item.downloaded)),
                            format_bytes(item.size)
                        ));
                        ui.separator();
                        ui.label(format!(
                            "Resume capability: {}",
                            if item.resumable() { "Yes" } else { "No" }
                        ));
                        ui.separator();
                        ui.label(format!(
                            "Connections: {}",
                            if item.status == DownloadStatus::Running {
                                "up to 8"
                            } else {
                                "—"
                            }
                        ));
                    });
                    let points = &self.speed_history;
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 70.0),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_filled(rect, 2.0, Color32::from_rgb(244, 248, 251));
                    if points.len() > 1 {
                        let max = points.iter().copied().fold(1.0_f32, f32::max);
                        let path: Vec<_> = points
                            .iter()
                            .enumerate()
                            .map(|(n, v)| {
                                egui::pos2(
                                    rect.left()
                                        + rect.width() * n as f32 / (points.len() - 1) as f32,
                                    rect.bottom() - rect.height() * (*v / max),
                                )
                            })
                            .collect();
                        ui.painter().add(egui::Shape::line(
                            path,
                            Stroke::new(1.5_f32, Color32::from_rgb(30, 125, 75)),
                        ));
                    }
                } else {
                    ui.label(
                        "Select a download to view transfer details and the live speed graph.",
                    );
                }
            });
        egui::SidePanel::left("categories")
            .resizable(true)
            .default_width(190.0)
            .width_range(145.0..=270.0)
            .show(ctx, |ui| {
                ui.heading("Categories");
                ui.separator();
                for category in CATEGORIES {
                    if ui
                        .selectable_label(self.filter == category, format!("📁  {category}"))
                        .clicked()
                    {
                        self.filter = category.into();
                    }
                }
                ui.separator();
                ui.label(
                    RichText::new("Custom queues and category folders are planned.")
                        .small()
                        .weak(),
                );
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(250, 251, 252)))
            .show(ctx, |ui| {
                ui.heading("FIDM — Free Internet Download Manager");
                ui.separator();
                let rows: Vec<Download> = self.visible_items().into_iter().cloned().collect();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("downloads_table")
                            .striped(true)
                            .min_col_width(55.0)
                            .show(ui, |ui| {
                                for header in [
                                    "File name",
                                    "Size",
                                    "Status",
                                    "Progress",
                                    "Speed",
                                    "Time left",
                                    "Last tried",
                                    "Resume",
                                ] {
                                    ui.strong(header);
                                }
                                ui.end_row();
                                for item in rows {
                                    let selected = self.selected == Some(item.id);
                                    if ui
                                        .selectable_label(selected, format!("📄 {}", item.name))
                                        .clicked()
                                    {
                                        self.selected = Some(item.id);
                                    }
                                    ui.label(format_bytes(item.size));
                                    ui.label(item.status.as_str());
                                    ui.add(
                                        egui::ProgressBar::new(item.progress())
                                            .desired_width(100.0)
                                            .text(format!("{:.0}%", item.progress() * 100.0)),
                                    );
                                    ui.label(if item.status == DownloadStatus::Running {
                                        format!(
                                            "{}/s",
                                            format_bytes(self.speed.get(&item.id).copied())
                                        )
                                    } else {
                                        "—".into()
                                    });
                                    let speed = self.speed.get(&item.id).copied().unwrap_or(0);
                                    let eta = item
                                        .size
                                        .filter(|_| speed > 0)
                                        .map(|size| {
                                            format!(
                                                "{} sec",
                                                size.saturating_sub(item.downloaded) / speed
                                            )
                                        })
                                        .unwrap_or_else(|| "—".into());
                                    ui.label(eta);
                                    ui.label(&item.created_at);
                                    ui.label(if item.resumable() { "Yes" } else { "No" });
                                    ui.end_row();
                                }
                            });
                        if self.items.is_empty() {
                            ui.centered_and_justified(|ui| {
                                ui.label("No downloads yet. Choose Add URL to get started.")
                            });
                        }
                    });
            });
        if self.add_open {
            self.add_dialog(ctx);
        }
    }
}

impl FidmApp {
    fn add_dialog(&mut self, ctx: &egui::Context) {
        egui::Window::new("Add URL")
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Direct URL");
                ui.text_edit_singleline(&mut self.url);
                ui.add_space(8.0);
                ui.label("Save folder");
                ui.text_edit_singleline(&mut self.save_dir);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Category");
                    egui::ComboBox::from_id_salt("category")
                        .selected_text(&self.category)
                        .show_ui(ui, |ui| {
                            for category in [
                                "Auto detect",
                                "All Downloads",
                                "Compressed",
                                "Documents",
                                "Music",
                                "Video",
                                "Programs",
                            ] {
                                ui.selectable_value(&mut self.category, category.into(), category);
                            }
                        });
                });
                if let Some(err) = &self.add_error {
                    ui.colored_label(Color32::RED, err);
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Download Later").clicked() {
                        self.add_download(true);
                    }
                    if ui.button(RichText::new("Start Now").strong()).clicked() {
                        self.add_download(false);
                    }
                    if ui.button("Cancel").clicked() {
                        self.add_open = false;
                    }
                });
            });
    }
}

fn toolbar_button(ui: &mut egui::Ui, icon: &str, label: &str, tooltip: &str) -> egui::Response {
    ui.add_sized(
        [86.0, 62.0],
        egui::Button::new(egui::RichText::new(format!("{icon}\n{label}")).size(13.0)),
    )
    .on_hover_text(tooltip)
}

fn default_download_dir() -> String {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Downloads")
        .to_string_lossy()
        .into_owned()
}
