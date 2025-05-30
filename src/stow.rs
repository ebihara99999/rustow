// Placeholder for stow module
// This file can be populated with stow logic later.

use crate::config::Config;
use crate::error::{RustowError, StowError, FsError};
use crate::fs_utils::{self};
use crate::dotfiles;
use std::path::PathBuf;
use crate::ignore::{self, IgnorePatterns};
use std::collections::HashMap;

// --- Action Planning Enums and Structs ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionType {
    CreateSymlink,      // Create a symbolic link
    DeleteSymlink,      // Delete a symbolic link
    CreateDirectory,    // Create a directory (for folding)
    DeleteDirectory,    // Delete an empty directory (during unstow)
    AdoptFile,          // Move a file from target to stow dir, then link (for --adopt)
    AdoptDirectory,     // Move a directory from target to stow dir, then link (for --adopt)
    Skip,               // Skip an operation (e.g., due to --defer or already correct state)
    Conflict,           // A conflict was detected that cannot be resolved by options
    // Maybe add more specific conflict types later if needed
}

// Re-define TargetAction based on the design document
// The existing one in tests/integration_tests.rs is a placeholder.
// We'll keep the existing one for now in stow.rs to avoid breaking tests immediately,
// but we should aim to replace it or make it compatible.
// For now, let's rename the existing one slightly to avoid direct collision if needed.
// Actually, let's define the proper one here. Tests will need to adapt.

#[derive(Debug, Clone)]
pub struct TargetAction {
    pub source_item: Option<StowItem>, // Original item from the package
    pub target_path: PathBuf,        // Absolute path in the target directory
    pub link_target_path: Option<PathBuf>, // Path the symlink should point to (relative to link's parent dir)
    pub action_type: ActionType,
    pub conflict_details: Option<String>, // Description of the conflict
}

// StowItem re-definition from design document
// The existing one in tests/integration_tests.rs is a placeholder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)] // Added PartialEq, Eq, Hash as per design doc
pub enum StowItemType {
    File,
    Directory,
    Symlink, // Represents a symlink within the package itself (less common for typical stow usage)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)] // Added PartialEq, Eq, Hash
pub struct StowItem {
    pub package_relative_path: PathBuf, // Path relative to the package root (e.g., "bin/script", "dot-config/nvim/init.vim")
    pub source_path: PathBuf,           // Absolute path to the item in the stow directory
    pub item_type: StowItemType,        // Type of the item in the stow package
    // Name of the item as it should appear in the target directory after dotfiles processing.
    // For "file.txt", it's "file.txt". For "dot-bashrc" with --dotfiles, it's ".bashrc".
    // For "dir/dot-foo", it's "dir/.foo".
    pub target_name_after_dotfiles_processing: PathBuf,
}

fn plan_actions(package_name: &str, config: &Config, current_ignore_patterns: &IgnorePatterns) -> Result<Vec<TargetAction>, RustowError> {
    let mut actions: Vec<TargetAction> = Vec::new();
    let package_path = config.stow_dir.join(package_name);

    if !fs_utils::path_exists(&package_path) {
        return Err(StowError::PackageNotFound(package_name.to_string()).into());
    }
    if !fs_utils::is_directory(&package_path) {
        return Err(StowError::InvalidPackageStructure(format!(
            "Package '{}' is not a directory at {:?}",
            package_name,
            package_path
        )).into());
    }

    // if config.verbosity > 1 {
    //     println!("  Loaded ignore patterns for '{}':", package_name);
    //     for pattern in current_ignore_patterns.iter_patterns() {
    //         println!("    - {}", pattern.as_str());
    //     }
    // }

    let raw_items = match fs_utils::walk_package_dir(&package_path) {
        Ok(items) => items,
        Err(RustowError::Fs(FsError::NotFound(_))) => {
            return Err(StowError::PackageNotFound(package_name.to_string()).into());
        }
        Err(e) => return Err(e),
    };    

    for raw_item in raw_items {
        let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
            raw_item.package_relative_path.to_str().unwrap_or(""), 
            config.dotfiles
        ));
        // if config.verbosity > 3 { // Add verbose logging for debugging
        //     println!(
        //         "    DEBUG: raw_item.package_relative_path: {:?}, processed_target_relative_path: {:?}",
        //         raw_item.package_relative_path,
        //         processed_target_relative_path
        //     );
        // }
        
        let path_for_ignore_check_fullpath = PathBuf::from("/").join(&processed_target_relative_path);
        let basename_for_ignore_check = processed_target_relative_path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        if ignore::is_ignored(&path_for_ignore_check_fullpath, &basename_for_ignore_check, current_ignore_patterns) {
            // if config.verbosity > 2 {
            //     println!("    Ignoring item '{:?}' (processed target: '{:?}') based on ignore patterns.", 
            //         raw_item.package_relative_path, processed_target_relative_path);
            // }
            continue;
        }

        let target_path_abs = config.target_dir.join(&processed_target_relative_path);

        let item_type_stow = match raw_item.item_type {
            fs_utils::RawStowItemType::File => StowItemType::File,
            fs_utils::RawStowItemType::Directory => StowItemType::Directory,
            fs_utils::RawStowItemType::Symlink => StowItemType::Symlink,
        };

        let stow_item_for_action = StowItem {
            source_path: raw_item.absolute_path.clone(),
            package_relative_path: raw_item.package_relative_path.clone(), // Original relative path
            target_name_after_dotfiles_processing: processed_target_relative_path.clone(), // Name after dotfiles processing
            item_type: item_type_stow,
        };
        
        let relative_to_target_parent = match target_path_abs.parent() {
            Some(parent) => parent,
            None => &config.target_dir,
        };
        let link_target_for_symlink = pathdiff::diff_paths(&stow_item_for_action.source_path, relative_to_target_parent)
            .unwrap_or_else(|| PathBuf::from("..").join(config.stow_dir.file_name().unwrap_or_default()).join(package_name).join(&stow_item_for_action.package_relative_path));

        let planned_action_type: ActionType;
        let mut conflict_details_str: Option<String> = None;
        let mut final_link_target: Option<PathBuf> = Some(link_target_for_symlink.clone());

        if fs_utils::path_exists(&target_path_abs) {
            planned_action_type = ActionType::Conflict;
            conflict_details_str = Some(format!("Target path {:?} already exists.", target_path_abs));
            final_link_target = None; 
        } else {
            match stow_item_for_action.item_type {
                StowItemType::Directory => {
                    planned_action_type = ActionType::CreateDirectory;
                    final_link_target = None; 
                }
                StowItemType::File | StowItemType::Symlink => {
                    planned_action_type = ActionType::CreateSymlink;
                }
            }
        }

        actions.push(TargetAction {
            source_item: Some(stow_item_for_action),
            target_path: target_path_abs,
            link_target_path: final_link_target,
            action_type: planned_action_type,
            conflict_details: conflict_details_str,
        });
    }

    let mut refined_actions = actions; // Modify in place or clone if necessary for safety

    for i in 0..refined_actions.len() {
        // If action is already a conflict (e.g. direct file collision), skip further parent checks for THIS action.
        if refined_actions[i].action_type == ActionType::Conflict {
            continue;
        }

        let current_action_target_path = refined_actions[i].target_path.clone();
        let mut parent_path_opt = current_action_target_path.parent();

        while let Some(parent_path) = parent_path_opt {
            if !parent_path.starts_with(&config.target_dir) || parent_path == config.target_dir {
                break; // Stop if we go above target_dir or reach target_dir itself
            }

            // Check if parent_path itself is a file (conflicting with the need for it to be a directory for the current item)
            if fs_utils::path_exists(parent_path) && !fs_utils::is_directory(parent_path) {
                // if config.verbosity > 1 {
                //     println!(
                //         "    CONFLICT (parent is file): Item {:?} conflicts because parent path {:?} is a file.",
                //         refined_actions[i].source_item.as_ref().map(|si| si.target_name_after_dotfiles_processing.clone()).unwrap_or_else(|| PathBuf::from("UnknownSource")),
                //         parent_path
                //     );
                // }
                refined_actions[i].action_type = ActionType::Conflict;
                refined_actions[i].conflict_details = Some(format!("Parent path {:?} is a file, but current item {:?} needs it to be a directory (or part of one).", parent_path, refined_actions[i].source_item.as_ref().map(|si| si.target_name_after_dotfiles_processing.clone()).unwrap_or_else(|| PathBuf::from("UnknownSource"))));
                refined_actions[i].link_target_path = None;
                break; // Conflict due to parent being a file, stop checking this item's parents
            }

            // Check if this parent_path is the target of another *already decided* conflicting action in the list
            // This requires careful iteration. If we iterate through `refined_actions` to find a conflicting parent,
            // the order of processing items might matter, or we might need a more stable way to check pre-existing conflicts.
            // For now, let's assume that if a parent directory *would be* a conflict (e.g. it's a file, or it's targeted by another stow package for a file),
            // then items within it are also conflicts.
            // The test case `test_plan_actions_basic_creation_and_conflict` specifically creates a dir_for_conflict
            // in the target, and an item dir_for_conflict/another_nested.txt in the package.
            // The `dir_for_conflict` (as a dir) should conflict with the package's `dir_for_conflict`.
            // Then `another_nested.txt` should also be a conflict because its parent is.

            // Simpler check: if any *other* processed StowItem maps to this exact parent_path and *that other item* is a file,
            // but the current item expects this parent_path to be a directory (which it must be if current item is nested).
            // This is part of the folding logic that is not fully implemented yet.

            // For the specific test case: target_dir_conflict_path = target_dir.join("dir_for_conflict") exists.
            // StowItem "dir_for_conflict" will have target_path_abs = target_dir_conflict_path. This will be a Conflict (dir vs dir, handled by basic check if types differ or adoption/override applies).
            // StowItem "dir_for_conflict/another_nested.txt" has target_path_abs = target_dir_conflict_path.join("another_nested.txt").
            // Its parent is target_dir_conflict_path.
            // We need to see if the action associated with target_dir_conflict_path is a conflict.
            
            let mut parent_is_target_of_conflict = false;
            for other_action_idx in 0..refined_actions.len() {
                // We only care if the *parent path* of the current action (item i) is the *target path* of another action (item j)
                // AND that other action (j) is a conflict.
                if refined_actions[other_action_idx].target_path == parent_path && 
                   refined_actions[other_action_idx].action_type == ActionType::Conflict {
                    parent_is_target_of_conflict = true;
                    break;
                }
            }

            if parent_is_target_of_conflict {
                 // if config.verbosity > 1 {
                 //    println!(
                 //        "    CONFLICT (parent conflict): Item {:?} conflicts because parent path {:?} is part of another conflicting action.",
                 //        refined_actions[i].source_item.as_ref().map(|si| si.target_name_after_dotfiles_processing.clone()).unwrap_or_else(|| PathBuf::from("UnknownSource")),
                 //        parent_path
                 //    );
                 // }
                refined_actions[i].action_type = ActionType::Conflict;
                refined_actions[i].conflict_details = Some(format!("Parent path {:?} is part of a conflicting item tree.", parent_path));
                refined_actions[i].link_target_path = None;
                break; // Conflict inherited from parent, stop checking this item's parents
            }
            
            if refined_actions[i].action_type == ActionType::Conflict { // If conflict was set by any check in this loop for this parent
                break;
            }
            parent_path_opt = parent_path.parent();
        }
    }

    // --- REMOVE: Inter-package conflict detection logic from plan_actions --- 
    // This logic will be moved to stow_packages after all actions are collected.

    Ok(refined_actions)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetActionReportStatus {
    Success,
    Skipped, // For simulation or if no action was needed
    ConflictPrevented, // For when a planned conflict action is "executed" (i.e. prevented)
    Failure(String), // Contains an error message
}

#[derive(Debug, Clone)]
pub struct TargetActionReport {
    pub original_action: TargetAction, // The action that was planned
    pub status: TargetActionReportStatus,
    pub message: Option<String>, // Additional details, e.g., error message or simulation output
}

fn execute_actions(actions: &[TargetAction], config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    let mut reports: Vec<TargetActionReport> = Vec::new();

    for action in actions {
        if config.simulate {
            let message = format!(
                "SIMULATE: Would perform {:?} on target {:?} (source: {:?}, link_target: {:?})",
                action.action_type,
                action.target_path,
                action.source_item.as_ref().map_or_else(|| PathBuf::from("N/A"), |si| si.source_path.clone()),
                action.link_target_path.as_ref().map_or_else(|| PathBuf::from("N/A"), |p| p.clone())
            );
            // if config.verbosity > 1 { // Corresponds to INFO level or higher
            //     println!("{}", message);
            // }
            reports.push(TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Skipped,
                message: Some(message),
            });
            continue;
        }

        // Placeholder for actual action execution logic
        // For now, we'll just report them as skipped if not simulating,
        // until we implement the actual file operations.
        // This will be replaced with calls to fs_utils based on action.action_type
        match action.action_type {
            ActionType::Conflict => {
                 reports.push(TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::ConflictPrevented,
                    message: Some(format!("CONFLICT: Operation prevented for target {:?}. Details: {}", action.target_path, action.conflict_details.as_deref().unwrap_or("N/A"))),
                });
            }
            ActionType::CreateDirectory => {
                match fs_utils::create_dir_all(&action.target_path) {
                    Ok(_) => {
                        // if config.verbosity > 1 {
                        //     println!("CREATED Directory: {:?}", action.target_path);
                        // }
                        reports.push(TargetActionReport {
                            original_action: action.clone(),
                            status: TargetActionReportStatus::Success,
                            message: Some(format!("Successfully created directory {:?}", action.target_path)),
                        });
                    }
                    Err(e) => {
                        // if config.verbosity > 0 {
                        //     eprintln!("ERROR creating directory {:?}: {}", action.target_path, e);
                        // }
                        reports.push(TargetActionReport {
                            original_action: action.clone(),
                            status: TargetActionReportStatus::Failure(e.to_string()),
                            message: Some(format!("Failed to create directory {:?}: {}", action.target_path, e)),
                        });
                    }
                }
            }
            ActionType::CreateSymlink => {
                if let Some(parent_dir) = action.target_path.parent() {
                    if !fs_utils::path_exists(parent_dir) {
                        if let Err(e) = fs_utils::create_dir_all(parent_dir) {
                            // if config.verbosity > 0 {
                            //     eprintln!(
                            //         "ERROR creating parent directory {:?} for symlink {:?}: {}",
                            //         parent_dir, action.target_path, e
                            //     );
                            // }
                            reports.push(TargetActionReport {
                                original_action: action.clone(),
                                status: TargetActionReportStatus::Failure(format!(
                                    "Failed to create parent directory {:?} for symlink: {}",
                                    parent_dir, e
                                )),
                                message: Some(format!(
                                    "Failed to create parent directory {:?} for symlink {:?}: {}",
                                    parent_dir, action.target_path, e
                                )),
                            });
                            continue; // Skip to next action if parent dir creation failed
                        }
                        // if config.verbosity > 1 {
                        //     println!("CREATED Parent Directory: {:?} for symlink {:?}", parent_dir, action.target_path);
                        // }
                    }
                }

                match &action.link_target_path {
                    Some(link_target) => {
                        match fs_utils::create_symlink(&action.target_path, link_target) {
                            Ok(_) => {
                                // if config.verbosity > 1 {
                                //     println!(
                                //         "CREATED Symlink: {:?} -> {:?}",
                                //         action.target_path,
                                //         link_target
                                //     );
                                // }
                                reports.push(TargetActionReport {
                                    original_action: action.clone(),
                                    status: TargetActionReportStatus::Success,
                                    message: Some(format!(
                                        "Successfully created symlink {:?} -> {:?}",
                                        action.target_path,
                                        link_target
                                    )),
                                });
                            }
                            Err(e) => {
                                // if config.verbosity > 0 {
                                //     eprintln!(
                                //         "ERROR creating symlink {:?} -> {:?}: {}",
                                //         action.target_path, link_target, e
                                //     );
                                // }
                                reports.push(TargetActionReport {
                                    original_action: action.clone(),
                                    status: TargetActionReportStatus::Failure(e.to_string()),
                                    message: Some(format!(
                                        "Failed to create symlink {:?} -> {:?}: {}",
                                        action.target_path, link_target, e
                                    )),
                                });
                            }
                        }
                    }
                    None => {
                        // This case should ideally not happen for CreateSymlink action type
                        // if plan_actions is correct.
                        // if config.verbosity > 0 {
                        //     eprintln!(
                        //         "ERROR: CreateSymlink action for {:?} is missing link_target_path.",
                        //         action.target_path
                        //     );
                        // }
                        reports.push(TargetActionReport {
                            original_action: action.clone(),
                            status: TargetActionReportStatus::Failure(
                                "CreateSymlink action missing link_target_path".to_string(),
                            ),
                            message: Some(format!(
                                "CreateSymlink action for {:?} is missing link_target_path.",
                                action.target_path
                            )),
                        });
                    }
                }
            }
            // TODO: Implement other action types
            _ => {
                 reports.push(TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Skipped, // Placeholder
                    message: Some(format!("Action {:?} not yet implemented for target {:?}", action.action_type, action.target_path)),
                });
            }
        }
    }
    Ok(reports)
}

pub fn stow_packages(config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    let mut all_planned_actions: Vec<TargetAction> = Vec::new();

    if config.packages.is_empty() {
        // If there are no packages, return an empty list of reports directly.
        return Ok(Vec::new());
    }

    for package_name in &config.packages {
        // Load ignore patterns for the current package
        let current_ignore_patterns = match IgnorePatterns::load(
            &config.stow_dir,
            Some(package_name),
            &config.home_dir, // Assuming home_dir is part of Config for global ignores
        ) {
            Ok(patterns) => patterns,
            Err(e) => {
                // Handle IgnoreError specifically if it's not already a RustowError
                // For example, by wrapping it.
                // Here, we assume IgnoreError can be converted or is a variant of RustowError
                // If IgnoreError::LoadPatternsError or InvalidPattern is the type from ignore.rs,
                // we need to map it to RustowError::Ignore.
                // The current IgnoreError enum in ignore.rs is already well-defined.
                return Err(RustowError::Ignore(crate::error::IgnoreError::LoadPatternsError(
                    format!("Failed to load ignore patterns for package '{}': {:?}", package_name, e)
                )));
            }
        };

        match plan_actions(package_name, config, &current_ignore_patterns) { // Pass loaded patterns
            Ok(package_actions) => all_planned_actions.extend(package_actions),
            Err(e) => return Err(e), 
        }
    }

    // --- START: Inter-package conflict detection (Moved to stow_packages) ---
    let mut final_actions = all_planned_actions; // Work on a mutable copy or the original if appropriate
    let mut target_map: HashMap<PathBuf, Vec<usize>> = HashMap::new();

    for (index, action) in final_actions.iter().enumerate() {
        if action.action_type != ActionType::Conflict { // Only consider non-conflicting actions for new conflicts
            target_map.entry(action.target_path.clone()).or_default().push(index);
        }
    }

    for (_target_path, action_indices) in target_map {
        if action_indices.len() > 1 {
            for index in action_indices {
                let conflicting_action = &mut final_actions[index];
                conflicting_action.action_type = ActionType::Conflict;
                if conflicting_action.conflict_details.is_none() {
                    let sources_involved = conflicting_action.source_item.as_ref()
                        .map(|si| si.source_path.display().to_string())
                        .unwrap_or_else(|| "Unknown source".to_string());
                    conflicting_action.conflict_details = Some(format!(
                        "Inter-package conflict: Multiple packages attempt to manage target path {:?}. Source: {}.",
                        conflicting_action.target_path,
                        sources_involved
                    ));
                }
                // if config.verbosity > 0 {
                //     println!(
                //         "    INTER-PACKAGE CONFLICT (stow_packages): Target path {:?} is targeted by multiple packages. Action for source '{}' marked as Conflict. Details: {:?}",
                //         conflicting_action.target_path,
                //         conflicting_action.source_item.as_ref().map(|si| si.source_path.display().to_string()).unwrap_or_else(|| "N/A".to_string()),
                //         conflicting_action.conflict_details
                //     );
                // }
                conflicting_action.link_target_path = None;
            }
        }
    }
    // --- END: Inter-package conflict detection ---

    // --- START: Propagate conflicts to child items ---
    // (1) 収集フェーズ: どのアイテムが親の衝突の影響を受けるか特定する
    let mut child_conflict_updates: Vec<(usize, String)> = Vec::new(); // (index, conflict_message)

    // 最初に、直接的な衝突（stow_packagesの最初のループで設定されたもの）を把握
    let parent_conflicts: std::collections::HashSet<PathBuf> = final_actions.iter()
        .filter(|action| action.action_type == ActionType::Conflict)
        .map(|action| action.target_path.clone())
        .collect();

    // 次に、子が親の衝突の影響を受けるかチェックする
    // このループでは final_actions を変更しない
    for (i, action) in final_actions.iter().enumerate() {
        if action.action_type == ActionType::Conflict {
            continue; // すでに直接的な衝突があるものはスキップ
        }

        if let Some(parent_target_path) = action.target_path.parent() {
            // action.target_path の親が parent_conflicts に含まれていたら、
            // この action も衝突とみなす
            if parent_conflicts.contains(parent_target_path) {
                let conflict_message = format!(
                    "Parent path {:?} is in conflict, so child item {:?} is also a conflict.",
                    parent_target_path,
                    action.source_item.as_ref().map(|si| si.target_name_after_dotfiles_processing.clone()).unwrap_or_else(|| PathBuf::from("UnknownSource"))
                );
                child_conflict_updates.push((i, conflict_message));
            }
        }
    }

    // (2) 更新フェーズ: 収集した情報に基づいて final_actions を更新
    for (index_to_update, conflict_message) in child_conflict_updates {
        let action_to_update = &mut final_actions[index_to_update];
        if action_to_update.action_type != ActionType::Conflict { // まだ衝突マークされていなければ更新
            action_to_update.action_type = ActionType::Conflict;
            action_to_update.conflict_details = Some(conflict_message.clone());
            action_to_update.link_target_path = None;

            // if config.verbosity > 1 {
            //     println!(
            //         "    PROPAGATED CONFLICT (stow_packages): Item {:?} at {:?} marked as Conflict. Reason: {}",
            //         action_to_update.source_item.as_ref().map(|si| si.target_name_after_dotfiles_processing.clone()).unwrap_or_else(|| PathBuf::from("UnknownSource")),
            //         action_to_update.target_path,
            //         conflict_message
            //     );
            // }
        }
    }
    // --- END: Propagate conflicts to child items ---

    execute_actions(&final_actions, config)
} 
