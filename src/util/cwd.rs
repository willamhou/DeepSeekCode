use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::error::{app_error, AppResult};

fn cwd_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub fn lock_cwd() -> AppResult<MutexGuard<'static, ()>> {
    cwd_lock()
        .lock()
        .map_err(|_| app_error("working directory lock is poisoned"))
}

pub struct CwdGuard {
    _lock: MutexGuard<'static, ()>,
    original: PathBuf,
    restored: bool,
}

impl CwdGuard {
    pub fn enter(path: &Path) -> AppResult<Self> {
        let lock = lock_cwd()?;
        let original = std::env::current_dir()?;
        std::env::set_current_dir(path).map_err(|error| {
            app_error(format!(
                "failed to enter working directory {}: {error}",
                path.display()
            ))
        })?;
        Ok(Self {
            _lock: lock,
            original,
            restored: false,
        })
    }

    pub fn restore(mut self) -> AppResult<()> {
        self.restore_inner()?;
        self.restored = true;
        Ok(())
    }

    fn restore_inner(&self) -> AppResult<()> {
        std::env::set_current_dir(&self.original)
            .map_err(|error| app_error(format!("failed to restore working directory: {error}")))
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore_inner();
        }
    }
}
