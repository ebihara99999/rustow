pub mod error;
pub mod dotfiles;
pub mod ignore;
pub mod fs_utils;
pub mod cli;
pub mod config;
pub mod stow;

use crate::cli::Args;
use crate::config::{Config, StowMode};
use crate::error::RustowError;
use crate::stow::{stow_packages, delete_packages, restow_packages};

/// Runs the rustow application logic.
pub fn run(args: Args) -> Result<(), RustowError> {
    // eprintln!("stderr: Successfully parsed args in lib::run: {:?}", args.clone());

    match Config::from_args(args) {
        Ok(config) => {
            // eprintln!("stderr: Successfully constructed config in lib::run: {:?}", config);

            let _reports = match config.mode {
                StowMode::Stow => stow_packages(&config)?,
                StowMode::Delete => delete_packages(&config)?,
                StowMode::Restow => restow_packages(&config)?,
            };

            // TODO: Process reports for logging/output
            Ok(())
        }
        Err(e) => {
            // eprintln!("stderr: Error constructing config in lib::run: {}", e);
            Err(e)
        }
    }
}
