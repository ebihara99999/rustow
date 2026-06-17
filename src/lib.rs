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
use crate::error::{ConfigError, RustowError, StowError};
use crate::stow::{
    delete_packages, mixed_packages, restow_packages, stow_packages,
    validate_package_for_operation_with_display,
};
use std::path::{Component, Path};

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
    )
}

#[doc(hidden)]
pub fn run_runtime_parsed(parsed_args: RuntimeParsedArgs) -> Result<(), RustowError> {
    let (parsed_args, path_displays) = parsed_args.into_parts();
    run_with_operation_groups_and_path_displays(
        parsed_args.args,
        parsed_args.operation_groups,
        path_displays,
    )
}

/// Runs rustow with operation groups reconstructed from CLI argument order.
pub fn run_with_operation_groups(
    args: Args,
    operation_groups: Vec<OperationGroup>,
) -> Result<(), RustowError> {
    run_with_operation_groups_and_path_displays(args, operation_groups, Vec::new())
}

fn run_with_operation_groups_and_path_displays(
    args: Args,
    operation_groups: Vec<OperationGroup>,
    mut path_displays: Vec<PathDisplayOverride>,
) -> Result<(), RustowError> {
    // eprintln!("stderr: Successfully parsed args in lib::run: {:?}", args.clone());
    if operation_groups.is_empty() {
        reject_ambiguous_mixed_args(&args)?;
    }

    match Config::from_args_with_path_displays(args, &mut path_displays) {
        Ok(config) => {
            // eprintln!("stderr: Successfully constructed config in lib::run: {:?}", config);

            let package_operations = package_operations_for_config(&config, operation_groups);
            preflight_package_operations(&config, &package_operations, &path_displays)?;
            let reports = execute_config_operations(&config, &package_operations)?;

            // Process reports for logging/output
            process_reports(&reports, &config, &path_displays);

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
                        eprintln!("{}", redact_report_text(message, path_displays));
                    }
                }
            },
            crate::stow::TargetActionReportStatus::Skipped => {
                if config.verbosity > 0 || config.simulate {
                    if let Some(message) = &report.message {
                        eprintln!("{}", redact_report_text(message, path_displays));
                    }
                }
            },
            crate::stow::TargetActionReportStatus::ConflictPrevented => {
                if let Some(message) = &report.message {
                    eprintln!("{}", redact_report_text(message, path_displays));
                }
            },
            crate::stow::TargetActionReportStatus::Failure(error) => {
                eprintln!("ERROR: {}", redact_report_text(error, path_displays));
                if let Some(message) = &report.message {
                    eprintln!("Details: {}", redact_report_text(message, path_displays));
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

fn redact_report_text(text: &str, path_displays: &[PathDisplayOverride]) -> String {
    let mut replacements: Vec<(String, String)> = path_displays
        .iter()
        .filter_map(|override_path| {
            let path = override_path.path.display().to_string();
            if path.is_empty() || path == override_path.display {
                return None;
            }
            Some((path, override_path.display.clone()))
        })
        .collect();
    replacements.sort_by(|(left, _), (right, _)| right.len().cmp(&left.len()));

    replacements
        .into_iter()
        .fold(text.to_string(), |redacted, (path, display)| {
            redacted.replace(&path, &display)
        })
}
