mod error;
mod dotfiles;
mod ignore;
mod fs_utils;
mod cli;
mod config;
mod stow;

use cli::Args;
use clap::Parser;
use crate::config::{Config, StowMode};
use crate::stow::stow_packages;

fn main() {
    let args = Args::parse();
    
    match Config::from_args(args) {
        Ok(config) => {
            // Execute stow operation based on mode
            match config.mode {
                StowMode::Stow => {
                    match stow_packages(&config) {
                        Ok(actions) => {
                            if config.simulate {
                                println!("SIMULATION MODE - No changes will be made:");
                                for action in &actions {
                                    println!("Would create symlink: {} -> {:?}", 
                                        action.target_path.display(),
                                        action.link_target_path.as_ref().unwrap_or(&std::path::PathBuf::from("unknown"))
                                    );
                                }
                            } else {
                                println!("Planning to create {} symlinks", actions.len());
                                // TODO: Implement actual action execution
                                println!("Action execution not yet implemented");
                            }
                        }
                        Err(e) => {
                            eprintln!("Error during stow operation: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                StowMode::Delete => {
                    println!("Delete mode not yet implemented");
                    // TODO: Implement delete_packages function
                }
                StowMode::Restow => {
                    println!("Restow mode not yet implemented");
                    // TODO: Implement restow_packages function
                }
            }
        }
        Err(e) => {
            eprintln!("Error constructing config: {}", e);
            std::process::exit(1);
        }
    }
}
