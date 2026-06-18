pub mod cli;
pub mod config;
pub mod dotfiles;
pub mod error;
pub mod fs_utils;
pub mod ignore;
pub mod stow;
#[cfg(test)]
mod test_sync;

use crate::cli::{
    Args, OperationGroup, OperationMode, ParsedArgs, PathDisplayOverride, RuntimeParsedArgs,
};
use crate::config::{Config, PackageOperation, StowMode};
use crate::error::{ConfigError, FsError, IgnoreError, RustowError, StowError};
use crate::stow::{
    delete_packages, mixed_packages, restow_packages, stow_packages,
    validate_package_for_operation_with_display,
};
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

/// Runs the rustow application logic.
pub fn run(args: Args) -> Result<(), RustowError> {
    reject_ambiguous_mixed_args(&args)?;
    run_with_operation_groups(args, Vec::new())
}

pub fn run_parsed(parsed_args: ParsedArgs) -> Result<(), RustowError> {
    run_with_operation_groups_and_path_displays(
        parsed_args.args,
        parsed_args.operation_groups,
        Vec::new(),
        false,
    )
}

/// Runs runtime-parsed arguments and returns errors redacted for diagnostics.
///
/// Resource-file path values expanded from environment variables or tildes may
/// be represented with their original display strings in returned errors.
#[doc(hidden)]
pub fn run_runtime_parsed(parsed_args: RuntimeParsedArgs) -> Result<(), RustowError> {
    let (parsed_args, path_displays) = parsed_args.into_parts();
    run_with_operation_groups_and_path_displays(
        parsed_args.args,
        parsed_args.operation_groups,
        path_displays,
        true,
    )
}

/// Runs rustow with operation groups reconstructed from CLI argument order.
pub fn run_with_operation_groups(
    args: Args,
    operation_groups: Vec<OperationGroup>,
) -> Result<(), RustowError> {
    run_with_operation_groups_and_path_displays(args, operation_groups, Vec::new(), false)
}

fn run_with_operation_groups_and_path_displays(
    args: Args,
    operation_groups: Vec<OperationGroup>,
    mut path_displays: Vec<PathDisplayOverride>,
    redact_diagnostics: bool,
) -> Result<(), RustowError> {
    let result = (|| {
        // eprintln!("stderr: Successfully parsed args in lib::run: {:?}", args.clone());
        if operation_groups.is_empty() {
            reject_ambiguous_mixed_args(&args)?;
        }

        match Config::from_args_with_path_displays(args, &mut path_displays) {
            Ok(config) => {
                // eprintln!("stderr: Successfully constructed config in lib::run: {:?}", config);

                let package_operations = package_operations_for_config(&config, operation_groups);
                let diagnostic_path_displays = if redact_diagnostics {
                    path_displays.as_slice()
                } else {
                    &[]
                };
                preflight_package_operations(
                    &config,
                    &package_operations,
                    diagnostic_path_displays,
                )?;
                let reports = execute_config_operations(&config, &package_operations)?;

                // Process reports for logging/output
                process_reports(&reports, &config, diagnostic_path_displays);

                let conflict_count = reports
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.status,
                            crate::stow::TargetActionReportStatus::ConflictPrevented
                        )
                    })
                    .count();
                let failure_count = reports
                    .iter()
                    .filter(|r| {
                        matches!(r.status, crate::stow::TargetActionReportStatus::Failure(_))
                    })
                    .count();

                if conflict_count > 0 || failure_count > 0 {
                    return Err(RustowError::Stow(StowError::OperationFailed(format!(
                        "Execution stopped with {} conflicts and {} failures",
                        conflict_count, failure_count
                    ))));
                }

                Ok(())
            },
            Err(e) => {
                // eprintln!("stderr: Error constructing config in lib::run: {}", e);
                Err(e)
            },
        }
    })();

    if redact_diagnostics {
        result.map_err(|error| redact_runtime_error(error, &path_displays))
    } else {
        result
    }
}

fn reject_ambiguous_mixed_args(args: &Args) -> Result<(), RustowError> {
    let operation_flag_count = [args.stow, args.delete, args.restow]
        .into_iter()
        .filter(|flag| *flag)
        .count();

    if operation_flag_count > 1 {
        return Err(RustowError::Config(ConfigError::InvalidOperation(
            "mixed -S/-D/-R arguments require Args::parse_from_with_operation_groups or run_parsed"
                .to_string(),
        )));
    }

    Ok(())
}

fn package_operations_for_config(
    config: &Config,
    operation_groups: Vec<OperationGroup>,
) -> Vec<PackageOperation> {
    if operation_groups.is_empty() {
        return vec![PackageOperation {
            mode: config.mode.clone(),
            packages: config.packages.clone(),
        }];
    }

    operation_groups
        .into_iter()
        .map(|group| {
            let mode = match group.mode {
                OperationMode::Stow => StowMode::Stow,
                OperationMode::Delete => StowMode::Delete,
                OperationMode::Restow => StowMode::Restow,
            };

            PackageOperation {
                mode,
                packages: group.packages,
            }
        })
        .collect()
}

fn preflight_package_operations(
    config: &Config,
    operations: &[PackageOperation],
    path_displays: &[PathDisplayOverride],
) -> Result<(), RustowError> {
    for operation in operations {
        for package_name in &operation.packages {
            validate_package_name(package_name)?;
            let package_path = config.stow_dir.join(package_name);
            let package_path_display =
                crate::cli::path_display_with_prefix(&package_path, path_displays);
            let stow_dir_display =
                crate::cli::path_display_with_prefix(&config.stow_dir, path_displays);
            validate_package_for_operation_with_display(
                &config.stow_dir,
                package_name,
                Some(&package_path_display),
                Some(&stow_dir_display),
            )?;
        }
    }

    Ok(())
}

fn validate_package_name(package_name: &str) -> Result<(), RustowError> {
    let package_path = Path::new(package_name);
    let escapes_stow_dir = package_path.is_absolute()
        || package_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        });

    if package_name.is_empty() || escapes_stow_dir {
        return Err(RustowError::Config(ConfigError::InvalidPackageName(
            package_name.to_string(),
        )));
    }

    Ok(())
}

fn execute_config_operations(
    config: &Config,
    operation_groups: &[PackageOperation],
) -> Result<Vec<crate::stow::TargetActionReport>, RustowError> {
    if operation_groups.len() > 1 {
        return execute_mixed_operation_groups(config, operation_groups);
    }

    let mut reports = Vec::new();

    for operation in operation_groups {
        let mut operation_reports = execute_operation_group(config, operation)?;
        let should_stop = !config.simulate && reports_have_blocking_status(&operation_reports);
        reports.append(&mut operation_reports);

        if should_stop {
            break;
        }
    }

    Ok(reports)
}

fn execute_mixed_operation_groups(
    config: &Config,
    operation_groups: &[PackageOperation],
) -> Result<Vec<crate::stow::TargetActionReport>, RustowError> {
    let mut delete_packages = Vec::new();
    let mut stow_packages = Vec::new();
    let mut restow_packages = Vec::new();

    for operation in operation_groups {
        match operation.mode {
            StowMode::Stow => stow_packages.extend(operation.packages.clone()),
            StowMode::Delete => delete_packages.extend(operation.packages.clone()),
            StowMode::Restow => restow_packages.extend(operation.packages.clone()),
        }
    }

    mixed_packages(config, &delete_packages, &stow_packages, &restow_packages)
}

fn execute_operation_group(
    config: &Config,
    operation: &PackageOperation,
) -> Result<Vec<crate::stow::TargetActionReport>, RustowError> {
    let mut operation_config = config.clone();
    operation_config.mode = operation.mode.clone();
    operation_config.packages = operation.packages.clone();

    match &operation.mode {
        StowMode::Stow => stow_packages(&operation_config),
        StowMode::Delete => delete_packages(&operation_config),
        StowMode::Restow => restow_packages(&operation_config),
    }
}

fn reports_have_blocking_status(reports: &[crate::stow::TargetActionReport]) -> bool {
    reports.iter().any(|report| {
        matches!(
            report.status,
            crate::stow::TargetActionReportStatus::ConflictPrevented
                | crate::stow::TargetActionReportStatus::Failure(_)
        )
    })
}

/// Process and display action reports based on verbosity and simulation settings
fn process_reports(
    reports: &[crate::stow::TargetActionReport],
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
            crate::stow::TargetActionReportStatus::Success => {
                if config.verbosity > 1 || config.simulate {
                    if let Some(message) = &report.message {
                        eprintln!("{}", redactions.redact(message));
                    }
                }
            },
            crate::stow::TargetActionReportStatus::Skipped => {
                if config.verbosity > 0 || config.simulate {
                    if let Some(message) = &report.message {
                        eprintln!("{}", redactions.redact(message));
                    }
                }
            },
            crate::stow::TargetActionReportStatus::ConflictPrevented => {
                if let Some(message) = &report.message {
                    eprintln!("{}", redactions.redact(message));
                }
            },
            crate::stow::TargetActionReportStatus::Failure(error) => {
                eprintln!("ERROR: {}", redactions.redact(error));
                if let Some(message) = &report.message {
                    eprintln!("Details: {}", redactions.redact(message));
                }
            },
        }
    }

    // Summary
    if config.verbosity > 0 || config.simulate {
        let success_count = reports
            .iter()
            .filter(|r| matches!(r.status, crate::stow::TargetActionReportStatus::Success))
            .count();
        let skipped_count = reports
            .iter()
            .filter(|r| matches!(r.status, crate::stow::TargetActionReportStatus::Skipped))
            .count();
        let conflict_count = reports
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    crate::stow::TargetActionReportStatus::ConflictPrevented
                )
            })
            .count();
        let failure_count = reports
            .iter()
            .filter(|r| matches!(r.status, crate::stow::TargetActionReportStatus::Failure(_)))
            .count();

        eprintln!(
            "\nSummary: {} successful, {} skipped, {} conflicts, {} failures",
            success_count, skipped_count, conflict_count, failure_count
        );
    }
}

struct RedactionTable {
    replacements: Vec<(String, String)>,
}

impl RedactionTable {
    fn new(path_displays: &[PathDisplayOverride]) -> Self {
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

    fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
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

fn redact_runtime_error(error: RustowError, path_displays: &[PathDisplayOverride]) -> RustowError {
    let redactions = RedactionTable::new(path_displays);
    redact_rustow_error(error, &redactions)
}

fn redact_rustow_error(error: RustowError, redactions: &RedactionTable) -> RustowError {
    match error {
        RustowError::Config(error) => RustowError::Config(redact_config_error(error, redactions)),
        RustowError::Stow(error) => RustowError::Stow(redact_stow_error(error, redactions)),
        RustowError::Fs(error) => RustowError::Fs(redact_fs_error(error, redactions)),
        RustowError::Ignore(error) => RustowError::Ignore(redact_ignore_error(error, redactions)),
        RustowError::Cli(message) => RustowError::Cli(redactions.redact(&message).into_owned()),
        RustowError::InvalidPattern(pattern) => {
            RustowError::InvalidPattern(redactions.redact(&pattern).into_owned())
        },
        other => other,
    }
}

fn redact_config_error(error: ConfigError, redactions: &RedactionTable) -> ConfigError {
    match error {
        ConfigError::InvalidTargetDir(message) => {
            ConfigError::InvalidTargetDir(redactions.redact(&message).into_owned())
        },
        ConfigError::InvalidStowDir(message) => {
            ConfigError::InvalidStowDir(redactions.redact(&message).into_owned())
        },
        ConfigError::InvalidPackageName(message) => {
            ConfigError::InvalidPackageName(redactions.redact(&message).into_owned())
        },
        ConfigError::InvalidRegexPattern(message) => {
            ConfigError::InvalidRegexPattern(redactions.redact(&message).into_owned())
        },
        ConfigError::InvalidOperation(message) => {
            ConfigError::InvalidOperation(redactions.redact(&message).into_owned())
        },
        ConfigError::InvalidVerbosityLevel(level) => ConfigError::InvalidVerbosityLevel(level),
    }
}

fn redact_stow_error(error: StowError, redactions: &RedactionTable) -> StowError {
    match error {
        StowError::Conflict(message) => {
            StowError::Conflict(redactions.redact(&message).into_owned())
        },
        StowError::PackageNotFound(package) => {
            StowError::PackageNotFound(redactions.redact(&package).into_owned())
        },
        StowError::InvalidPackageStructure(message) => {
            StowError::InvalidPackageStructure(redactions.redact(&message).into_owned())
        },
        StowError::OperationFailed(message) => {
            StowError::OperationFailed(redactions.redact(&message).into_owned())
        },
    }
}

fn redact_ignore_error(error: IgnoreError, redactions: &RedactionTable) -> IgnoreError {
    match error {
        IgnoreError::LoadPatternsError(message) => {
            IgnoreError::LoadPatternsError(redactions.redact(&message).into_owned())
        },
        IgnoreError::InvalidPattern(pattern) => {
            IgnoreError::InvalidPattern(redactions.redact(&pattern).into_owned())
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
        let error = run(args).unwrap_err().to_string();

        assert!(error.contains(stow_dir.join("pkg").to_string_lossy().as_ref()));
        assert!(!error.contains("\"stow/pkg\""));
    }
}
