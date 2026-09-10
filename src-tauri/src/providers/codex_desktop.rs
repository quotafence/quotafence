use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, OpenFlags};

use crate::{application::DesktopUsageObservation, workspace::canonicalize_workspace_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDesktopScan {
    pub observations: Vec<DesktopUsageObservation>,
    pub message: Option<String>,
}

impl CodexDesktopScan {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            observations: Vec::new(),
            message: Some(message.into()),
        }
    }

    pub fn is_available(&self) -> bool {
        self.message.is_none()
    }
}

pub fn scan() -> CodexDesktopScan {
    let Some(database_path) = state_database_path() else {
        return CodexDesktopScan::unavailable(
            "Codex Desktop metadata was not found. Start Codex Desktop once, then refresh.",
        );
    };
    match scan_database(&database_path) {
        Ok(observations) => CodexDesktopScan {
            observations,
            message: None,
        },
        Err(message) => CodexDesktopScan::unavailable(message),
    }
}

fn state_database_path() -> Option<PathBuf> {
    let codex_home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;
    newest_state_database(&codex_home)
}

fn newest_state_database(codex_home: &Path) -> Option<PathBuf> {
    fs::read_dir(codex_home)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            let version = file_name
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse::<u32>()
                .ok()?;
            Some((version, path))
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, path)| path)
}

fn scan_database(path: &Path) -> Result<Vec<DesktopUsageObservation>, String> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "Codex Desktop metadata could not be opened read-only.".to_owned())?;
    connection
        .busy_timeout(Duration::from_secs(2))
        .map_err(|_| "Codex Desktop metadata is temporarily busy.".to_owned())?;

    let columns = table_columns(&connection, "threads")?;
    for required in ["id", "cwd", "tokens_used", "updated_at"] {
        if !columns.iter().any(|column| column == required) {
            return Err(format!(
                "Codex Desktop metadata has no {required} field; this Codex version is not supported yet."
            ));
        }
    }
    let updated_at = if columns.iter().any(|column| column == "updated_at_ms") {
        "COALESCE(NULLIF(updated_at_ms, 0), updated_at * 1000)"
    } else {
        "updated_at * 1000"
    };
    let model = if columns.iter().any(|column| column == "model") {
        "NULLIF(trim(model), '')"
    } else {
        "NULL"
    };
    let query = format!(
        "SELECT id, cwd, {model}, tokens_used, {updated_at}
         FROM threads
         WHERE length(trim(id)) > 0
           AND length(trim(cwd)) > 0
           AND tokens_used >= 0"
    );
    let mut statement = connection
        .prepare(&query)
        .map_err(|_| "Codex Desktop thread metadata could not be queried.".to_owned())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|_| "Codex Desktop thread metadata could not be read.".to_owned())?;

    let mut observations = Vec::new();
    for row in rows {
        let Ok((thread_id, cwd, model, total_tokens, updated_at)) = row else {
            continue;
        };
        let Ok(total_tokens) = u64::try_from(total_tokens) else {
            continue;
        };
        let Ok(canonical_path) = canonicalize_workspace_path(cwd) else {
            continue;
        };
        observations.push(DesktopUsageObservation {
            thread_id,
            canonical_path,
            model,
            total_tokens,
            updated_at,
        });
    }
    Ok(observations)
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|_| "Codex Desktop schema could not be inspected.".to_owned())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|_| "Codex Desktop schema could not be inspected.".to_owned())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Codex Desktop schema could not be inspected.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "quotafence-codex-desktop-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn newest_state_database_uses_the_highest_schema_number() {
        let directory = TestDirectory::new();
        fs::write(directory.0.join("state_2.sqlite"), []).unwrap();
        fs::write(directory.0.join("state_11.sqlite"), []).unwrap();
        fs::write(directory.0.join("unrelated.sqlite"), []).unwrap();

        assert_eq!(
            newest_state_database(&directory.0),
            Some(directory.0.join("state_11.sqlite"))
        );
    }

    #[test]
    fn scanner_reads_only_thread_usage_metadata() {
        let directory = TestDirectory::new();
        let workspace = directory.0.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let database_path = directory.0.join("state_5.sqlite");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE threads (
                    id TEXT PRIMARY KEY,
                    cwd TEXT NOT NULL,
                    model TEXT,
                    tokens_used INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    updated_at_ms INTEGER,
                    title TEXT NOT NULL,
                    preview TEXT NOT NULL
                 );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO threads (
                    id, cwd, model, tokens_used, updated_at, updated_at_ms, title, preview
                 ) VALUES (?1, ?2, 'gpt-5.6-sol', 420, 10, 12000, 'private title', 'private preview')",
                params!["thread-1", workspace.to_str().unwrap()],
            )
            .unwrap();
        drop(connection);

        let observations = scan_database(&database_path).unwrap();

        assert_eq!(
            observations,
            vec![DesktopUsageObservation {
                thread_id: "thread-1".to_owned(),
                canonical_path: fs::canonicalize(workspace)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
                model: Some("gpt-5.6-sol".to_owned()),
                total_tokens: 420,
                updated_at: 12_000,
            }]
        );
    }

    #[test]
    fn incompatible_schema_fails_without_reading_content_fields() {
        let directory = TestDirectory::new();
        let database_path = directory.0.join("state_5.sqlite");
        let connection = Connection::open(&database_path).unwrap();
        connection
            .execute_batch("CREATE TABLE threads (id TEXT, title TEXT, preview TEXT);")
            .unwrap();
        drop(connection);

        assert!(scan_database(&database_path)
            .unwrap_err()
            .contains("no cwd field"));
    }
}
