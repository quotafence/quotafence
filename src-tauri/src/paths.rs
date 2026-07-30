use std::{io, path::PathBuf};

pub const APP_IDENTIFIER: &str = "com.buisonanh.agentquotamanager";
pub const DATABASE_FILENAME: &str = "agent-quota-manager.sqlite3";

pub fn default_database_path() -> io::Result<PathBuf> {
    dirs::data_dir()
        .map(|directory| directory.join(APP_IDENTIFIER).join(DATABASE_FILENAME))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "app data directory is unavailable"))
}
