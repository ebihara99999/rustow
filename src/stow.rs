// Placeholder for stow module
// This file can be populated with stow logic later.

use crate::config::Config;
use crate::error::{RustowError, StowError, FsError};
use crate::fs_utils::{self, RawStowItem, RawStowItemType};
use crate::dotfiles;
use std::path::{Path, PathBuf};

// 仮の TargetAction 構造体（tests/integration_tests.rs で使われているため）
// 本来は stow モジュール内でちゃんと定義するのだ
#[derive(Debug, Clone)]
pub struct TargetAction {
    pub source_item: Option<StowItem>,
    pub target_path: PathBuf,
    pub link_target_path: Option<PathBuf>,
    // pub action_type: ActionType, // ActionType も仮で定義が必要になるかもしれないのだ
    pub conflict_details: Option<String>,
}

// 仮の StowItem 構造体
#[derive(Debug, Clone)]
pub struct StowItem {
    pub package_relative_path: PathBuf, // Original path in package
    pub source_path: PathBuf,           // Absolute path to source item in stow dir
    pub target_name_after_dotfiles_processing: String, // Name in target dir after dot- prefix conversion
    // pub item_type: RawStowItemType, // Potentially add this later
}


pub fn stow_packages(config: &Config) -> Result<Vec<TargetAction>, RustowError> {
    let mut actions: Vec<TargetAction> = Vec::new();

    if config.packages.is_empty() {
        return Ok(actions); // No packages, no actions
    }

    for package_name in &config.packages {
        let package_path = config.stow_dir.join(package_name);

        if !fs_utils::path_exists(&package_path) {
            // For test_nonexistent_package to pass, we need to return an error here.
            return Err(StowError::PackageNotFound(package_name.clone()).into());
        }
        if !fs_utils::is_directory(&package_path) {
            return Err(StowError::InvalidPackageStructure(format!(
                "Package '{}' is not a directory at {:?}",
                package_name,
                package_path
            )).into());
        }

        let raw_items = match fs_utils::walk_package_dir(&package_path) {
            Ok(items) => items,
            Err(RustowError::Fs(FsError::NotFound(_))) => {
                 // This case should ideally be caught by the path_exists check above,
                 // but as a safeguard or if walk_package_dir has subtle differences.
                return Err(StowError::PackageNotFound(package_name.clone()).into());
            }
            Err(e) => return Err(e), // Propagate other errors from walk_package_dir
        };

        for raw_item in raw_items {
            let processed_item_name_str = dotfiles::process_item_name(
                raw_item.package_relative_path.to_str().unwrap_or(""), // Convert PathBuf to &str
                config.dotfiles,
            );
            // The processed_item_name_str is the full relative path after dot- processing.
            // For target_path, we join this with target_dir.
            let target_path = config.target_dir.join(&processed_item_name_str);

            let stow_item = StowItem {
                source_path: raw_item.absolute_path.clone(),
                package_relative_path: raw_item.package_relative_path.clone(),
                target_name_after_dotfiles_processing: processed_item_name_str, // Store the processed name
            };
            
            let relative_to_target_parent = match target_path.parent() {
                Some(parent) => parent,
                None => &config.target_dir, // Should not happen for items inside target
            };
            let link_target = pathdiff::diff_paths(&raw_item.absolute_path, relative_to_target_parent)
                .unwrap_or_else(|| PathBuf::from("..").join(config.stow_dir.file_name().unwrap_or_default()).join(package_name).join(&stow_item.package_relative_path)); // Use original relative for source

            actions.push(TargetAction {
                source_item: Some(stow_item),
                target_path,
                link_target_path: Some(link_target),
                conflict_details: None,
            });
        }
    }

    Ok(actions)
} 
