use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum WorkspaceError {
    PathUnavailable { path: PathBuf, source: io::Error },
    NotDirectory { path: PathBuf },
    NonUtf8Path { path: PathBuf },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathUnavailable { path, .. } => {
                write!(
                    formatter,
                    "workspace path {} is unavailable",
                    path.display()
                )
            }
            Self::NotDirectory { path } => {
                write!(
                    formatter,
                    "workspace path {} is not a directory",
                    path.display()
                )
            }
            Self::NonUtf8Path { path } => write!(
                formatter,
                "workspace path {} cannot be represented as UTF-8",
                path.display()
            ),
        }
    }
}

impl std::error::Error for WorkspaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PathUnavailable { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn canonicalize_workspace_path(path: impl AsRef<Path>) -> Result<String, WorkspaceError> {
    let path = path.as_ref();
    let canonical = fs::canonicalize(path).map_err(|source| WorkspaceError::PathUnavailable {
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.is_dir() {
        return Err(WorkspaceError::NotDirectory { path: canonical });
    }

    canonical
        .to_str()
        .map(str::to_owned)
        .ok_or(WorkspaceError::NonUtf8Path { path: canonical })
}

pub fn contains_path(workspace_root: &str, candidate: &str) -> bool {
    Path::new(candidate).starts_with(Path::new(workspace_root))
}

pub fn path_depth(path: &str) -> usize {
    Path::new(path).components().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("quotafence-{name}-{}-{unique}", std::process::id()));
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
    fn canonicalizes_a_plain_folder_without_git() {
        let directory = TestDirectory::new("plain-workspace");

        assert_eq!(
            canonicalize_workspace_path(&directory.0).unwrap(),
            fs::canonicalize(&directory.0).unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn deleted_path_is_reported_as_unavailable() {
        let directory = TestDirectory::new("deleted-workspace");
        let deleted = directory.0.join("deleted");

        assert!(matches!(
            canonicalize_workspace_path(deleted),
            Err(WorkspaceError::PathUnavailable { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_folder_resolves_to_the_canonical_path() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new("workspace-target");
        let link_parent = TestDirectory::new("workspace-link-parent");
        let link = link_parent.0.join("workspace-link");
        symlink(&directory.0, &link).unwrap();

        assert_eq!(
            canonicalize_workspace_path(link).unwrap(),
            fs::canonicalize(&directory.0).unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn containment_is_path_component_aware() {
        assert!(contains_path("/code/app", "/code/app/src"));
        assert!(contains_path("/code/app", "/code/app"));
        assert!(!contains_path("/code/app", "/code/application"));
        assert!(path_depth("/code/app/src") > path_depth("/code/app"));
    }
}
