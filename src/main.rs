pub mod error;
pub mod dotfiles;
pub mod ignore;
pub mod fs_utils;
pub mod cli;
pub mod config; // Enabled config module
pub mod stow;

use clap::Parser;
use rustow::cli::Args;

fn main() {
    // Print all arguments received by main to stderr for debugging purposes.
    // These can be removed or commented out in production.
    // let all_args: Vec<String> = std::env::args().collect();
    // eprintln!("stderr: main received args: {:?}", all_args);
    // if all_args.len() == 1 && all_args[0].contains("rustow") {
    //     eprintln!("stderr: Attempting to parse arguments for main binary execution context...");
    // }
    // let raw_cli_args: Vec<String> = std::env::args().collect(); 
    // eprintln!("stderr: Raw CLI args (non-test execution): {:?}", raw_cli_args);

    let args = Args::parse();
    // eprintln!("stderr: Successfully parsed args in main: {:?}", args.clone()); 

    if let Err(e) = rustow::run(args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
