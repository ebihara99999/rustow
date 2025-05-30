mod error;
mod dotfiles;
mod ignore;
mod fs_utils;
mod cli;
mod config; // Enabled config module

use cli::Args;
use clap::Parser;
use crate::config::Config; // Enabled Config import

fn main() {
    // Print all arguments received by main to stderr
    let all_args: Vec<String> = std::env::args().collect();
    eprintln!("stderr: main received args: {:?}", all_args);

    if all_args.len() == 1 && all_args[0].contains("rustow") {
        eprintln!("stderr: Attempting to parse arguments for main binary execution context...");
    }

    // Non-test execution continues here
    let raw_cli_args: Vec<String> = std::env::args().collect(); // Still useful for non-test debugging
    eprintln!("stderr: Raw CLI args (non-test execution): {:?}", raw_cli_args);

    let args = Args::parse();
    // println!("Parsed arguments: {:?}", args);
    eprintln!("stderr: Successfully parsed args in main: {:?}", args.clone()); // Clone args for eprintln if needed after move

    match Config::from_args(args) {
        Ok(config) => {
            // println!("Constructed config: {:?}", config);
            eprintln!("stderr: Successfully constructed config: {:?}", config);
            // Proceed with stow logic based on config
        }
        Err(e) => {
            eprintln!("stderr: Error constructing config: {}", e);
            std::process::exit(1);
        }
    }
}
