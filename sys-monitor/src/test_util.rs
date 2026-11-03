//! Throw-away fake filesystem roots for unit tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A temporary directory standing in for `/`, removed on drop.
pub(crate) struct Fixture {
    root: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("sys-monitor-test-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture root");
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Write `contents` to `rel`, creating parent directories.
    pub fn write(&self, rel: &str, contents: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().expect("fixture path has parent"))
            .expect("create fixture dirs");
        fs::write(path, contents).expect("write fixture file");
    }

    /// Create a symlink at `rel` pointing to `target` (stored verbatim).
    pub fn symlink(&self, rel: &str, target: &str) {
        let path = self.path(rel);
        fs::create_dir_all(path.parent().expect("fixture path has parent"))
            .expect("create fixture dirs");
        std::os::unix::fs::symlink(target, path).expect("create fixture symlink");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
