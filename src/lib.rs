pub mod cli;
pub mod config;
mod diagnostics;
pub mod dotfiles;
pub mod error;
pub mod fs_utils;
pub mod ignore;
mod path_utils;
pub mod stow;
mod stow_types;
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
        if operation_groups.is_empty() {
            reject_ambiguous_mixed_args(&args)?;
        }

        match Config::from_args_with_path_displays(args, &mut path_displays) {
            Ok(config) => {
                path_displays.push(PathDisplayOverride::new(
                    config.home_dir.join(".stow-global-ignore"),
                    "~/.stow-global-ignore".to_string(),
                ));

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
                diagnostics::process_reports(&reports, &config, diagnostic_path_displays);

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
            Err(e) => Err(e),
        }
    })();

    if redact_diagnostics {
        result.map_err(|error| diagnostics::redact_runtime_error(error, &path_displays))
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
    reports.iter().any(|report| report.status.is_blocking())
}
