use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn redacted_paths() -> &'static Mutex<Vec<(PathBuf, String)>> {
    static REDACTED_PATHS: OnceLock<Mutex<Vec<(PathBuf, String)>>> = OnceLock::new();

    REDACTED_PATHS.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn clear_redacted_paths() {
    redacted_paths().lock().unwrap().clear();
}

pub(crate) fn register_redacted_path(path: PathBuf, display: String) {
    redacted_paths().lock().unwrap().push((path, display));
}

pub(crate) fn path_display(path: &Path) -> String {
    let redacted_paths = redacted_paths().lock().unwrap();
    redacted_paths
        .iter()
        .rev()
        .find(|(registered_path, _)| registered_path == path)
        .map(|(_, display)| display.clone())
        .unwrap_or_else(|| path.display().to_string())
}
