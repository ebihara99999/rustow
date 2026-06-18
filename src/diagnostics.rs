use crate::cli::PathDisplayOverride;
use crate::config::Config;
use crate::error::{ConfigError, FsError, IgnoreError, RustowError, StowError};
use crate::stow::{TargetActionReport, TargetActionReportStatus};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub(crate) struct RedactionTable {
    replacements: Vec<(String, String)>,
}

impl RedactionTable {
    pub(crate) fn new(path_displays: &[PathDisplayOverride]) -> Self {
        let mut replacements: Vec<(String, String)> = Vec::new();
        for override_path in path_displays {
            if let Some((path, display)) = {
                let path = override_path.path.display().to_string();
                if path.is_empty()
                    || path == override_path.display
                    || is_bare_relative_redaction_path(&override_path.path)
                {
                    None
                } else {
                    Some((path, override_path.display.clone()))
                }
            } {
                add_redaction_replacement(&mut replacements, path.clone(), display.clone());
                let debug_path = debug_path_fragment(&override_path.path);
                let debug_display = debug_string_fragment(&display);
                add_redaction_replacement(&mut replacements, debug_path, debug_display);
            }
        }
        replacements.sort_by(|(left, _), (right, _)| right.len().cmp(&left.len()));

        Self { replacements }
    }

    pub(crate) fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.replacements.is_empty() {
            return Cow::Borrowed(text);
        }

        self.replacements
            .iter()
            .fold(Cow::Borrowed(text), |redacted, (path, display)| {
                replace_path_occurrences(redacted, path, display)
            })
    }

    fn redact_path(&self, path: PathBuf) -> PathBuf {
        let display = path.display().to_string();
        let redacted = self.redact(&display);
        if redacted.as_ref() == display {
            path
        } else {
            PathBuf::from(redacted.into_owned())
        }
    }
}

pub(crate) fn process_reports(
    reports: &[TargetActionReport],
    config: &Config,
    path_displays: &[PathDisplayOverride],
) {
    let redactions = RedactionTable::new(path_displays);

    if reports.is_empty() {
        if config.verbosity > 0 {
            eprintln!("No actions to perform.");
        }
        return;
    }

    for report in reports {
        match &report.status {
            TargetActionReportStatus::Success => {
                if config.verbosity > 1 || config.simulate {
                    if let Some(message) = &report.message {
                        eprintln!("{}", redactions.redact(message));
                    }
                }
            },
            TargetActionReportStatus::Skipped => {
                if config.verbosity > 0 || config.simulate {
                    if let Some(message) = &report.message {
                        eprintln!("{}", redactions.redact(message));
                    }
                }
            },
            TargetActionReportStatus::ConflictPrevented => {
                if let Some(message) = &report.message {
                    eprintln!("{}", redactions.redact(message));
                }
            },
            TargetActionReportStatus::Failure(error) => {
                eprintln!("ERROR: {}", redactions.redact(error));
                if let Some(message) = &report.message {
                    eprintln!("Details: {}", redactions.redact(message));
                }
            },
        }
    }

    if config.verbosity > 0 || config.simulate {
        let success_count = reports
            .iter()
            .filter(|report| matches!(report.status, TargetActionReportStatus::Success))
            .count();
        let skipped_count = reports
            .iter()
            .filter(|report| matches!(report.status, TargetActionReportStatus::Skipped))
            .count();
        let conflict_count = reports
            .iter()
            .filter(|report| matches!(report.status, TargetActionReportStatus::ConflictPrevented))
            .count();
        let failure_count = reports
            .iter()
            .filter(|report| matches!(report.status, TargetActionReportStatus::Failure(_)))
            .count();

        eprintln!(
            "\nSummary: {} successful, {} skipped, {} conflicts, {} failures",
            success_count, skipped_count, conflict_count, failure_count
        );
    }
}

fn add_redaction_replacement(
    replacements: &mut Vec<(String, String)>,
    path: String,
    display: String,
) {
    if !path.is_empty()
        && path != display
        && !replacements.iter().any(|(needle, _)| needle == &path)
    {
        replacements.push((path, display));
    }
}

fn debug_path_fragment(path: &Path) -> String {
    unquote_debug_fragment(&format!("{:?}", path))
}

fn debug_string_fragment(value: &str) -> String {
    unquote_debug_fragment(&format!("{:?}", value))
}

fn unquote_debug_fragment(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

fn is_bare_relative_redaction_path(path: &Path) -> bool {
    path.is_relative() && path.components().count() == 1
}

fn replace_path_occurrences<'a>(
    text: Cow<'a, str>,
    needle: &str,
    replacement: &str,
) -> Cow<'a, str> {
    let source = text.as_ref();
    let mut output: Option<String> = None;
    let mut copied_until = 0;
    let mut search_start = 0;

    while let Some(relative_index) = source[search_start..].find(needle) {
        let index = search_start + relative_index;
        let end = index + needle.len();

        if is_path_boundary_before(source, index) && is_path_boundary_after(source, end) {
            let output = output.get_or_insert_with(|| String::with_capacity(source.len()));
            output.push_str(&source[copied_until..index]);
            output.push_str(replacement);
            copied_until = end;
        }

        search_start = end;
    }

    match output {
        Some(mut output) => {
            output.push_str(&source[copied_until..]);
            Cow::Owned(output)
        },
        None => text,
    }
}

fn is_path_boundary_before(text: &str, index: usize) -> bool {
    match text[..index].chars().next_back() {
        Some(ch) => is_path_boundary_char(ch),
        None => true,
    }
}

fn is_path_boundary_after(text: &str, index: usize) -> bool {
    match text[index..].chars().next() {
        Some(ch) => is_path_boundary_char(ch),
        None => true,
    }
}

fn is_path_boundary_char(ch: char) -> bool {
    matches!(
        ch,
        '/' | '\\'
            | '"'
            | '\''
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | ','
            | ':'
            | ';'
            | ' '
            | '\n'
            | '\t'
    )
}

pub(crate) fn redact_runtime_error(
    error: RustowError,
    path_displays: &[PathDisplayOverride],
) -> RustowError {
    let redactions = RedactionTable::new(path_displays);
    redact_rustow_error(error, &redactions)
}

fn redact_owned_string(value: String, redactions: &RedactionTable) -> String {
    match redactions.redact(&value) {
        Cow::Borrowed(_) => value,
        Cow::Owned(redacted) => redacted,
    }
}

fn redact_rustow_error(error: RustowError, redactions: &RedactionTable) -> RustowError {
    match error {
        RustowError::Config(error) => RustowError::Config(redact_config_error(error, redactions)),
        RustowError::Stow(error) => RustowError::Stow(redact_stow_error(error, redactions)),
        RustowError::Fs(error) => RustowError::Fs(redact_fs_error(error, redactions)),
        RustowError::Ignore(error) => RustowError::Ignore(redact_ignore_error(error, redactions)),
        RustowError::Cli(message) => RustowError::Cli(redact_owned_string(message, redactions)),
        RustowError::InvalidPattern(pattern) => {
            RustowError::InvalidPattern(redact_owned_string(pattern, redactions))
        },
        RustowError::Io(error) => RustowError::Io(error),
        RustowError::Regex(error) => RustowError::Regex(error),
    }
}

fn redact_config_error(error: ConfigError, redactions: &RedactionTable) -> ConfigError {
    match error {
        ConfigError::InvalidTargetDir(message) => {
            ConfigError::InvalidTargetDir(redact_owned_string(message, redactions))
        },
        ConfigError::InvalidStowDir(message) => {
            ConfigError::InvalidStowDir(redact_owned_string(message, redactions))
        },
        ConfigError::InvalidPackageName(message) => {
            ConfigError::InvalidPackageName(redact_owned_string(message, redactions))
        },
        ConfigError::InvalidRegexPattern(message) => {
            ConfigError::InvalidRegexPattern(redact_owned_string(message, redactions))
        },
        ConfigError::InvalidOperation(message) => {
            ConfigError::InvalidOperation(redact_owned_string(message, redactions))
        },
        ConfigError::InvalidVerbosityLevel(level) => ConfigError::InvalidVerbosityLevel(level),
    }
}

fn redact_stow_error(error: StowError, redactions: &RedactionTable) -> StowError {
    match error {
        StowError::Conflict(message) => {
            StowError::Conflict(redact_owned_string(message, redactions))
        },
        StowError::PackageNotFound(package) => {
            StowError::PackageNotFound(redact_owned_string(package, redactions))
        },
        StowError::InvalidPackageStructure(message) => {
            StowError::InvalidPackageStructure(redact_owned_string(message, redactions))
        },
        StowError::OperationFailed(message) => {
            StowError::OperationFailed(redact_owned_string(message, redactions))
        },
    }
}

fn redact_ignore_error(error: IgnoreError, redactions: &RedactionTable) -> IgnoreError {
    match error {
        IgnoreError::LoadPatternsError(message) => {
            IgnoreError::LoadPatternsError(redact_owned_string(message, redactions))
        },
        IgnoreError::InvalidPattern(pattern) => {
            IgnoreError::InvalidPattern(redact_owned_string(pattern, redactions))
        },
    }
}

fn redact_fs_error(error: FsError, redactions: &RedactionTable) -> FsError {
    match error {
        FsError::Io { path, source } => FsError::Io {
            path: redactions.redact_path(path),
            source,
        },
        FsError::Canonicalize { path, source } => FsError::Canonicalize {
            path: redactions.redact_path(path),
            source,
        },
        FsError::NotFound(path) => FsError::NotFound(redactions.redact_path(path)),
        FsError::NotADirectory(path) => FsError::NotADirectory(redactions.redact_path(path)),
        FsError::NotASymlink(path) => FsError::NotASymlink(redactions.redact_path(path)),
        FsError::CreateSymlink {
            link_path,
            target_path,
            source,
        } => FsError::CreateSymlink {
            link_path: redactions.redact_path(link_path),
            target_path: redactions.redact_path(target_path),
            source,
        },
        FsError::ReadSymlink { path, source } => FsError::ReadSymlink {
            path: redactions.redact_path(path),
            source,
        },
        FsError::DeleteSymlink { path, source } => FsError::DeleteSymlink {
            path: redactions.redact_path(path),
            source,
        },
        FsError::CreateDirectory { path, source } => FsError::CreateDirectory {
            path: redactions.redact_path(path),
            source,
        },
        FsError::DeleteDirectory { path, source } => FsError::DeleteDirectory {
            path: redactions.redact_path(path),
            source,
        },
        FsError::MoveItem {
            source_path,
            destination_path,
            source_io_error,
        } => FsError::MoveItem {
            source_path: redactions.redact_path(source_path),
            destination_path: redactions.redact_path(destination_path),
            source_io_error,
        },
        FsError::MoveSamePath(path) => FsError::MoveSamePath(redactions.redact_path(path)),
        FsError::WalkDir { path, source } => FsError::WalkDir {
            path: redactions.redact_path(path),
            source,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Args;
    use std::fs;
    use tempfile::tempdir;

    struct CurrentDirGuard {
        original: PathBuf,
    }

    impl CurrentDirGuard {
        fn set(path: &Path) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { original }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.original).unwrap();
        }
    }

    #[test]
    fn test_replace_path_occurrences_requires_leading_boundary() {
        let text = "real: /tmp/secret/file; unrelated: /backup/tmp/secret/file";

        assert_eq!(
            replace_path_occurrences(Cow::Borrowed(text), "/tmp/secret", "$RUSTOW_SECRET"),
            "real: $RUSTOW_SECRET/file; unrelated: /backup/tmp/secret/file"
        );
    }

    #[test]
    fn test_redaction_table_ignores_bare_relative_paths() {
        let redactions = RedactionTable::new(&[PathDisplayOverride::new(
            PathBuf::from("stow"),
            "$RUSTOW_STOW_DIR".to_string(),
        )]);

        assert_eq!(
            redactions
                .redact("Failed to canonicalize stow directory '$RUSTOW_STOW_DIR'")
                .as_ref(),
            "Failed to canonicalize stow directory '$RUSTOW_STOW_DIR'"
        );
    }

    #[test]
    fn test_redaction_table_borrows_when_no_replacements_apply() {
        let redactions = RedactionTable::new(&[]);
        let redacted = redactions.redact("No path redaction needed");

        assert!(matches!(redacted, Cow::Borrowed(_)));
    }

    #[test]
    fn test_redact_owned_string_returns_original_allocation_when_unchanged() {
        let redactions = RedactionTable::new(&[PathDisplayOverride::new(
            PathBuf::from("/tmp/secret"),
            "$RUSTOW_SECRET".to_string(),
        )]);
        let message = "No matching path".to_string();
        let original_ptr = message.as_ptr();
        let redacted = redact_owned_string(message, &redactions);

        assert_eq!(redacted.as_ptr(), original_ptr);
    }

    #[test]
    fn test_redaction_table_keeps_relative_paths_with_separators() {
        let redactions = RedactionTable::new(&[PathDisplayOverride::new(
            PathBuf::from("secret/stow"),
            "$RUSTOW_STOW_DIR".to_string(),
        )]);

        assert_eq!(
            redactions.redact("Path secret/stow/pkg is hidden").as_ref(),
            "Path $RUSTOW_STOW_DIR/pkg is hidden"
        );
    }

    #[test]
    fn test_redaction_table_replaces_debug_escaped_paths() {
        let secret_path = PathBuf::from("/tmp/secret\\root/stow");
        let redactions = RedactionTable::new(&[PathDisplayOverride::new(
            secret_path.clone(),
            "$RUSTOW_SECRET_ROOT/stow".to_string(),
        )]);
        let message = format!("SIMULATE: target {:?}", secret_path.join("pkg/bin/tool"));
        let redacted = redactions.redact(&message);

        assert!(!redacted.contains("secret\\\\root"));
        assert!(redacted.contains("$RUSTOW_SECRET_ROOT/stow/pkg/bin/tool"));
    }

    #[test]
    fn test_public_run_keeps_returned_error_paths_unredacted() {
        let _lock = crate::test_sync::IsolatedProcessEnv::new();
        let temp_dir = tempdir().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let target_dir = temp_dir.path().join("target");
        fs::create_dir_all(&stow_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(stow_dir.join("pkg"), "not a directory").unwrap();
        let _cwd_guard = CurrentDirGuard::set(temp_dir.path());

        let args = Args::parse_from(["rustow", "-d", "stow", "-t", "target", "pkg"]);
        let error = crate::run(args).unwrap_err().to_string();

        assert!(error.contains(stow_dir.join("pkg").to_string_lossy().as_ref()));
        assert!(!error.contains("\"stow/pkg\""));
    }
}
