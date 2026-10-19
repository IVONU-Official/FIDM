use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Queued,
    Running,
    Paused,
    Finished,
    Error,
}

impl DownloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Paused => "Paused",
            Self::Finished => "Finished",
            Self::Error => "Error",
        }
    }
    pub fn parse(value: &str) -> Self {
        match value {
            "Running" => Self::Running,
            "Paused" => Self::Paused,
            "Finished" => Self::Finished,
            "Error" => Self::Error,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub path: String,
    pub category: String,
    pub size: Option<u64>,
    pub downloaded: u64,
    pub status: DownloadStatus,
    pub error: Option<String>,
    pub created_at: String,
}

impl Download {
    pub fn progress(&self) -> f32 {
        match self.size {
            Some(size) if size > 0 => (self.downloaded as f64 / size as f64).clamp(0.0, 1.0) as f32,
            _ => 0.0,
        }
    }
    pub fn resumable(&self) -> bool {
        self.downloaded > 0 && self.status != DownloadStatus::Finished
    }
}

pub fn category_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => "Compressed",
        "pdf" | "doc" | "docx" | "txt" | "epub" | "odt" | "rtf" => "Documents",
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => "Music",
        "mp4" | "mkv" | "avi" | "webm" | "mov" | "ogv" => "Video",
        "exe" | "msi" | "deb" | "rpm" | "appimage" | "dmg" => "Programs",
        _ => "All Downloads",
    }
}

pub fn format_bytes(value: Option<u64>) -> String {
    let Some(bytes) = value else {
        return "Unknown".into();
    };
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut amount = bytes as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < units.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{amount:.1} {}", units[unit])
    }
}
