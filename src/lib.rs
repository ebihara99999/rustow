// This file is created to make cargo test discover tests in src/cli.rs and other modules
// when tests for src/main.rs are disabled in Cargo.toml

pub mod error;
pub mod dotfiles;
pub mod ignore;
pub mod fs_utils;
pub mod cli;
// pub mod config; // Commented out since config.rs doesn't exist yet
pub mod config;
pub mod stow; 
