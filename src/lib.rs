pub mod error;
pub mod dotfiles;
pub mod ignore;
pub mod fs_utils;
pub mod cli;
pub mod config;
pub mod stow;

use crate::cli::Args;
use crate::config::Config;
use crate::error::RustowError;
use crate::stow::stow_packages; // Assuming stow_packages is available

/// Runs the rustow application logic.
pub fn run(args: Args) -> Result<(), RustowError> {
    // eprintln!("stderr: Successfully parsed args in lib::run: {:?}", args.clone());

    match Config::from_args(args) {
        Ok(config) => {
            // eprintln!("stderr: Successfully constructed config in lib::run: {:?}", config);
            stow_packages(&config)?; // Call the stow_packages function
            Ok(())
        }
        Err(e) => {
            // eprintln!("stderr: Error constructing config in lib::run: {}", e);
            Err(e)
        }
    }
} 
