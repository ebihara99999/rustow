pub mod cli;
pub mod config;
pub mod dotfiles;
pub mod error;
pub mod fs_utils;
pub mod ignore;
pub mod stow;

use crate::cli::Args;
use crate::config::{Config, StowMode};
use crate::error::RustowError;
use crate::stow::{delete_packages, restow_packages, stow_packages};

/// Runs the rustow application logic.
pub fn run(args: Args) -> Result<(), RustowError> {
    // eprintln!("stderr: Successfully parsed args in lib::run: {:?}", args.clone());

    match Config::from_args(args) {
        Ok(config) => {
            // eprintln!("stderr: Successfully constructed config in lib::run: {:?}", config);

            let reports = match config.mode {
                StowMode::Stow => stow_packages(&config)?,
                StowMode::Delete => delete_packages(&config)?,
                StowMode::Restow => restow_packages(&config)?,
            };

            // Process reports for logging/output
            process_reports(&reports, &config);
            Ok(())
        },
        Err(e) => {
            // eprintln!("stderr: Error constructing config in lib::run: {}", e);
            Err(e)
        },
    }
}

/// Process and display action reports based on verbosity and simulation settings
fn process_reports(reports: &[crate::stow::TargetActionReport], config: &Config) {
    if reports.is_empty() {
        if config.verbosity > 0 {
            println!("No actions to perform.");
        }
        return;
    }

    for report in reports {
        match &report.status {
            crate::stow::TargetActionReportStatus::Success => {
                if config.verbosity > 1 || config.simulate {
                    if let Some(message) = &report.message {
                        println!("{}", message);
                    }
                }
            },
            crate::stow::TargetActionReportStatus::Skipped => {
                if config.verbosity > 0 || config.simulate {
                    if let Some(message) = &report.message {
                        println!("{}", message);
                    }
                }
            },
            crate::stow::TargetActionReportStatus::ConflictPrevented => {
                if let Some(message) = &report.message {
                    eprintln!("{}", message);
                }
            },
            crate::stow::TargetActionReportStatus::Failure(error) => {
                eprintln!("ERROR: {}", error);
                if let Some(message) = &report.message {
                    eprintln!("Details: {}", message);
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

        println!(
            "\nSummary: {} successful, {} skipped, {} conflicts, {} failures",
            success_count, skipped_count, conflict_count, failure_count
        );
    }
}
