use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug)]
pub enum RepositoryError {
    PathUnavailable { path: PathBuf, source: io::Error },
    NotDirectory { path: PathBuf },
    GitUnavailable { source: io::Error },
    NotRepository { path: PathBuf },
    InvalidGitOutput,
    NonUtf8Path { path: PathBuf },
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathUnavailable { path, .. } => {
                write!(
                    formatter,
                    "repository path {} is unavailable",
                    path.display()
                )
            }
            Self::NotDirectory { path } => {
                write!(
                    formatter,
                    "repository path {} is not a directory",
                    path.display()
                )
            }
            Self::GitUnavailable { .. } => {
                formatter.write_str("Git is not installed or is unavailable on PATH")
            }
            Self::NotRepository { path } => {
                write!(formatter, "{} is not inside a Git worktree", path.display())
            }
            Self::InvalidGitOutput => {
                formatter.write_str("Git returned an invalid repository root")
            }
            Self::NonUtf8Path { path } => write!(
                formatter,
                "repository path {} cannot be represented as UTF-8",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RepositoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PathUnavailable { source, .. } | Self::GitUnavailable { source } => Some(source),
            _ => None,
        }
    }
}

pub fn resolve_git_root(start: impl AsRef<Path>) -> Result<String, RepositoryError> {
    let start = start.as_ref();
    let canonical_start =
        fs::canonicalize(start).map_err(|source| RepositoryError::PathUnavailable {
            path: start.to_path_buf(),
            source,
        })?;
    if !canonical_start.is_dir() {
        return Err(RepositoryError::NotDirectory {
            path: canonical_start,
        });
    }

    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&canonical_start)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|source| RepositoryError::GitUnavailable { source })?;
    if !output.status.success() {
        return Err(RepositoryError::NotRepository {
            path: canonical_start,
        });
    }

    let root = String::from_utf8(output.stdout)
        .map_err(|_| RepositoryError::InvalidGitOutput)?
        .trim()
        .to_owned();
    if root.is_empty() {
        return Err(RepositoryError::InvalidGitOutput);
    }
    let root_path = PathBuf::from(root);
    let root = fs::canonicalize(&root_path).map_err(|source| RepositoryError::PathUnavailable {
        path: root_path,
        source,
    })?;
    root.to_str()
        .map(str::to_owned)
        .ok_or(RepositoryError::NonUtf8Path { path: root })
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
            let path =
                std::env::temp_dir().join(format!("aqm-{name}-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn init_repository(path: &Path) {
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn nested_directory_resolves_to_the_worktree_root() {
        let directory = TestDirectory::new("nested-repository");
        init_repository(&directory.0);
        let nested = directory.0.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            resolve_git_root(&nested).unwrap(),
            fs::canonicalize(&directory.0).unwrap().to_str().unwrap()
        );
    }

    #[test]
    fn directory_outside_a_repository_is_rejected() {
        let directory = TestDirectory::new("not-repository");

        assert!(matches!(
            resolve_git_root(&directory.0),
            Err(RepositoryError::NotRepository { .. })
        ));
    }

    #[test]
    fn deleted_path_is_reported_as_unavailable() {
        let directory = TestDirectory::new("deleted-repository");
        let deleted = directory.0.join("deleted");

        assert!(matches!(
            resolve_git_root(deleted),
            Err(RepositoryError::PathUnavailable { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_resolves_to_the_canonical_root() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::new("symlinked-repository");
        init_repository(&directory.0);
        let link_parent = TestDirectory::new("symlink-parent");
        let link = link_parent.0.join("repo-link");
        symlink(&directory.0, &link).unwrap();

        assert_eq!(
            resolve_git_root(link).unwrap(),
            fs::canonicalize(&directory.0).unwrap().to_str().unwrap()
        );
    }
}
