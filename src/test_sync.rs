use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use tempfile::TempDir;

#[cfg(test)]
pub fn global_process_env_lock() -> MutexGuard<'static, ()> {
    static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[cfg(test)]
pub struct IsolatedProcessEnv {
    original_cwd: PathBuf,
    original_home: Option<OsString>,
    original_stow_dir: Option<OsString>,
    _temp_dir: TempDir,
    _lock: MutexGuard<'static, ()>,
}

#[cfg(test)]
impl IsolatedProcessEnv {
    pub fn new() -> Self {
        let lock = global_process_env_lock();
        let original_cwd = std::env::current_dir().expect("current dir should be obtainable");
        let original_home = std::env::var_os("HOME");
        let original_stow_dir = std::env::var_os("STOW_DIR");
        let temp_dir = tempfile::tempdir().expect("test temp dir should be creatable");
        let home_dir = temp_dir.path().join("home");
        let cwd = temp_dir.path().join("cwd");

        std::fs::create_dir_all(&home_dir).expect("test HOME should be creatable");
        std::fs::create_dir_all(&cwd).expect("test cwd should be creatable");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("STOW_DIR");
        }
        std::env::set_current_dir(&cwd).expect("test cwd should be usable");

        Self {
            original_cwd,
            original_home,
            original_stow_dir,
            _temp_dir: temp_dir,
            _lock: lock,
        }
    }
}

#[cfg(test)]
impl Drop for IsolatedProcessEnv {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
        unsafe {
            match &self.original_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
            match &self.original_stow_dir {
                Some(value) => std::env::set_var("STOW_DIR", value),
                None => std::env::remove_var("STOW_DIR"),
            }
        }
    }
}
