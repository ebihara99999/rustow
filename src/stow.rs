// Placeholder for stow module
// This file can be populated with stow logic later.

use crate::config::Config;
use crate::error::{RustowError, StowError, FsError};
use crate::fs_utils::{self};
use crate::dotfiles;
use std::path::PathBuf;

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

        // Load ignore patterns for the current package
        let current_ignore_patterns = crate::ignore::IgnorePatterns::load(
            &config.stow_dir, 
            Some(package_name),
            &config.home_dir
        ).map_err(|e: crate::ignore::IgnoreError| {
            RustowError::Ignore(crate::error::IgnoreError::LoadPatternsError(
                format!("Failed to load ignore patterns for package '{}': {:?}", package_name, e)
            ))
        })?;

        if config.verbosity > 1 { // Or a new specific verbosity level for ignore patterns
            println!("  Loaded ignore patterns for '{}':", package_name);
            for pattern in current_ignore_patterns.iter_patterns() {
                println!("    - {}", pattern.as_str());
            }
        }

        let raw_items = match fs_utils::walk_package_dir(&package_path) {
            Ok(items) => items,
            Err(RustowError::Fs(FsError::NotFound(_))) => {
                return Err(StowError::PackageNotFound(package_name.clone()).into());
            }
            Err(e) => return Err(e),
        };

        for raw_item in raw_items {
            let package_relative_path_str = raw_item.package_relative_path.to_str().unwrap_or("");
            let processed_path_str_after_dotfiles = dotfiles::process_item_name(
                package_relative_path_str,
                config.dotfiles,
            );

            // Check if item (or any parent) is ignored AFTER dotfiles processing
            let mut is_item_ignored = false;
            let path_to_check_ignore = PathBuf::from(&processed_path_str_after_dotfiles);

            // Check 1: Full path ignore patterns (relative to target_dir, effectively starts with /package_name/...)
            // This is how default patterns like `^/README.*` are expected to work.
            // The `processed_path_str_after_dotfiles` is package_relative_path, 
            // so we need to prepend `/` to match patterns like `^/README.md`
            let path_for_full_match = PathBuf::from("/").join(&path_to_check_ignore);
            for pattern in current_ignore_patterns.iter_patterns() {
                if pattern.is_match(path_for_full_match.to_str().unwrap_or_default()) {
                    if config.verbosity > 2 {
                        println!("    Ignoring '{}' due to full path pattern: {}", path_for_full_match.display(), pattern.as_str());
                    }
                    is_item_ignored = true;
                    break;
                }
            }
            if is_item_ignored { continue; }

            // Check 2: Basename ignore for files/dirs directly in the package root (e.g. .git)
            // And also check if any parent component of the item is ignored.
            let mut current_path_segment = PathBuf::new();
            for component in path_to_check_ignore.components() {
                current_path_segment.push(component.as_os_str());
                let basename_to_check = current_path_segment.file_name().unwrap_or_default().to_string_lossy();
                
                for pattern in current_ignore_patterns.iter_patterns() {
                    // Check if the pattern is intended for basenames (does not start with / or .)
                    // This is a heuristic. A more robust way might be needed if patterns become complex.
                    let pattern_str = pattern.as_str();
                    let is_basename_pattern = !pattern_str.starts_with('/') && !pattern_str.starts_with("./") && !pattern_str.contains('/');

                    if is_basename_pattern && pattern.is_match(&basename_to_check) {
                         if config.verbosity > 2 {
                            println!("    Ignoring '{}' (component '{}') due to basename pattern: {}", 
                                path_to_check_ignore.display(), basename_to_check, pattern.as_str());
                        }
                        is_item_ignored = true;
                        break;
                    }
                }
                if is_item_ignored { break; }
            }
            if is_item_ignored { continue; }

            let target_path = config.target_dir.join(&processed_path_str_after_dotfiles);

            let stow_item = StowItem {
                source_path: raw_item.absolute_path.clone(),
                package_relative_path: raw_item.package_relative_path.clone(),
                target_name_after_dotfiles_processing: processed_path_str_after_dotfiles,
            };
            
            let relative_to_target_parent = match target_path.parent() {
                Some(parent) => parent,
                None => &config.target_dir, // Should not happen for items inside target
            };
            let link_target = pathdiff::diff_paths(&raw_item.absolute_path, relative_to_target_parent)
                .unwrap_or_else(|| PathBuf::from("..").join(config.stow_dir.file_name().unwrap_or_default()).join(package_name).join(&raw_item.package_relative_path));

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
