use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg(test)]
pub fn global_process_env_lock() -> MutexGuard<'static, ()> {
    static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap()
}
