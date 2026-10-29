//! Exclusive ownership of workdirs shared by supervisor startup and purge.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::path::Path;

/// Held until every managed service has stopped, or the entire purge completes.
/// The lock files live outside `run/` and must never be unlinked: replacing a
/// locked inode would let a second process acquire a different lock at that path.
#[derive(Debug)]
pub(crate) struct WorkdirLocks {
    _files: Vec<File>,
}

impl WorkdirLocks {
    pub(crate) fn acquire<'a>(workdirs: impl IntoIterator<Item = &'a Path>) -> Result<Self> {
        let mut paths = workdirs
            .into_iter()
            .map(|path| {
                path.canonicalize()
                    .with_context(|| format!("failed to resolve work directory {}", path.display()))
            })
            .collect::<Result<Vec<_>>>()?;
        paths.sort();
        paths.dedup();

        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let lock_path = path.join(".ckbadger.lock");
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
                .with_context(|| format!("failed to open workdir lock {}", lock_path.display()))?;
            // SAFETY: the descriptor is valid for the lifetime of `file`.
            // Rust opens it close-on-exec, so children cannot keep this lock alive.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    anyhow::bail!(
                        "work directory {} is already in use by another ckbadger run or purge; \
                         stop the existing supervisor with Ctrl+C before retrying (lock: {})",
                        path.display(),
                        lock_path.display()
                    );
                }
                return Err(error)
                    .with_context(|| format!("failed to lock work directory {}", path.display()));
            }
            files.push(file);
        }
        Ok(Self { _files: files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn workdir_lock_excludes_other_owners_and_releases_on_drop() {
        let dir = TempDir::new().unwrap();
        let owner = WorkdirLocks::acquire([dir.path()]).unwrap();
        let error = WorkdirLocks::acquire([dir.path()]).unwrap_err();
        assert!(error.to_string().contains("already in use"));
        drop(owner);
        let _next = WorkdirLocks::acquire([dir.path()]).unwrap();
        assert!(dir.path().join(".ckbadger.lock").is_file());
    }

    #[test]
    fn workdir_lock_deduplicates_symlink_aliases() {
        let dir = TempDir::new().unwrap();
        let workdir = dir.path().join("work");
        let alias = dir.path().join("alias");
        std::fs::create_dir(&workdir).unwrap();
        std::os::unix::fs::symlink(&workdir, &alias).unwrap();
        let _owner = WorkdirLocks::acquire([workdir.as_path(), alias.as_path()]).unwrap();
        assert!(WorkdirLocks::acquire([alias.as_path()]).is_err());
    }

    #[test]
    fn workdir_lock_failure_releases_previously_acquired_workdirs() {
        let dir = TempDir::new().unwrap();
        let first = dir.path().join("a");
        let second = dir.path().join("b");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let _owner = WorkdirLocks::acquire([second.as_path()]).unwrap();
        assert!(WorkdirLocks::acquire([first.as_path(), second.as_path()]).is_err());
        let _released = WorkdirLocks::acquire([first.as_path()]).unwrap();
    }
}
