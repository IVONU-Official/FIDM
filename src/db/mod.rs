use std::path::Path;
use std::sync::Mutex;

use anyhow::Context;
use rusqlite::{Connection, params};

use crate::model::{Download, DownloadStatus};

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let connection = Connection::open(path).context("open FIDM database")?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS downloads (
               id INTEGER PRIMARY KEY, url TEXT NOT NULL, name TEXT NOT NULL, path TEXT NOT NULL,
               category TEXT NOT NULL, size INTEGER, downloaded INTEGER NOT NULL DEFAULT 0,
               status TEXT NOT NULL, error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
             );",
            )
            .context("initialize FIDM database")?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn list(&self) -> anyhow::Result<Vec<Download>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare("SELECT id,url,name,path,category,size,downloaded,status,error,created_at FROM downloads ORDER BY id DESC")?;
        let rows = statement.query_map([], |row| {
            Ok(Download {
                id: row.get(0)?,
                url: row.get(1)?,
                name: row.get(2)?,
                path: row.get(3)?,
                category: row.get(4)?,
                size: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
                downloaded: row.get::<_, i64>(6)? as u64,
                status: DownloadStatus::parse(&row.get::<_, String>(7)?),
                error: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .context("read downloads")
    }

    pub fn add(
        &self,
        url: &str,
        name: &str,
        path: &str,
        category: &str,
        size: Option<u64>,
    ) -> anyhow::Result<i64> {
        let connection = self.connection.lock().unwrap();
        connection.execute("INSERT INTO downloads(url,name,path,category,size,status) VALUES(?1,?2,?3,?4,?5,'Queued')",
            params![url, name, path, category, size.map(|v| v as i64)])?;
        Ok(connection.last_insert_rowid())
    }

    pub fn update(
        &self,
        id: i64,
        status: DownloadStatus,
        downloaded: u64,
        size: Option<u64>,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute("UPDATE downloads SET status=?2,downloaded=?3,size=COALESCE(?4,size),error=?5 WHERE id=?1",
            params![id, status.as_str(), downloaded as i64, size.map(|v| v as i64), error])?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> anyhow::Result<()> {
        self.connection
            .lock()
            .unwrap()
            .execute("DELETE FROM downloads WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn set_status(
        &self,
        id: i64,
        status: DownloadStatus,
        error: Option<&str>,
    ) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute(
            "UPDATE downloads SET status=?2,error=?3 WHERE id=?1",
            params![id, status.as_str(), error],
        )?;
        Ok(())
    }

    pub fn recover_interrupted(&self) -> anyhow::Result<()> {
        self.connection.lock().unwrap().execute(
            "UPDATE downloads SET status='Paused' WHERE status='Running'",
            [],
        )?;
        Ok(())
    }
}
