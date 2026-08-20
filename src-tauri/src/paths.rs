use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const APP_IDENTIFIER: &str = "com.buisonanh.quotafence";
pub const DATABASE_FILENAME: &str = "quotafence.sqlite3";

// Kept only for the one-time on-device migration from pre-QuotaFence builds.
const LEGACY_APP_IDENTIFIER: &str = "com.buisonanh.agentquotamanager";
const LEGACY_DATABASE_FILENAME: &str = "agent-quota-manager.sqlite3";

pub fn default_database_path() -> io::Result<PathBuf> {
    dirs::data_dir()
        .map(|directory| directory.join(APP_IDENTIFIER).join(DATABASE_FILENAME))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "app data directory is unavailable"))
}

pub fn migrate_legacy_database(target: &Path) -> io::Result<bool> {
    if target.exists() {
        return Ok(false);
    }
    let Some(data_dir) = dirs::data_dir() else {
        return Ok(false);
    };
    let legacy = data_dir
        .join(LEGACY_APP_IDENTIFIER)
        .join(LEGACY_DATABASE_FILENAME);
    migrate_database_from(&legacy, target)
}

fn migrate_database_from(legacy: &Path, target: &Path) -> io::Result<bool> {
    if target.exists() || !legacy.exists() {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(legacy, target)?;
    for suffix in ["-wal", "-shm"] {
        let source = PathBuf::from(format!("{}{suffix}", legacy.display()));
        if source.exists() {
            fs::copy(
                &source,
                PathBuf::from(format!("{}{suffix}", target.display())),
            )?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_identity_uses_quotafence_namespace() {
        assert_eq!(APP_IDENTIFIER, "com.buisonanh.quotafence");
        assert_eq!(DATABASE_FILENAME, "quotafence.sqlite3");
    }

    #[test]
    fn legacy_database_is_copied_once_without_overwriting_new_state() {
        let root = std::env::temp_dir().join(format!(
            "quotafence-path-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let legacy = root.join("legacy.sqlite3");
        let target = root.join("new/quotafence.sqlite3");
        fs::write(&legacy, b"legacy-ledger").unwrap();

        assert!(migrate_database_from(&legacy, &target).unwrap());
        assert_eq!(fs::read(&target).unwrap(), b"legacy-ledger");
        fs::write(&target, b"new-ledger").unwrap();
        assert!(!migrate_database_from(&legacy, &target).unwrap());
        assert_eq!(fs::read(&target).unwrap(), b"new-ledger");

        fs::remove_dir_all(root).unwrap();
    }
}
