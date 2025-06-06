// Placeholder for stow module
// This file can be populated with stow logic later.

use crate::config::Config;
use crate::error::{RustowError, StowError, FsError};
use crate::fs_utils::{self};
use crate::dotfiles;
use std::path::{Path, PathBuf};
use crate::ignore::{self, IgnorePatterns};

// Define modules inline for now
mod conflict_resolver {
    use crate::config::Config;
    use crate::stow::{TargetAction, ActionType};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Handles conflict detection and resolution between packages
    pub struct ConflictResolver<'a> {
        config: &'a Config,
    }

    impl<'a> ConflictResolver<'a> {
        pub fn new(config: &'a Config) -> Self {
            Self { config }
        }

        /// Detect and resolve inter-package conflicts
        pub fn resolve_inter_package_conflicts(&self, actions: &mut [TargetAction]) {
            let target_map = self.build_target_map(actions);
            self.mark_conflicting_actions(actions, target_map);
        }

        /// Propagate conflicts to child items
        pub fn propagate_conflicts_to_children(&self, actions: &mut [TargetAction]) {
            let parent_conflicts = self.collect_parent_conflicts(actions);
            let child_updates = self.find_child_conflicts(actions, &parent_conflicts);
            self.apply_child_conflict_updates(actions, child_updates);
        }

        fn build_target_map(&self, actions: &[TargetAction]) -> HashMap<PathBuf, Vec<usize>> {
            let mut target_map: HashMap<PathBuf, Vec<usize>> = HashMap::new();

            for (index, action) in actions.iter().enumerate() {
                if action.action_type != ActionType::Conflict {
                    target_map.entry(action.target_path.clone()).or_default().push(index);
                }
            }

            target_map
        }

        fn mark_conflicting_actions(&self, actions: &mut [TargetAction], target_map: HashMap<PathBuf, Vec<usize>>) {
            for (_target_path, action_indices) in target_map {
                if action_indices.len() > 1 {
                    for index in action_indices {
                        self.mark_action_as_conflict(&mut actions[index]);
                    }
                }
            }
        }

        fn mark_action_as_conflict(&self, action: &mut TargetAction) {
            action.action_type = ActionType::Conflict;
            if action.conflict_details.is_none() {
                let sources_involved = action.source_item.as_ref()
                    .map(|si| si.source_path.display().to_string())
                    .unwrap_or_else(|| "Unknown source".to_string());
                action.conflict_details = Some(format!(
                    "Inter-package conflict: Multiple packages attempt to manage target path {:?}. Source: {}.",
                    action.target_path,
                    sources_involved
                ));
            }
            action.link_target_path = None;
        }

        fn collect_parent_conflicts(&self, actions: &[TargetAction]) -> std::collections::HashSet<PathBuf> {
            actions.iter()
                .filter(|action| action.action_type == ActionType::Conflict)
                .map(|action| action.target_path.clone())
                .collect()
        }

        fn find_child_conflicts(&self, actions: &[TargetAction], parent_conflicts: &std::collections::HashSet<PathBuf>) -> Vec<(usize, String)> {
            let mut child_conflict_updates = Vec::new();

            for (i, action) in actions.iter().enumerate() {
                if action.action_type == ActionType::Conflict {
                    continue;
                }

                if let Some(parent_target_path) = action.target_path.parent() {
                    if parent_conflicts.contains(parent_target_path) {
                        let conflict_message = format!(
                            "Parent path {:?} is in conflict, so child item {:?} is also a conflict.",
                            parent_target_path,
                            action.source_item.as_ref()
                                .map(|si| si.target_name_after_dotfiles_processing.clone())
                                .unwrap_or_else(|| PathBuf::from("UnknownSource"))
                        );
                        child_conflict_updates.push((i, conflict_message));
                    }
                }
            }

            child_conflict_updates
        }

        fn apply_child_conflict_updates(&self, actions: &mut [TargetAction], child_updates: Vec<(usize, String)>) {
            for (index_to_update, conflict_message) in child_updates {
                let action_to_update = &mut actions[index_to_update];
                if action_to_update.action_type != ActionType::Conflict {
                    action_to_update.action_type = ActionType::Conflict;
                    action_to_update.conflict_details = Some(conflict_message);
                    action_to_update.link_target_path = None;
                }
            }
        }
    }
}

mod pattern_matcher {
    use crate::config::Config;
    use crate::stow::ActionType;
    use std::path::Path;
    use std::path::PathBuf;

    /// Handles pattern matching for override and defer options
    pub struct PatternMatcher<'a> {
        config: &'a Config,
    }

    impl<'a> PatternMatcher<'a> {
        pub fn new(config: &'a Config) -> Self {
            Self { config }
        }

        /// Check patterns and return appropriate action type and message
        pub fn check_patterns(&self, target_path_abs: &Path, link_target: PathBuf) -> Option<(ActionType, String, Option<PathBuf>)> {
            let target_relative_path = match target_path_abs.strip_prefix(&self.config.target_dir) {
                Ok(path) => path,
                Err(_) => return None,
            };

            let target_path_str = target_relative_path.to_string_lossy();

            // Check defer patterns first (defer takes precedence over override)
            if let Some(defer_pattern) = self.find_matching_defer_pattern(&target_path_str) {
                return Some((
                    ActionType::Skip,
                    format!("Deferred due to pattern match: {}", defer_pattern.as_str()),
                    None
                ));
            }

            // Check override patterns
            if let Some(override_pattern) = self.find_matching_override_pattern(&target_path_str) {
                return Some((
                    ActionType::CreateSymlink,
                    format!("Overriding existing file due to pattern match: {}", override_pattern.as_str()),
                    Some(link_target)
                ));
            }

            None
        }

        fn find_matching_defer_pattern(&self, target_path_str: &str) -> Option<&regex::Regex> {
            self.config.defers.iter().find(|pattern| pattern.is_match(target_path_str))
        }

        fn find_matching_override_pattern(&self, target_path_str: &str) -> Option<&regex::Regex> {
            self.config.overrides.iter().find(|pattern| pattern.is_match(target_path_str))
        }
    }
}

use conflict_resolver::ConflictResolver;
use pattern_matcher::PatternMatcher;

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
    let package_path = config.stow_dir.join(package_name);
    validate_package_path(&package_path, package_name)?;

    let raw_items = load_package_items(&package_path, package_name)?;
    let mut actions = Vec::new();

    // Process each item to create initial actions
    for raw_item in raw_items {
        if let Some(action) = process_item_for_stow(raw_item, config, current_ignore_patterns, package_name)? {
            actions.push(action);
        }
    }

    // Refine actions by checking for parent conflicts
    refine_actions_for_parent_conflicts(&mut actions, config);

    Ok(actions)
}

/// Process a single item for stowing, returning an action if needed
fn process_item_for_stow(
    raw_item: fs_utils::RawStowItem,
    config: &Config,
    current_ignore_patterns: &IgnorePatterns,
    package_name: &str
) -> Result<Option<TargetAction>, RustowError> {
    let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
        raw_item.package_relative_path.to_str().unwrap_or(""), 
        config.dotfiles
    ));

    // Check if item should be ignored
    if should_ignore_item(&processed_target_relative_path, current_ignore_patterns) {
        return Ok(None);
    }

    let target_path_abs = config.target_dir.join(&processed_target_relative_path);
    let stow_item = create_stow_item_from_raw(raw_item, processed_target_relative_path);

    let link_target_for_symlink = calculate_link_target(&stow_item, &target_path_abs, config, package_name);
    let action = plan_stow_action_for_item(&stow_item, &target_path_abs, link_target_for_symlink, config)?;

    Ok(Some(action))
}

/// Calculate the relative path for a symlink target
fn calculate_link_target(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config,
    package_name: &str
) -> PathBuf {
    let relative_to_target_parent = target_path_abs
        .parent()
        .unwrap_or(&config.target_dir);

    pathdiff::diff_paths(&stow_item.source_path, relative_to_target_parent)
        .unwrap_or_else(|| {
            PathBuf::from("..")
                .join(config.stow_dir.file_name().unwrap_or_default())
                .join(package_name)
                .join(&stow_item.package_relative_path)
        })
}

/// Plan the appropriate stow action for a single item
fn plan_stow_action_for_item(
    stow_item: &StowItem,
    target_path_abs: &Path,
    link_target_for_symlink: PathBuf,
    config: &Config
) -> Result<TargetAction, RustowError> {
    let (action_type, conflict_details, final_link_target) = if fs_utils::path_exists(target_path_abs) {
        // Target path exists, need to check for conflicts and resolution options
        handle_existing_target_conflict(stow_item, target_path_abs, link_target_for_symlink, config)?
    } else {
        // Target path doesn't exist, proceed with normal action
        match stow_item.item_type {
            StowItemType::Directory => (ActionType::CreateDirectory, None, None),
            StowItemType::File | StowItemType::Symlink => {
                (ActionType::CreateSymlink, None, Some(link_target_for_symlink))
            }
        }
    };

    Ok(TargetAction {
        source_item: Some(stow_item.clone()),
        target_path: target_path_abs.to_path_buf(),
        link_target_path: final_link_target,
        action_type,
        conflict_details,
    })
}

/// Handle conflicts when target path already exists
fn handle_existing_target_conflict(
    stow_item: &StowItem,
    target_path_abs: &Path,
    link_target_for_symlink: PathBuf,
    config: &Config
) -> Result<(ActionType, Option<String>, Option<PathBuf>), RustowError> {


    // Check if target is a directory and we're trying to create a directory
    if fs_utils::is_directory(target_path_abs) && stow_item.item_type == StowItemType::Directory {
        // Check if the existing directory contains any non-stow managed files
        if let Ok(entries) = std::fs::read_dir(target_path_abs) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let entry_path = entry.path();
                    // If there's any file that's not a stow-managed symlink, it's a conflict
                    if !fs_utils::is_symlink(&entry_path) {
                        return Ok((ActionType::Conflict, 
                                  Some(format!("Directory {:?} contains non-stow managed files", target_path_abs)), 
                                  None));
                    }

                                         // Check if it's a stow-managed symlink
                     match fs_utils::is_stow_symlink(&entry_path, &config.stow_dir) {
                         Ok(Some(_)) => {
                             // It's a stow-managed symlink, continue checking
                         }
                         Ok(None) | Err(_) => {
                             // Not a stow-managed symlink or error checking, treat as conflict
                             return Ok((ActionType::Conflict, 
                                       Some(format!("Directory {:?} contains non-stow managed files", target_path_abs)), 
                                       None));
                         }
                     }
                }
            }
        }
        return Ok((ActionType::CreateDirectory, None, None));
    }

    // Check if target is a symlink pointing to the same source (already stowed)
    if fs_utils::is_symlink(target_path_abs) {
        if let Ok(Some((existing_package_name, existing_item_path))) = fs_utils::is_stow_symlink(target_path_abs, &config.stow_dir) {


            // It's a stow-managed symlink
            if existing_item_path == stow_item.package_relative_path {
                // Check if it's the same package
                if let Some(current_package_name) = config.packages.get(0) {
                    if existing_package_name == *current_package_name {
                        // Same package and same item, no conflict - already correctly stowed
                        return Ok((ActionType::Skip, 
                                  Some("Target already points to the same source".to_string()), 
                                  None));
                    }
                }
                // Different package but same item path - this is a conflict that needs resolution
                return handle_stow_package_conflict(stow_item, target_path_abs, link_target_for_symlink, config);
            } else {
                // Different item path - check conflict resolution options
                return handle_stow_package_conflict(stow_item, target_path_abs, link_target_for_symlink, config);
            }
        }
    }

    // Check if it's a file vs directory conflict
    if fs_utils::is_directory(target_path_abs) && stow_item.item_type != StowItemType::Directory {
        return Ok((ActionType::Conflict, 
                  Some(format!("Cannot create file symlink at {:?}: target is a directory", target_path_abs)), 
                  None));
    }

    if !fs_utils::is_directory(target_path_abs) && stow_item.item_type == StowItemType::Directory {
        return Ok((ActionType::Conflict, 
                  Some(format!("Cannot create directory at {:?}: target is a file", target_path_abs)), 
                  None));
    }

    // Check override/defer patterns for non-stow managed files
    let pattern_matcher = PatternMatcher::new(config);
    if let Some((action_type, message, link_target)) = pattern_matcher.check_patterns(target_path_abs, link_target_for_symlink) {
        return Ok((action_type, Some(message), link_target));
    }

    // No pattern matches, it's a conflict
    Ok((ActionType::Conflict, 
        Some(format!("Target path {:?} already exists and is not stow-managed", target_path_abs)), 
        None))
}

/// Handle conflicts between different stow packages
fn handle_stow_package_conflict(
    _stow_item: &StowItem,
    target_path_abs: &Path,
    link_target_for_symlink: PathBuf,
    config: &Config
) -> Result<(ActionType, Option<String>, Option<PathBuf>), RustowError> {
    let pattern_matcher = PatternMatcher::new(config);
    if let Some((action_type, message, link_target)) = pattern_matcher.check_patterns(target_path_abs, link_target_for_symlink) {
        return Ok((action_type, Some(message), link_target));
    }

    // No pattern matches, it's a conflict
    Ok((ActionType::Conflict, 
        Some(format!("Target path {:?} is managed by another stow package", target_path_abs)), 
        None))
}

/// Refine actions by checking for parent path conflicts
fn refine_actions_for_parent_conflicts(actions: &mut [TargetAction], config: &Config) {
    // Collect conflict information first to avoid borrowing issues
    let mut conflicts_to_apply = Vec::new();

    for (i, action) in actions.iter().enumerate() {
        if action.action_type == ActionType::Conflict {
            continue; // Skip actions that are already conflicts
        }

        if let Some(conflict_info) = find_parent_conflict(action, actions, config) {
            conflicts_to_apply.push((i, conflict_info));
        }
    }

    // Apply conflicts
    for (index, conflict_info) in conflicts_to_apply {
        apply_conflict_to_action(&mut actions[index], conflict_info);
    }
}

/// Information about a parent conflict
#[derive(Debug)]
struct ParentConflictInfo {
    conflict_type: ParentConflictType,
    parent_path: PathBuf,
}

#[derive(Debug)]
enum ParentConflictType {
    ParentIsFile,
    ParentIsConflictTarget,
}

/// Find parent conflicts for an action
fn find_parent_conflict(
    action: &TargetAction, 
    all_actions: &[TargetAction], 
    config: &Config
) -> Option<ParentConflictInfo> {
    let mut parent_path_opt = action.target_path.parent();

    while let Some(parent_path) = parent_path_opt {
        if !parent_path.starts_with(&config.target_dir) || parent_path == config.target_dir {
            break;
        }

        // Check if parent path is a file (conflicts with directory requirement)
        if fs_utils::path_exists(parent_path) && !fs_utils::is_directory(parent_path) {
            return Some(ParentConflictInfo {
                conflict_type: ParentConflictType::ParentIsFile,
                parent_path: parent_path.to_path_buf(),
            });
        }

        // Check if parent path is target of another conflicting action
        if is_parent_target_of_conflict(parent_path, all_actions) {
            return Some(ParentConflictInfo {
                conflict_type: ParentConflictType::ParentIsConflictTarget,
                parent_path: parent_path.to_path_buf(),
            });
        }

        parent_path_opt = parent_path.parent();
    }

    None
}

/// Apply conflict information to an action
fn apply_conflict_to_action(action: &mut TargetAction, conflict_info: ParentConflictInfo) {
    action.action_type = ActionType::Conflict;
    action.link_target_path = None;

    action.conflict_details = Some(match conflict_info.conflict_type {
        ParentConflictType::ParentIsFile => {
            let item_name = action.source_item
                .as_ref()
                .map(|si| si.target_name_after_dotfiles_processing.clone())
                .unwrap_or_else(|| PathBuf::from("UnknownSource"));

            format!(
                "Parent path {:?} is a file, but current item {:?} needs it to be a directory (or part of one).",
                conflict_info.parent_path, item_name
            )
        }
        ParentConflictType::ParentIsConflictTarget => {
            format!(
                "Parent path {:?} is part of a conflicting item tree.",
                conflict_info.parent_path
            )
        }
    });
}

/// Check if parent path is the target of another conflicting action
fn is_parent_target_of_conflict(parent_path: &Path, all_actions: &[TargetAction]) -> bool {
    all_actions.iter().any(|action| {
        action.target_path == parent_path && action.action_type == ActionType::Conflict
    })
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
    let mut reports = Vec::new();

    for action in actions {
        let report = if config.simulate {
            execute_simulate_action(action)
        } else {
            execute_real_action(action)
        };
        reports.push(report);
    }

    Ok(reports)
}

/// Execute an action in simulation mode
fn execute_simulate_action(action: &TargetAction) -> TargetActionReport {
    let message = format!(
        "SIMULATE: Would perform {:?} on target {:?} (source: {:?}, link_target: {:?})",
        action.action_type,
        action.target_path,
        action.source_item.as_ref().map_or_else(|| PathBuf::from("N/A"), |si| si.source_path.clone()),
        action.link_target_path.as_ref().map_or_else(|| PathBuf::from("N/A"), |p| p.clone())
    );

    TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::Skipped,
        message: Some(message),
    }
}

/// Execute an action for real
fn execute_real_action(action: &TargetAction) -> TargetActionReport {
    match action.action_type {
        ActionType::Conflict => execute_conflict_action(action),
        ActionType::CreateDirectory => execute_create_directory_action(action),
        ActionType::CreateSymlink => execute_create_symlink_action(action),
        ActionType::DeleteSymlink => execute_delete_symlink_action(action),
        ActionType::DeleteDirectory => execute_delete_directory_action(action),
        ActionType::Skip => execute_skip_action(action),
        _ => create_unimplemented_action_report(action),
    }
}

/// Execute a conflict action (prevent operation)
fn execute_conflict_action(action: &TargetAction) -> TargetActionReport {
    TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::ConflictPrevented,
        message: Some(format!(
            "CONFLICT: Operation prevented for target {:?}. Details: {}",
            action.target_path,
            action.conflict_details.as_deref().unwrap_or("N/A")
        )),
    }
}

/// Execute a create directory action
fn execute_create_directory_action(action: &TargetAction) -> TargetActionReport {
    match fs_utils::create_dir_all(&action.target_path) {
        Ok(_) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Success,
            message: Some(format!("Successfully created directory {:?}", action.target_path)),
        },
        Err(e) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!("Failed to create directory {:?}: {}", action.target_path, e)),
        },
    }
}

/// Execute a create symlink action
fn execute_create_symlink_action(action: &TargetAction) -> TargetActionReport {
    // Ensure parent directory exists
    if let Some(parent_dir) = action.target_path.parent() {
        if !fs_utils::path_exists(parent_dir) {
            if let Err(e) = fs_utils::create_dir_all(parent_dir) {
                return TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Failure(format!(
                        "Failed to create parent directory {:?} for symlink: {}",
                        parent_dir, e
                    )),
                    message: Some(format!(
                        "Failed to create parent directory {:?} for symlink {:?}: {}",
                        parent_dir, action.target_path, e
                    )),
                };
            }
        }
    }

    match &action.link_target_path {
        Some(link_target) => {
            // If target already exists, remove it first (for override behavior)
            if fs_utils::path_exists(&action.target_path) {
                if fs_utils::is_symlink(&action.target_path) {
                    if let Err(e) = fs_utils::delete_symlink(&action.target_path) {
                        return TargetActionReport {
                            original_action: action.clone(),
                            status: TargetActionReportStatus::Failure(format!(
                                "Failed to remove existing symlink before override: {}",
                                e
                            )),
                            message: Some(format!(
                                "Failed to remove existing symlink {:?} before creating new one: {}",
                                action.target_path, e
                            )),
                        };
                    }
                } else {
                    // Target exists but is not a symlink - this should have been caught in planning
                    return TargetActionReport {
                        original_action: action.clone(),
                        status: TargetActionReportStatus::Failure(
                            "Target exists and is not a symlink - cannot override".to_string(),
                        ),
                        message: Some(format!(
                            "Target {:?} exists and is not a symlink - cannot override",
                            action.target_path
                        )),
                    };
                }
            }

            match fs_utils::create_symlink(&action.target_path, link_target) {
                Ok(_) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Success,
                    message: Some(format!(
                        "Successfully created symlink {:?} -> {:?}",
                        action.target_path, link_target
                    )),
                },
                Err(e) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Failure(e.to_string()),
                    message: Some(format!(
                        "Failed to create symlink {:?} -> {:?}: {}",
                        action.target_path, link_target, e
                    )),
                },
            }
        }
        None => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(
                "CreateSymlink action missing link_target_path".to_string(),
            ),
            message: Some(format!(
                "CreateSymlink action for {:?} is missing link_target_path.",
                action.target_path
            )),
        },
    }
}

/// Execute a delete symlink action
fn execute_delete_symlink_action(action: &TargetAction) -> TargetActionReport {
    match fs_utils::delete_symlink(&action.target_path) {
        Ok(_) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Success,
            message: Some(format!("Successfully deleted symlink {:?}", action.target_path)),
        },
        Err(e) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!("Failed to delete symlink {:?}: {}", action.target_path, e)),
        },
    }
}

/// Execute a delete directory action
fn execute_delete_directory_action(action: &TargetAction) -> TargetActionReport {
    // Check if directory exists first
    if !fs_utils::path_exists(&action.target_path) {
        return TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Skipped,
            message: Some(format!("Directory {:?} does not exist, skipping deletion", action.target_path)),
        };
    }

    // Check if directory is empty before attempting deletion
    match is_directory_empty(&action.target_path) {
        Ok(true) => {
            // Directory is empty, proceed with deletion
            match fs_utils::delete_empty_dir(&action.target_path) {
                Ok(_) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Success,
                    message: Some(format!("Successfully deleted empty directory {:?}", action.target_path)),
                },
                Err(e) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Failure(e.to_string()),
                    message: Some(format!("Failed to delete directory {:?}: {}", action.target_path, e)),
                }
            }
        },
        Ok(false) => {
            // Directory is not empty, skip deletion
            TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Skipped,
                message: Some(format!("Skipped deleting directory {:?}: not empty", action.target_path)),
            }
        },
        Err(e) => {
            // Error checking if directory is empty
            TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Failure(e.to_string()),
                message: Some(format!("Failed to check if directory {:?} is empty: {}", action.target_path, e)),
            }
        }
    }
}

/// Execute a skip action
fn execute_skip_action(action: &TargetAction) -> TargetActionReport {
    TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::Skipped,
        message: action.conflict_details.clone().or_else(|| Some("Action skipped".to_string())),
    }
}

/// Create a report for unimplemented action types
fn create_unimplemented_action_report(action: &TargetAction) -> TargetActionReport {
    TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::Skipped, // Placeholder
        message: Some(format!("Action {:?} not yet implemented for target {:?}", action.action_type, action.target_path)),
    }
}

/// Load ignore patterns for a package, with error handling
fn load_ignore_patterns_for_package(
    package_name: &str,
    config: &Config
) -> Result<IgnorePatterns, RustowError> {
    IgnorePatterns::load(&config.stow_dir, Some(package_name), &config.home_dir)
        .map_err(|e| {
            RustowError::Ignore(crate::error::IgnoreError::LoadPatternsError(
                format!("Failed to load ignore patterns for package '{}': {:?}", package_name, e)
            ))
        })
}

/// Process all packages and collect their actions
fn collect_package_actions<F>(
    config: &Config,
    action_planner: F
) -> Result<Vec<TargetAction>, RustowError>
where
    F: Fn(&str, &Config, &IgnorePatterns) -> Result<Vec<TargetAction>, RustowError>,
{
    if config.packages.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_actions = Vec::new();

    for package_name in &config.packages {
        let ignore_patterns = load_ignore_patterns_for_package(package_name, config)?;
        let package_actions = action_planner(package_name, config, &ignore_patterns)?;
        all_actions.extend(package_actions);
    }

    Ok(all_actions)
}

pub fn stow_packages(config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    if config.packages.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_planned_actions = collect_package_actions(config, plan_actions)?;

    // Resolve conflicts using the dedicated conflict resolver
    let conflict_resolver = ConflictResolver::new(config);
    conflict_resolver.resolve_inter_package_conflicts(&mut all_planned_actions);
    conflict_resolver.propagate_conflicts_to_children(&mut all_planned_actions);

    execute_actions(&all_planned_actions, config)
}

/// Delete (unstow) packages from the target directory
pub fn delete_packages(config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    if config.packages.is_empty() {
        return Ok(Vec::new());
    }

    let all_planned_actions = collect_package_actions(config, plan_delete_actions)?;
    execute_actions(&all_planned_actions, config)
}

/// Restow packages (delete then stow)
pub fn restow_packages(config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    let mut all_reports = Vec::new();

    // For restow, we need to delete all existing stow-managed symlinks for the packages
    // regardless of what's currently in the package directory
    for package_name in &config.packages {
        let delete_actions = plan_restow_delete_actions(package_name, config)?;
        let delete_reports = execute_actions(&delete_actions, config)?;
        all_reports.extend(delete_reports);
    }

    // Then stow them again based on current package contents
    let stow_reports = stow_packages(config)?;
    all_reports.extend(stow_reports);

    Ok(all_reports)
}

/// Plan delete actions for restow operation - removes all stow-managed symlinks for a package
/// regardless of current package contents
fn plan_restow_delete_actions(package_name: &str, config: &Config) -> Result<Vec<TargetAction>, RustowError> {
    let mut actions: Vec<TargetAction> = Vec::new();
    let package_path: PathBuf = config.stow_dir.join(package_name);

    if !fs_utils::path_exists(&package_path) {
        return Err(StowError::PackageNotFound(package_name.to_string()).into());
    }

    // Walk through the target directory and find all stow-managed symlinks that point to this package
    collect_stow_symlinks_for_package(&config.target_dir, &config.stow_dir, package_name, &mut actions)?;

    // Sort actions so that symlink deletions come before directory deletions
    // This ensures that directories are only deleted after their contents are removed
    actions.sort_by(|a, b| {
        match (&a.action_type, &b.action_type) {
            (ActionType::DeleteSymlink, ActionType::DeleteDirectory) => std::cmp::Ordering::Less,
            (ActionType::DeleteDirectory, ActionType::DeleteSymlink) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });

    Ok(actions)
}

/// Recursively collect all stow-managed symlinks in target_dir that point to the specified package
fn collect_stow_symlinks_for_package(
    target_dir: &Path, 
    stow_dir: &Path, 
    package_name: &str, 
    actions: &mut Vec<TargetAction>
) -> Result<(), RustowError> {
    if !fs_utils::path_exists(target_dir) {
        return Ok(());
    }

    let entries = std::fs::read_dir(target_dir).map_err(|_| {
        // Convert to a more specific error if needed, but for now just skip
        RustowError::Stow(StowError::InvalidPackageStructure(
            format!("Cannot read directory: {:?}", target_dir)
        ))
    })?;

    for entry in entries.flatten() {
        let path = entry.path();

        if fs_utils::is_symlink(&path) {
            process_symlink_for_deletion(&path, stow_dir, package_name, actions)?;
        } else if fs_utils::is_directory(&path) {
            process_directory_for_deletion(&path, stow_dir, package_name, actions)?;
        }
    }

    Ok(())
}

/// Process a symlink to determine if it should be deleted during restow
fn process_symlink_for_deletion(
    symlink_path: &Path,
    stow_dir: &Path,
    package_name: &str,
    actions: &mut Vec<TargetAction>
) -> Result<(), RustowError> {
    let link_target = fs_utils::read_link(symlink_path).map_err(|_| {
        RustowError::Stow(StowError::InvalidPackageStructure(
            format!("Failed to read symlink: {:?}", symlink_path)
        ))
    })?;

    let resolved_target = resolve_symlink_target(symlink_path, &link_target);
    let package_path = stow_dir.join(package_name);
    let canonical_package_path = fs_utils::canonicalize_path(&package_path)?;

    if should_delete_symlink(&resolved_target, &canonical_package_path)? {
        actions.push(create_delete_symlink_action(symlink_path.to_path_buf()));
    }

    Ok(())
}

/// Process a directory recursively and mark empty directories for deletion
fn process_directory_for_deletion(
    dir_path: &Path,
    stow_dir: &Path,
    package_name: &str,
    actions: &mut Vec<TargetAction>
) -> Result<(), RustowError> {
    // Recursively process subdirectories first
    collect_stow_symlinks_for_package(dir_path, stow_dir, package_name, actions)?;

    // Always mark directory for potential deletion - the execution phase will check if it's empty
    actions.push(create_delete_directory_action(dir_path.to_path_buf()));

    Ok(())
}

/// Resolve symlink target to absolute path
fn resolve_symlink_target(symlink_path: &Path, link_target: &Path) -> PathBuf {
    if link_target.is_absolute() {
        link_target.to_path_buf()
    } else {
        symlink_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(link_target)
    }
}

/// Determine if a symlink should be deleted based on its target
fn should_delete_symlink(
    resolved_target: &Path,
    canonical_package_path: &Path
) -> Result<bool, RustowError> {
    // Try to canonicalize the target (works for existing files)
    if let Ok(canonical_target) = fs_utils::canonicalize_path(resolved_target) {
        return Ok(canonical_target.starts_with(canonical_package_path));
    }

    // For broken symlinks, normalize the path manually
    let normalized_target = normalize_path_components(resolved_target);
    Ok(normalized_target.starts_with(canonical_package_path))
}

/// Normalize path by resolving .. and . components manually
fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized_components = Vec::new();

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized_components.pop();
            }
            std::path::Component::CurDir => {
                // Skip current directory components
            }
            other => {
                normalized_components.push(other);
            }
        }
    }

    normalized_components.iter().collect()
}

/// Check if a directory is empty
fn is_directory_empty(dir_path: &Path) -> Result<bool, RustowError> {
    let entries = std::fs::read_dir(dir_path).map_err(|_| {
        RustowError::Stow(StowError::InvalidPackageStructure(
            format!("Cannot read directory: {:?}", dir_path)
        ))
    })?;

    Ok(entries.count() == 0)
}

/// Create a delete symlink action
fn create_delete_symlink_action(target_path: PathBuf) -> TargetAction {
    TargetAction {
        source_item: None,
        target_path,
        link_target_path: None,
        action_type: ActionType::DeleteSymlink,
        conflict_details: None,
    }
}

/// Create a delete directory action
fn create_delete_directory_action(target_path: PathBuf) -> TargetAction {
    TargetAction {
        source_item: None,
        target_path,
        link_target_path: None,
        action_type: ActionType::DeleteDirectory,
        conflict_details: None,
    }
}

/// Plan actions for deleting (unstowing) a package
fn plan_delete_actions(package_name: &str, config: &Config, current_ignore_patterns: &IgnorePatterns) -> Result<Vec<TargetAction>, RustowError> {
    let package_path = config.stow_dir.join(package_name);
    validate_package_path(&package_path, package_name)?;

    let raw_items = load_package_items(&package_path, package_name)?;
    let mut actions = Vec::new();

    for raw_item in raw_items {
        if let Some(action) = process_item_for_deletion(raw_item, config, current_ignore_patterns)? {
            actions.push(action);
        }
    }

    Ok(actions)
}

/// Validate that the package path exists and is a directory
fn validate_package_path(package_path: &Path, package_name: &str) -> Result<(), RustowError> {
    if !fs_utils::path_exists(package_path) {
        return Err(StowError::PackageNotFound(package_name.to_string()).into());
    }

    if !fs_utils::is_directory(package_path) {
        return Err(StowError::InvalidPackageStructure(format!(
            "Package '{}' is not a directory at {:?}",
            package_name,
            package_path
        )).into());
    }

    Ok(())
}

/// Load all items from a package directory
fn load_package_items(package_path: &Path, package_name: &str) -> Result<Vec<fs_utils::RawStowItem>, RustowError> {
    match fs_utils::walk_package_dir(package_path) {
        Ok(items) => Ok(items),
        Err(RustowError::Fs(FsError::NotFound(_))) => {
            Err(StowError::PackageNotFound(package_name.to_string()).into())
        }
        Err(e) => Err(e),
    }
}

/// Process a single item for deletion, returning an action if needed
fn process_item_for_deletion(
    raw_item: fs_utils::RawStowItem,
    config: &Config,
    current_ignore_patterns: &IgnorePatterns
) -> Result<Option<TargetAction>, RustowError> {
    let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
        raw_item.package_relative_path.to_str().unwrap_or(""), 
        config.dotfiles
    ));

    // Check if item should be ignored
    if should_ignore_item(&processed_target_relative_path, current_ignore_patterns) {
        return Ok(None);
    }

    let target_path_abs = config.target_dir.join(&processed_target_relative_path);
    let stow_item = create_stow_item_from_raw(raw_item, processed_target_relative_path);

    let action = if fs_utils::path_exists(&target_path_abs) {
        plan_deletion_for_existing_target(&stow_item, &target_path_abs, config)?
    } else {
        create_skip_action_for_missing_target(stow_item, target_path_abs)
    };

    Ok(Some(action))
}

/// Check if an item should be ignored based on ignore patterns
fn should_ignore_item(
    processed_target_relative_path: &Path,
    current_ignore_patterns: &IgnorePatterns
) -> bool {
    let path_for_ignore_check_fullpath = PathBuf::from("/").join(processed_target_relative_path);
    let basename_for_ignore_check = processed_target_relative_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    ignore::is_ignored(&path_for_ignore_check_fullpath, &basename_for_ignore_check, current_ignore_patterns)
}

/// Create a StowItem from a RawStowItem
fn create_stow_item_from_raw(
    raw_item: fs_utils::RawStowItem,
    processed_target_relative_path: PathBuf
) -> StowItem {
    let item_type_stow = match raw_item.item_type {
        fs_utils::RawStowItemType::File => StowItemType::File,
        fs_utils::RawStowItemType::Directory => StowItemType::Directory,
        fs_utils::RawStowItemType::Symlink => StowItemType::Symlink,
    };

    StowItem {
        source_path: raw_item.absolute_path,
        package_relative_path: raw_item.package_relative_path,
        target_name_after_dotfiles_processing: processed_target_relative_path,
        item_type: item_type_stow,
    }
}

/// Plan deletion action for an existing target
fn plan_deletion_for_existing_target(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config
) -> Result<TargetAction, RustowError> {
    let (action_type, conflict_details) = match stow_item.item_type {
        StowItemType::Directory => {
            (ActionType::DeleteDirectory, None)
        }
        StowItemType::File | StowItemType::Symlink => {
            determine_file_deletion_action(stow_item, target_path_abs, config)?
        }
    };

    Ok(TargetAction {
        source_item: Some(stow_item.clone()),
        target_path: target_path_abs.to_path_buf(),
        link_target_path: None,
        action_type,
        conflict_details,
    })
}

/// Determine the appropriate action for deleting a file or symlink
fn determine_file_deletion_action(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config
) -> Result<(ActionType, Option<String>), RustowError> {
    if !fs_utils::is_symlink(target_path_abs) {
        return Ok((
            ActionType::Skip,
            Some(format!("Target {:?} exists but is not a symlink", target_path_abs))
        ));
    }

    match fs_utils::is_stow_symlink(target_path_abs, &config.stow_dir) {
        Ok(Some((_package_name, item_path_in_package))) => {
            if item_path_in_package == stow_item.package_relative_path {
                Ok((ActionType::DeleteSymlink, None))
            } else {
                Ok((
                    ActionType::Skip,
                    Some(format!(
                        "Symlink at {:?} belongs to different package item: {:?}",
                        target_path_abs, item_path_in_package
                    ))
                ))
            }
        }
        Ok(None) => Ok((
            ActionType::Skip,
            Some(format!("File at {:?} is not a stow-managed symlink", target_path_abs))
        )),
        Err(_) => Ok((
            ActionType::Conflict,
            Some(format!("Error checking symlink at {:?}", target_path_abs))
        )),
    }
}

/// Create a skip action for a missing target
fn create_skip_action_for_missing_target(
    stow_item: StowItem,
    target_path_abs: PathBuf
) -> TargetAction {
    TargetAction {
        source_item: Some(stow_item),
        target_path: target_path_abs,
        link_target_path: None,
        action_type: ActionType::Skip,
        conflict_details: Some("Target does not exist, nothing to delete".to_string()),
    }
} 
