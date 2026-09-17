use crate::models::{AiSettings, EventRecord, Issue, Project, StatsSummary};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;

        // WAL mode for high concurrency and performance
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA busy_timeout = 5000;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                public_key TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS issues (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                title TEXT NOT NULL,
                culprit TEXT NOT NULL,
                level TEXT NOT NULL,
                platform TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'unresolved',
                count INTEGER NOT NULL DEFAULT 1,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                ai_diagnosis TEXT,
                ai_fix_diff TEXT,
                UNIQUE(project_id, fingerprint)
            );

            CREATE INDEX IF NOT EXISTS idx_issues_project_status ON issues(project_id, status);
            CREATE INDEX IF NOT EXISTS idx_issues_last_seen ON issues(last_seen DESC);

            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                issue_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(issue_id) REFERENCES issues(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_events_issue_id ON events(issue_id);
            CREATE INDEX IF NOT EXISTS idx_events_created_at ON events(created_at DESC);

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )?;

        // Seed default project if missing
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM projects WHERE id = '1'",
            [],
            |r| r.get(0),
        )?;
        if exists == 0 {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO projects (id, name, public_key, created_at) VALUES ('1', 'Default Project', 'public', ?1)",
                params![now],
            )?;
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn get_default_project(&self) -> Result<Project, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, name, public_key, created_at FROM projects WHERE id = '1'",
            [],
            |r| {
                Ok(Project {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    public_key: r.get(2)?,
                    created_at: r.get(3)?,
                })
            },
        )
    }

    pub fn record_event(
        &self,
        project_id: &str,
        event_id: &str,
        fingerprint: &str,
        title: &str,
        culprit: &str,
        level: &str,
        platform: &str,
        payload_json: &serde_json::Value,
    ) -> Result<(String, String), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();

        // Check if issue exists
        let existing_issue_id: Option<String> = conn
            .query_row(
                "SELECT id FROM issues WHERE project_id = ?1 AND fingerprint = ?2",
                params![project_id, fingerprint],
                |r| r.get(0),
            )
            .ok();

        let issue_id = if let Some(id) = existing_issue_id {
            // Update existing issue
            conn.execute(
                "UPDATE issues 
                 SET count = count + 1, last_seen = ?1, status = CASE WHEN status = 'resolved' THEN 'unresolved' ELSE status END 
                 WHERE id = ?2",
                params![now, id],
            )?;
            id
        } else {
            // Insert new issue
            let new_id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO issues (id, project_id, fingerprint, title, culprit, level, platform, status, count, first_seen, last_seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'unresolved', 1, ?8, ?8)",
                params![
                    new_id,
                    project_id,
                    fingerprint,
                    title,
                    culprit,
                    level,
                    platform,
                    now
                ],
            )?;
            new_id
        };

        // Insert event
        let payload_str = payload_json.to_string();
        conn.execute(
            "INSERT INTO events (id, issue_id, project_id, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![event_id, issue_id, project_id, payload_str, now],
        )?;

        Ok((issue_id, event_id.to_string()))
    }

    pub fn list_issues(
        &self,
        project_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Issue>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let mut query = "SELECT id, project_id, fingerprint, title, culprit, level, platform, status, count, first_seen, last_seen, ai_diagnosis, ai_fix_diff FROM issues WHERE 1=1".to_string();
        let mut param_vals: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(pid) = project_id {
            query.push_str(" AND project_id = ?");
            param_vals.push(Box::new(pid.to_string()));
        }

        if let Some(st) = status {
            if st != "all" {
                query.push_str(" AND status = ?");
                param_vals.push(Box::new(st.to_string()));
            }
        }

        query.push_str(" ORDER BY last_seen DESC LIMIT ?");
        param_vals.push(Box::new(limit));

        let params_ref: Vec<&dyn rusqlite::ToSql> = param_vals.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&query)?;

        let rows = stmt.query_map(params_ref.as_slice(), |r| {
            Ok(Issue {
                id: r.get(0)?,
                project_id: r.get(1)?,
                fingerprint: r.get(2)?,
                title: r.get(3)?,
                culprit: r.get(4)?,
                level: r.get(5)?,
                platform: r.get(6)?,
                status: r.get(7)?,
                count: r.get(8)?,
                first_seen: r.get(9)?,
                last_seen: r.get(10)?,
                ai_diagnosis: r.get(11)?,
                ai_fix_diff: r.get(12)?,
            })
        })?;

        let mut issues = Vec::new();
        for issue in rows {
            issues.push(issue?);
        }
        Ok(issues)
    }

    pub fn get_issue(&self, issue_id: &str) -> Result<Option<(Issue, Vec<EventRecord>)>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let issue_opt = conn
            .query_row(
                "SELECT id, project_id, fingerprint, title, culprit, level, platform, status, count, first_seen, last_seen, ai_diagnosis, ai_fix_diff FROM issues WHERE id = ?1",
                params![issue_id],
                |r| {
                    Ok(Issue {
                        id: r.get(0)?,
                        project_id: r.get(1)?,
                        fingerprint: r.get(2)?,
                        title: r.get(3)?,
                        culprit: r.get(4)?,
                        level: r.get(5)?,
                        platform: r.get(6)?,
                        status: r.get(7)?,
                        count: r.get(8)?,
                        first_seen: r.get(9)?,
                        last_seen: r.get(10)?,
                        ai_diagnosis: r.get(11)?,
                        ai_fix_diff: r.get(12)?,
                    })
                },
            )
            .ok();

        let issue = match issue_opt {
            Some(i) => i,
            None => return Ok(None),
        };

        // Fetch up to 20 recent events for this issue
        let mut stmt = conn.prepare(
            "SELECT id, issue_id, project_id, payload, created_at FROM events WHERE issue_id = ?1 ORDER BY created_at DESC LIMIT 20",
        )?;

        let event_rows = stmt.query_map(params![issue_id], |r| {
            let payload_str: String = r.get(3)?;
            let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
            Ok(EventRecord {
                id: r.get(0)?,
                issue_id: r.get(1)?,
                project_id: r.get(2)?,
                payload,
                created_at: r.get(4)?,
            })
        })?;

        let mut events = Vec::new();
        for ev in event_rows {
            events.push(ev?);
        }

        Ok(Some((issue, events)))
    }

    pub fn update_issue_status(&self, issue_id: &str, status: &str) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE issues SET status = ?1 WHERE id = ?2",
            params![status, issue_id],
        )?;
        Ok(())
    }

    pub fn save_ai_analysis(
        &self,
        issue_id: &str,
        diagnosis: &str,
        diff: Option<&str>,
    ) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE issues SET ai_diagnosis = ?1, ai_fix_diff = ?2 WHERE id = ?3",
            params![diagnosis, diff, issue_id],
        )?;
        Ok(())
    }

    pub fn get_stats(&self) -> Result<StatsSummary, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let total_issues: i64 = conn.query_row("SELECT COUNT(*) FROM issues", [], |r| r.get(0))?;
        let unresolved_issues: i64 = conn.query_row(
            "SELECT COUNT(*) FROM issues WHERE status = 'unresolved'",
            [],
            |r| r.get(0),
        )?;
        let total_events: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;

        // 24h count
        let one_day_ago = (Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
        let events_24h: i64 = conn.query_row(
            "SELECT COUNT(*) FROM events WHERE created_at >= ?1",
            params![one_day_ago],
            |r| r.get(0),
        )?;

        Ok(StatsSummary {
            total_issues,
            unresolved_issues,
            total_events,
            events_24h,
        })
    }

    pub fn get_ai_settings(&self) -> Result<AiSettings, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let val_opt: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'ai_settings'",
                [],
                |r| r.get(0),
            )
            .ok();

        match val_opt {
            Some(v) => Ok(serde_json::from_str(&v).unwrap_or_default()),
            None => Ok(AiSettings::default()),
        }
    }

    pub fn save_ai_settings(&self, settings: &AiSettings) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let json_str = serde_json::to_string(settings).unwrap_or_default();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('ai_settings', ?1)",
            params![json_str],
        )?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM events", [])?;
        conn.execute("DELETE FROM issues", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_db_init_and_project() {
        let db = Database::new(":memory:").expect("Failed to create memory db");
        let proj = db.get_default_project().expect("Failed to get default project");
        assert_eq!(proj.id, "1");
        assert_eq!(proj.name, "Default Project");
    }

    #[test]
    fn test_record_and_deduplicate() {
        let db = Database::new(":memory:").unwrap();
        let payload = json!({"level": "error", "message": "Crash"});

        // 1st event
        let (issue_id1, event_id1) = db
            .record_event("1", "ev-1", "fp-a", "Error A", "main.rs:1", "error", "rust", &payload)
            .unwrap();

        // 2nd event with same fingerprint -> should group to same issue
        let (issue_id2, event_id2) = db
            .record_event("1", "ev-2", "fp-a", "Error A", "main.rs:1", "error", "rust", &payload)
            .unwrap();

        assert_eq!(issue_id1, issue_id2);
        assert_ne!(event_id1, event_id2);

        let issues = db.list_issues(Some("1"), None, 10).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].count, 2);

        let (issue, events) = db.get_issue(&issue_id1).unwrap().unwrap();
        assert_eq!(issue.id, issue_id1);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_status_update_and_stats() {
        let db = Database::new(":memory:").unwrap();
        let payload = json!({"level": "error"});
        let (issue_id, _) = db
            .record_event("1", "ev-1", "fp-b", "Error B", "lib.rs:2", "error", "rust", &payload)
            .unwrap();

        let stats_before = db.get_stats().unwrap();
        assert_eq!(stats_before.unresolved_issues, 1);

        db.update_issue_status(&issue_id, "resolved").unwrap();

        let stats_after = db.get_stats().unwrap();
        assert_eq!(stats_after.unresolved_issues, 0);

        // Filter by unresolved
        let unresolved = db.list_issues(None, Some("unresolved"), 10).unwrap();
        assert_eq!(unresolved.len(), 0);

        // Filter by resolved
        let resolved = db.list_issues(None, Some("resolved"), 10).unwrap();
        assert_eq!(resolved.len(), 1);
    }

    #[test]
    fn test_ai_settings_roundtrip() {
        let db = Database::new(":memory:").unwrap();
        let s = db.get_ai_settings().unwrap();
        assert_eq!(s.provider, "ollama");

        let mut custom = s;
        custom.provider = "gemini".to_string();
        custom.api_key = "secret123".to_string();
        db.save_ai_settings(&custom).unwrap();

        let loaded = db.get_ai_settings().unwrap();
        assert_eq!(loaded.provider, "gemini");
        assert_eq!(loaded.api_key, "secret123");
    }
}

