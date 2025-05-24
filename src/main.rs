mod error;
mod dotfiles;
mod ignore;
mod fs_utils;
mod cli;
// mod config; // Will be added later

use cli::Args;
use clap::Parser;
// use config::Config; // Will be used later

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
    println!("Parsed arguments: {:?}", args);

    // Later, we will do something like:
    // match Config::from_args(args) {
    //     Ok(config) => {
    //         println!("Constructed config: {:?}", config);
    //         // Proceed with stow logic based on config
    //     }
    //     Err(e) => {
    //         eprintln!("Error constructing config: {}", e);
    //         std::process::exit(1);
    //     }
    // }
}
