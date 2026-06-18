// Placeholder for stow module
// This file can be populated with stow logic later.

use crate::config::Config;
use crate::dotfiles;
use crate::error::{FsError, RustowError, StowError};
use crate::fs_utils::{self};
use crate::ignore::{self, IgnorePatterns};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// Define modules inline for now
mod conflict_resolver {
    use crate::stow::{ActionType, TargetAction};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Handles conflict detection and resolution between packages
    pub struct ConflictResolver;

    impl ConflictResolver {
        /// Detect and resolve inter-package conflicts
        pub fn resolve_inter_package_conflicts(actions: &mut [TargetAction]) {
            let target_map = Self::build_target_map(actions);
            Self::mark_conflicting_actions(actions, target_map);
        }

        /// Propagate conflicts to child items
        pub fn propagate_conflicts_to_children(actions: &mut [TargetAction]) {
            let parent_conflicts = Self::collect_parent_conflicts(actions);
            let child_updates = Self::find_child_conflicts(actions, &parent_conflicts);
            Self::apply_child_conflict_updates(actions, child_updates);
        }

        fn build_target_map(actions: &[TargetAction]) -> HashMap<PathBuf, Vec<usize>> {
            let mut target_map: HashMap<PathBuf, Vec<usize>> = HashMap::new();

            for (index, action) in actions.iter().enumerate() {
                if action.action_type != ActionType::Conflict {
                    target_map
                        .entry(action.target_path.clone())
                        .or_default()
                        .push(index);
                }
            }

            target_map
        }

        fn mark_conflicting_actions(
            actions: &mut [TargetAction],
            target_map: HashMap<PathBuf, Vec<usize>>,
        ) {
            for (_target_path, action_indices) in target_map {
                if action_indices.len() > 1
                    && !Self::are_target_actions_compatible(actions, &action_indices)
                {
                    for index in action_indices {
                        Self::mark_action_as_conflict(&mut actions[index]);
                    }
                }
            }
        }

        fn are_target_actions_compatible(
            actions: &[TargetAction],
            action_indices: &[usize],
        ) -> bool {
            let relevant_action_types: Vec<&ActionType> = action_indices
                .iter()
                .map(|index| &actions[*index].action_type)
                .filter(|action_type| !matches!(action_type, ActionType::Skip))
                .collect();

            if relevant_action_types.is_empty() {
                return true;
            }

            if relevant_action_types
                .iter()
                .all(|action_type| matches!(action_type, ActionType::CreateDirectory))
            {
                return true;
            }

            let has_split_delete = relevant_action_types
                .iter()
                .any(|action_type| matches!(action_type, ActionType::DeleteSymlink));
            let has_split_directory = relevant_action_types
                .iter()
                .any(|action_type| matches!(action_type, ActionType::CreateDirectory));

            has_split_delete
                && has_split_directory
                && relevant_action_types.iter().all(|action_type| {
                    matches!(
                        action_type,
                        ActionType::DeleteSymlink | ActionType::CreateDirectory
                    )
                })
        }

        fn mark_action_as_conflict(action: &mut TargetAction) {
            action.action_type = ActionType::Conflict;
            if action.conflict_details.is_none() {
                let sources_involved = action
                    .source_item
                    .as_ref()
                    .map(|si| si.source_path.display().to_string())
                    .unwrap_or_else(|| "Unknown source".to_string());
                action.conflict_details = Some(format!(
                    "Inter-package conflict: Multiple packages attempt to manage target path {:?}. Source: {}.",
                    action.target_path, sources_involved
                ));
            }
            action.link_target_path = None;
        }

        fn collect_parent_conflicts(
            actions: &[TargetAction],
        ) -> std::collections::HashSet<PathBuf> {
            actions
                .iter()
                .filter(|action| action.action_type == ActionType::Conflict)
                .map(|action| action.target_path.clone())
                .collect()
        }

        fn find_child_conflicts(
            actions: &[TargetAction],
            parent_conflicts: &std::collections::HashSet<PathBuf>,
        ) -> Vec<(usize, String)> {
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
                            action
                                .source_item
                                .as_ref()
                                .map(|si| si.target_name_after_dotfiles_processing.clone())
                                .unwrap_or_else(|| PathBuf::from("UnknownSource"))
                        );
                        child_conflict_updates.push((i, conflict_message));
                    }
                }
            }

            child_conflict_updates
        }

        fn apply_child_conflict_updates(
            actions: &mut [TargetAction],
            child_updates: Vec<(usize, String)>,
        ) {
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
        pub fn check_patterns(
            &self,
            target_path_abs: &Path,
            link_target: PathBuf,
        ) -> Option<(ActionType, String, Option<PathBuf>)> {
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
                    None,
                ));
            }

            // Check override patterns
            if let Some(override_pattern) = self.find_matching_override_pattern(&target_path_str) {
                return Some((
                    ActionType::CreateSymlink,
                    format!(
                        "Overriding existing file due to pattern match: {}",
                        override_pattern.as_str()
                    ),
                    Some(link_target),
                ));
            }

            None
        }

        fn find_matching_defer_pattern(&self, target_path_str: &str) -> Option<&regex::Regex> {
            self.config
                .defers
                .iter()
                .find(|pattern| pattern.is_match(target_path_str))
        }

        fn find_matching_override_pattern(&self, target_path_str: &str) -> Option<&regex::Regex> {
            self.config
                .overrides
                .iter()
                .find(|pattern| pattern.is_match(target_path_str))
        }
    }
}

use conflict_resolver::ConflictResolver;
use pattern_matcher::PatternMatcher;

// --- Action Planning Enums and Structs ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionType {
    CreateSymlink,   // Create a symbolic link
    DeleteSymlink,   // Delete a symbolic link
    CreateDirectory, // Create a directory (for folding)
    DeleteDirectory, // Delete an empty directory (during unstow)
    AdoptFile,       // Move a file from target to stow dir, then link (for --adopt)
    AdoptDirectory,  // Move a directory from target to stow dir, then link (for --adopt)
    Skip,            // Skip an operation (e.g., due to --defer or already correct state)
    Conflict,        // A conflict was detected that cannot be resolved by options
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
    pub target_path: PathBuf,          // Absolute path in the target directory
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

fn plan_actions(
    package_name: &str,
    config: &Config,
    current_ignore_patterns: &IgnorePatterns,
) -> Result<Vec<TargetAction>, RustowError> {
    let package_path = validated_package_path(&config.stow_dir, package_name)?;

    let raw_items = load_package_items(&package_path, package_name)?;
    let mut actions = Vec::new();

    // Process each item to create initial actions
    for raw_item in raw_items {
        actions.extend(process_item_for_stow(
            raw_item,
            config,
            current_ignore_patterns,
            package_name,
        )?);
    }

    if config.adopt {
        prune_actions_for_adopted_dirs(&mut actions);
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
    package_name: &str,
) -> Result<Vec<TargetAction>, RustowError> {
    let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
        raw_item.package_relative_path.to_str().unwrap_or(""),
        config.dotfiles,
    ));

    // Check if item should be ignored
    if should_ignore_item(&processed_target_relative_path, current_ignore_patterns) {
        return Ok(Vec::new());
    }

    let target_path_abs = config.target_dir.join(&processed_target_relative_path);
    let stow_item = create_stow_item_from_raw(raw_item, processed_target_relative_path);

    if let Some(actions) =
        plan_split_open_actions_if_needed(&stow_item, &target_path_abs, config, package_name)?
    {
        return Ok(actions);
    }

    let link_target_for_symlink =
        calculate_link_target(&stow_item, &target_path_abs, config, package_name);
    let action = plan_stow_action_for_item(
        &stow_item,
        &target_path_abs,
        link_target_for_symlink,
        config,
        package_name,
    )?;

    Ok(vec![action])
}

/// Calculate the relative path for a symlink target
fn calculate_link_target(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config,
    package_name: &str,
) -> PathBuf {
    let relative_to_target_parent = target_path_abs.parent().unwrap_or(&config.target_dir);

    pathdiff::diff_paths(&stow_item.source_path, relative_to_target_parent).unwrap_or_else(|| {
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
    config: &Config,
    package_name: &str,
) -> Result<TargetAction, RustowError> {
    let (action_type, conflict_details, final_link_target) =
        if fs_utils::path_exists(target_path_abs) {
            // Target path exists, need to check for conflicts and resolution options
            handle_existing_target_conflict(
                stow_item,
                target_path_abs,
                link_target_for_symlink,
                config,
                package_name,
            )?
        } else {
            // Target path doesn't exist, proceed with normal action
            match stow_item.item_type {
                StowItemType::Directory => (ActionType::CreateDirectory, None, None),
                StowItemType::File | StowItemType::Symlink => (
                    ActionType::CreateSymlink,
                    None,
                    Some(link_target_for_symlink),
                ),
            }
        };

    Ok(TargetAction {
        source_item: Some(source_item_for_action(
            stow_item,
            &action_type,
            config,
            package_name,
        )?),
        target_path: target_path_abs.to_path_buf(),
        link_target_path: final_link_target,
        action_type,
        conflict_details,
    })
}

fn source_item_for_action(
    stow_item: &StowItem,
    action_type: &ActionType,
    config: &Config,
    package_name: &str,
) -> Result<StowItem, RustowError> {
    let mut source_item = stow_item.clone();

    if config.adopt
        && matches!(
            action_type,
            ActionType::AdoptFile | ActionType::AdoptDirectory
        )
    {
        source_item.source_path = canonical_package_path(&config.stow_dir, package_name)?
            .join(&stow_item.package_relative_path);
    }

    Ok(source_item)
}

/// Handle conflicts when target path already exists
/// Check if a directory contains non-stow managed files
fn check_directory_for_non_stow_files(
    target_path_abs: &Path,
    config: &Config,
) -> Result<bool, RustowError> {
    let entries = std::fs::read_dir(target_path_abs).map_err(|e| {
        RustowError::Fs(FsError::Io {
            path: target_path_abs.to_path_buf(),
            source: e,
        })
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            RustowError::Fs(FsError::Io {
                path: target_path_abs.to_path_buf(),
                source: e,
            })
        })?;
        let entry_path = entry.path();
        if is_non_stow_entry(&entry_path, &config.stow_dir) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if a directory entry represents a non-stow managed file
fn is_non_stow_entry(entry_path: &Path, stow_dir: &Path) -> bool {
    // If there's any file that's not a stow-managed symlink, it's a conflict
    if !fs_utils::is_symlink(entry_path) {
        return true;
    }

    // Check if it's a stow-managed symlink
    match fs_utils::is_stow_symlink(entry_path, stow_dir) {
        Ok(Some(_)) => {
            // It's a stow-managed symlink, not a conflict
            false
        },
        Ok(None) | Err(_) => {
            // Not a stow-managed symlink or error checking, treat as conflict
            true
        },
    }
}

/// Handle directory-to-directory conflicts
fn handle_directory_conflict(
    target_path_abs: &Path,
    config: &Config,
) -> Result<(ActionType, Option<String>, Option<PathBuf>), RustowError> {
    if config.adopt && check_directory_for_non_stow_files(target_path_abs, config)? {
        // Check for --adopt option when directory contains non-stow files
        return Ok((
            ActionType::AdoptDirectory,
            Some(format!(
                "Adopting existing directory: {:?}",
                target_path_abs
            )),
            None,
        ));
    }

    Ok((ActionType::CreateDirectory, None, None))
}

/// Validate if symlink is stow-managed and extract package info
fn validate_stow_symlink(
    target_path_abs: &Path,
    stow_dir: &Path,
) -> Result<Option<(String, PathBuf)>, RustowError> {
    fs_utils::is_stow_symlink(target_path_abs, stow_dir)
}

/// Check if symlink points to the same package and item
fn is_same_package_and_item(
    existing_package_name: &str,
    existing_item_path: &Path,
    stow_item: &StowItem,
    package_name: &str,
    config: &Config,
) -> bool {
    is_same_package_name(existing_package_name, package_name, config)
        && existing_item_path == stow_item.package_relative_path
}

fn calculate_link_target_for_source(source_path: &Path, target_path_abs: &Path) -> PathBuf {
    let relative_to_target_parent = target_path_abs.parent().unwrap_or_else(|| Path::new(""));
    pathdiff::diff_paths(source_path, relative_to_target_parent)
        .unwrap_or_else(|| source_path.to_path_buf())
}

fn read_sorted_directory_entries(path: &Path) -> Result<Vec<std::fs::DirEntry>, RustowError> {
    let entries = std::fs::read_dir(path).map_err(|e| {
        RustowError::Fs(FsError::Io {
            path: path.to_path_buf(),
            source: e,
        })
    })?;

    let mut entries = entries.collect::<Result<Vec<_>, _>>().map_err(|e| {
        RustowError::Fs(FsError::Io {
            path: path.to_path_buf(),
            source: e,
        })
    })?;
    entries.sort_by_key(|entry| entry.file_name());
    Ok(entries)
}

fn read_directory_entries(path: &Path) -> Result<std::fs::ReadDir, RustowError> {
    std::fs::read_dir(path).map_err(|e| {
        RustowError::Fs(FsError::Io {
            path: path.to_path_buf(),
            source: e,
        })
    })
}

fn read_directory_entry(
    entry: std::io::Result<std::fs::DirEntry>,
    path: &Path,
) -> Result<std::fs::DirEntry, RustowError> {
    entry.map_err(|e| {
        RustowError::Fs(FsError::Io {
            path: path.to_path_buf(),
            source: e,
        })
    })
}

fn stow_item_type_from_file_type(file_type: std::fs::FileType) -> Option<StowItemType> {
    if file_type.is_symlink() {
        Some(StowItemType::Symlink)
    } else if file_type.is_dir() {
        Some(StowItemType::Directory)
    } else if file_type.is_file() {
        Some(StowItemType::File)
    } else {
        None
    }
}

fn create_stow_item_from_existing_package_path(
    source_path: PathBuf,
    package_relative_path: PathBuf,
    target_name_after_dotfiles_processing: PathBuf,
    item_type: StowItemType,
) -> StowItem {
    StowItem {
        package_relative_path,
        source_path,
        item_type,
        target_name_after_dotfiles_processing,
    }
}

fn directory_contains_ignored_descendants(
    package_name: &str,
    directory_item_path: &Path,
    config: &Config,
    ignore_patterns: &IgnorePatterns,
) -> Result<bool, RustowError> {
    let source_dir = config.stow_dir.join(package_name).join(directory_item_path);

    for raw_item in fs_utils::walk_package_dir(&source_dir)? {
        let package_relative_path = directory_item_path.join(raw_item.package_relative_path);
        let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
            package_relative_path.to_str().unwrap_or(""),
            config.dotfiles,
        ));

        if should_ignore_item(&processed_target_relative_path, ignore_patterns) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn create_symlink_action_for_item(stow_item: StowItem, target_path: PathBuf) -> TargetAction {
    let link_target_path = calculate_link_target_for_source(&stow_item.source_path, &target_path);

    TargetAction {
        source_item: Some(stow_item),
        target_path,
        link_target_path: Some(link_target_path),
        action_type: ActionType::CreateSymlink,
        conflict_details: None,
    }
}

fn create_directory_action_for_item(stow_item: StowItem, target_path: PathBuf) -> TargetAction {
    TargetAction {
        source_item: Some(stow_item),
        target_path,
        link_target_path: None,
        action_type: ActionType::CreateDirectory,
        conflict_details: None,
    }
}

fn create_relink_actions_for_directory_contents(
    package_name: &str,
    directory_item_path: &Path,
    config: &Config,
    ignore_patterns: &IgnorePatterns,
) -> Result<Vec<TargetAction>, RustowError> {
    let source_dir = config.stow_dir.join(package_name).join(directory_item_path);
    let mut actions = Vec::new();

    for entry in read_sorted_directory_entries(&source_dir)? {
        let source_path = entry.path();
        let file_type = entry.file_type().map_err(|e| {
            RustowError::Fs(FsError::Io {
                path: source_path.clone(),
                source: e,
            })
        })?;

        let Some(item_type) = stow_item_type_from_file_type(file_type) else {
            continue;
        };

        let package_relative_path = directory_item_path.join(entry.file_name());
        let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
            package_relative_path.to_str().unwrap_or(""),
            config.dotfiles,
        ));

        if should_ignore_item(&processed_target_relative_path, ignore_patterns) {
            continue;
        }

        let target_path = config.target_dir.join(&processed_target_relative_path);
        let stow_item = create_stow_item_from_existing_package_path(
            source_path,
            package_relative_path.clone(),
            processed_target_relative_path,
            item_type.clone(),
        );

        if item_type == StowItemType::Directory
            && (config.no_folding
                || directory_contains_ignored_descendants(
                    package_name,
                    &package_relative_path,
                    config,
                    ignore_patterns,
                )?)
        {
            actions.push(create_directory_action_for_item(stow_item, target_path));
            actions.extend(create_relink_actions_for_directory_contents(
                package_name,
                &package_relative_path,
                config,
                ignore_patterns,
            )?);
        } else {
            actions.push(create_symlink_action_for_item(stow_item, target_path));
        }
    }

    Ok(actions)
}

fn plan_split_open_actions_if_needed(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config,
    package_name: &str,
) -> Result<Option<Vec<TargetAction>>, RustowError> {
    if stow_item.item_type != StowItemType::Directory || !fs_utils::is_symlink(target_path_abs) {
        return Ok(None);
    }

    let Some((existing_package_name, existing_item_path)) =
        validate_stow_symlink(target_path_abs, &config.stow_dir)?
    else {
        return Ok(None);
    };
    let (existing_package_name, existing_item_path) =
        lexical_stow_symlink_package_and_item_path(target_path_abs, &config.stow_dir)?
            .unwrap_or((existing_package_name, existing_item_path));
    if !package_is_valid_refold_source(&existing_package_name, config)? {
        return Ok(None);
    }

    if is_same_package_and_item(
        &existing_package_name,
        &existing_item_path,
        stow_item,
        package_name,
        config,
    ) {
        return Ok(None);
    }

    let existing_source_dir = config
        .stow_dir
        .join(&existing_package_name)
        .join(&existing_item_path);
    if !fs_utils::is_directory(&existing_source_dir) {
        return Ok(None);
    }

    let ignore_patterns = load_ignore_patterns_for_package(&existing_package_name, config)?;
    let mut actions = vec![
        create_delete_symlink_action(target_path_abs.to_path_buf()),
        create_create_directory_action(target_path_abs.to_path_buf()),
    ];
    actions.extend(create_relink_actions_for_directory_contents(
        &existing_package_name,
        &existing_item_path,
        config,
        &ignore_patterns,
    )?);

    Ok(Some(actions))
}

/// Handle conflicts with existing symlinks
fn handle_existing_symlink_conflict(
    stow_item: &StowItem,
    target_path_abs: &Path,
    link_target_for_symlink: PathBuf,
    config: &Config,
    package_name: &str,
) -> Result<(ActionType, Option<String>, Option<PathBuf>), RustowError> {
    if let Some((existing_package_name, existing_item_path)) =
        validate_stow_symlink(target_path_abs, &config.stow_dir)?
    {
        // It's a stow-managed symlink
        if is_same_package_and_item(
            &existing_package_name,
            &existing_item_path,
            stow_item,
            package_name,
            config,
        ) {
            // Same package and same item, no conflict - already correctly stowed
            return Ok((
                ActionType::Skip,
                Some("Target already points to the same source".to_string()),
                Some(link_target_for_symlink),
            ));
        } else {
            // Different package or item path - check conflict resolution options
            return handle_stow_package_conflict(
                stow_item,
                target_path_abs,
                link_target_for_symlink,
                config,
            );
        }
    }

    // Not a stow-managed symlink, treat as regular file conflict
    handle_file_type_conflicts(stow_item, target_path_abs, link_target_for_symlink, config)
}

/// Check for file vs directory type conflicts
fn check_file_directory_type_conflicts(
    stow_item: &StowItem,
    target_path_abs: &Path,
) -> Option<(ActionType, String)> {
    // Check if it's a file vs directory conflict
    if fs_utils::is_directory(target_path_abs) && stow_item.item_type != StowItemType::Directory {
        return Some((
            ActionType::Conflict,
            format!(
                "Cannot create file symlink at {:?}: target is a directory",
                target_path_abs
            ),
        ));
    }

    if !fs_utils::is_directory(target_path_abs) && stow_item.item_type == StowItemType::Directory {
        return Some((
            ActionType::Conflict,
            format!(
                "Cannot create directory at {:?}: target is a file",
                target_path_abs
            ),
        ));
    }

    None
}

fn handle_file_type_conflicts(
    stow_item: &StowItem,
    target_path_abs: &Path,
    link_target_for_symlink: PathBuf,
    config: &Config,
) -> Result<(ActionType, Option<String>, Option<PathBuf>), RustowError> {
    // Check for file vs directory type conflicts first
    if let Some((action_type, message)) =
        check_file_directory_type_conflicts(stow_item, target_path_abs)
    {
        return Ok((action_type, Some(message), None));
    }

    // Check for --adopt option
    if config.adopt {
        let action_type = if fs_utils::is_directory(target_path_abs) {
            ActionType::AdoptDirectory
        } else {
            ActionType::AdoptFile
        };
        return Ok((
            action_type,
            Some(format!(
                "Adopting existing file/directory: {:?}",
                target_path_abs
            )),
            Some(link_target_for_symlink),
        ));
    }

    // No pattern matches and no adopt, it's a conflict
    Ok((
        ActionType::Conflict,
        Some(format!(
            "Target path {:?} already exists and is not stow-managed",
            target_path_abs
        )),
        None,
    ))
}

fn handle_existing_target_conflict(
    stow_item: &StowItem,
    target_path_abs: &Path,
    link_target_for_symlink: PathBuf,
    config: &Config,
    package_name: &str,
) -> Result<(ActionType, Option<String>, Option<PathBuf>), RustowError> {
    // Check if target is a symlink pointing to the same source (already stowed)
    if fs_utils::is_symlink(target_path_abs) {
        return handle_existing_symlink_conflict(
            stow_item,
            target_path_abs,
            link_target_for_symlink,
            config,
            package_name,
        );
    }

    // Check if target is a directory and we're trying to create a directory
    if fs_utils::is_directory(target_path_abs) && stow_item.item_type == StowItemType::Directory {
        let result = handle_directory_conflict(target_path_abs, config)?;

        // If it's an adopt action, we need to provide the link target
        if result.0 == ActionType::AdoptDirectory {
            return Ok((result.0, result.1, Some(link_target_for_symlink)));
        }

        return Ok(result);
    }

    handle_file_type_conflicts(stow_item, target_path_abs, link_target_for_symlink, config)
}

/// Handle conflicts between different stow packages
fn handle_stow_package_conflict(
    _stow_item: &StowItem,
    target_path_abs: &Path,
    link_target_for_symlink: PathBuf,
    config: &Config,
) -> Result<(ActionType, Option<String>, Option<PathBuf>), RustowError> {
    let pattern_matcher = PatternMatcher::new(config);
    if let Some((action_type, message, link_target)) =
        pattern_matcher.check_patterns(target_path_abs, link_target_for_symlink.clone())
    {
        return Ok((action_type, Some(message), link_target));
    }

    // No pattern matches, it's a conflict
    Ok((
        ActionType::Conflict,
        Some(format!(
            "Target path {:?} is managed by another stow package",
            target_path_abs
        )),
        Some(link_target_for_symlink),
    ))
}

/// Refine actions by checking for parent path conflicts
/// Collect parent conflict information for all actions
fn collect_parent_conflict_info(
    actions: &[TargetAction],
    config: &Config,
) -> Vec<(usize, ParentConflictInfo)> {
    let mut conflicts_to_apply = Vec::new();

    for (i, action) in actions.iter().enumerate() {
        if action.action_type == ActionType::Conflict {
            continue; // Skip actions that are already conflicts
        }

        if let Some(conflict_info) = find_parent_conflict(action, actions, config) {
            conflicts_to_apply.push((i, conflict_info));
        }
    }

    conflicts_to_apply
}

fn refine_actions_for_parent_conflicts(actions: &mut [TargetAction], config: &Config) {
    // Collect conflict information first to avoid borrowing issues
    let conflicts_to_apply = collect_parent_conflict_info(actions, config);

    // Apply conflicts
    for (index, conflict_info) in conflicts_to_apply {
        apply_conflict_to_action(&mut actions[index], conflict_info);
    }
}

fn prune_actions_for_adopted_dirs(actions: &mut Vec<TargetAction>) {
    let adopted_dirs: Vec<PathBuf> = actions
        .iter()
        .filter(|action| action.action_type == ActionType::AdoptDirectory)
        .map(|action| action.target_path.clone())
        .collect();

    if adopted_dirs.is_empty() {
        return;
    }

    actions.retain(|action| {
        if action.action_type == ActionType::AdoptDirectory {
            return true;
        }

        !adopted_dirs
            .iter()
            .any(|dir| action.target_path.starts_with(dir) && action.target_path != *dir)
    });
}

fn action_package_name(action: &TargetAction, config: &Config) -> Option<String> {
    let source_path = &action.source_item.as_ref()?.source_path;
    let relative_to_stow = source_path.strip_prefix(&config.stow_dir).ok()?;
    let mut components = relative_to_stow.components();

    match components.next() {
        Some(std::path::Component::Normal(package_name)) => {
            Some(package_name.to_string_lossy().into_owned())
        },
        _ => None,
    }
}

fn path_depth(path: &Path) -> usize {
    path.components().count()
}

fn prune_descendants_for_folded_targets(
    actions: &mut Vec<TargetAction>,
    folded_targets: &[(PathBuf, String)],
    config: &Config,
) {
    if folded_targets.is_empty() {
        return;
    }

    actions.retain(|action| {
        let Some(package_name) = action_package_name(action, config) else {
            return true;
        };

        !folded_targets
            .iter()
            .any(|(folded_target, folded_package)| {
                package_name == *folded_package
                    && action.target_path.starts_with(folded_target)
                    && action.target_path != *folded_target
            })
    });
}

fn prune_descendants_of_existing_folded_directory_actions(
    actions: &mut Vec<TargetAction>,
    config: &Config,
) {
    let folded_targets: Vec<(PathBuf, String)> = actions
        .iter()
        .filter(|action| {
            matches!(
                action.action_type,
                ActionType::CreateSymlink | ActionType::Skip
            ) && action
                .source_item
                .as_ref()
                .is_some_and(|item| item.item_type == StowItemType::Directory)
        })
        .filter_map(|action| {
            action_package_name(action, config)
                .map(|package_name| (action.target_path.clone(), package_name))
        })
        .collect();

    prune_descendants_for_folded_targets(actions, &folded_targets, config);
}

fn target_has_other_package_actions(
    candidate_index: usize,
    actions: &[TargetAction],
    config: &Config,
    candidate_target: &Path,
    candidate_package: &str,
) -> bool {
    actions.iter().enumerate().any(|(index, action)| {
        if index == candidate_index {
            return false;
        }

        let target_overlaps = action.target_path.starts_with(candidate_target);
        if !target_overlaps {
            return false;
        }

        action_package_name(action, config)
            .is_none_or(|package_name| package_name != candidate_package)
    })
}

fn directory_action_can_fold(
    index: usize,
    actions: &[TargetAction],
    config: &Config,
) -> Result<bool, RustowError> {
    let action = &actions[index];
    if action.action_type != ActionType::CreateDirectory
        || fs_utils::path_exists(&action.target_path)
        || fs_utils::is_symlink(&action.target_path)
    {
        return Ok(false);
    }

    let Some(stow_item) = &action.source_item else {
        return Ok(false);
    };
    if stow_item.item_type != StowItemType::Directory {
        return Ok(false);
    }

    let Some(package_name) = action_package_name(action, config) else {
        return Ok(false);
    };

    if target_has_other_package_actions(index, actions, config, &action.target_path, &package_name)
    {
        return Ok(false);
    }

    let ignore_patterns = load_ignore_patterns_for_package(&package_name, config)?;
    Ok(!directory_contains_ignored_descendants(
        &package_name,
        &stow_item.package_relative_path,
        config,
        &ignore_patterns,
    )?)
}

fn fold_missing_directory_actions(
    actions: &mut Vec<TargetAction>,
    config: &Config,
) -> Result<(), RustowError> {
    let mut candidate_indices: Vec<usize> = actions
        .iter()
        .enumerate()
        .filter_map(|(index, action)| {
            action
                .source_item
                .as_ref()
                .filter(|item| item.item_type == StowItemType::Directory)
                .map(|_| index)
        })
        .collect();
    candidate_indices.sort_by_key(|index| path_depth(&actions[*index].target_path));

    let mut folded_targets: Vec<(PathBuf, String)> = Vec::new();

    for index in candidate_indices {
        let Some(package_name) = action_package_name(&actions[index], config) else {
            continue;
        };

        if folded_targets
            .iter()
            .any(|(folded_target, folded_package)| {
                package_name == *folded_package
                    && actions[index].target_path.starts_with(folded_target)
                    && actions[index].target_path != *folded_target
            })
        {
            continue;
        }

        if !directory_action_can_fold(index, actions, config)? {
            continue;
        }

        let source_path = actions[index]
            .source_item
            .as_ref()
            .expect("directory candidate should have source item")
            .source_path
            .clone();
        let link_target =
            calculate_link_target_for_source(&source_path, &actions[index].target_path);

        actions[index].action_type = ActionType::CreateSymlink;
        actions[index].link_target_path = Some(link_target);
        actions[index].conflict_details = None;
        folded_targets.push((actions[index].target_path.clone(), package_name));
    }

    prune_descendants_for_folded_targets(actions, &folded_targets, config);
    Ok(())
}

fn deduplicate_create_directory_actions(actions: &mut Vec<TargetAction>) {
    let mut seen_directories = std::collections::HashSet::new();
    actions.retain(|action| {
        if action.action_type != ActionType::CreateDirectory {
            return true;
        }

        seen_directories.insert(action.target_path.clone())
    });
}

fn apply_tree_folding(actions: &mut Vec<TargetAction>, config: &Config) -> Result<(), RustowError> {
    prune_descendants_of_existing_folded_directory_actions(actions, config);

    if !config.no_folding {
        fold_missing_directory_actions(actions, config)?;
    }

    deduplicate_create_directory_actions(actions);
    Ok(())
}

/// Information about a parent conflict
#[derive(Debug)]
struct ParentConflictInfo {
    conflict_type: ParentConflictType,
    parent_path: PathBuf,
}

#[derive(Debug)]
enum ParentConflictType {
    File,
    SymlinkAncestor,
    ConflictTarget,
}

/// Check if a specific parent path has conflicts
fn check_parent_path_conflicts(
    parent_path: &Path,
    all_actions: &[TargetAction],
) -> Option<ParentConflictInfo> {
    if fs_utils::is_symlink(parent_path)
        && !parent_symlink_is_opened_by_plan(parent_path, all_actions)
    {
        return Some(ParentConflictInfo {
            conflict_type: ParentConflictType::SymlinkAncestor,
            parent_path: parent_path.to_path_buf(),
        });
    }

    // Check if parent path is a file (conflicts with directory requirement)
    if fs_utils::path_exists(parent_path) && !fs_utils::is_directory(parent_path) {
        return Some(ParentConflictInfo {
            conflict_type: ParentConflictType::File,
            parent_path: parent_path.to_path_buf(),
        });
    }

    // Check if parent path is target of another conflicting action
    if is_parent_target_of_conflict(parent_path, all_actions) {
        return Some(ParentConflictInfo {
            conflict_type: ParentConflictType::ConflictTarget,
            parent_path: parent_path.to_path_buf(),
        });
    }

    None
}

fn parent_symlink_is_opened_by_plan(parent_path: &Path, all_actions: &[TargetAction]) -> bool {
    let has_delete_symlink = all_actions.iter().any(|action| {
        action.target_path == parent_path && action.action_type == ActionType::DeleteSymlink
    });
    let has_create_directory = all_actions.iter().any(|action| {
        action.target_path == parent_path && action.action_type == ActionType::CreateDirectory
    });

    has_delete_symlink && has_create_directory
}

/// Find parent conflicts for an action
fn find_parent_conflict(
    action: &TargetAction,
    all_actions: &[TargetAction],
    config: &Config,
) -> Option<ParentConflictInfo> {
    let mut parent_path_opt = action.target_path.parent();

    while let Some(parent_path) = parent_path_opt {
        if !parent_path.starts_with(&config.target_dir) || parent_path == config.target_dir {
            break;
        }

        if let Some(conflict_info) = check_parent_path_conflicts(parent_path, all_actions) {
            return Some(conflict_info);
        }

        parent_path_opt = parent_path.parent();
    }

    None
}

/// Generate conflict message based on conflict type and action
fn generate_conflict_message(conflict_info: &ParentConflictInfo, action: &TargetAction) -> String {
    match conflict_info.conflict_type {
        ParentConflictType::File => {
            let item_name = action
                .source_item
                .as_ref()
                .map(|si| si.target_name_after_dotfiles_processing.clone())
                .unwrap_or_else(|| PathBuf::from("UnknownSource"));

            format!(
                "Parent path {:?} is a file, but current item {:?} needs it to be a directory (or part of one).",
                conflict_info.parent_path, item_name
            )
        },
        ParentConflictType::SymlinkAncestor => {
            format!(
                "Parent path {:?} is a symlink; refusing to traverse symlinked target ancestors.",
                conflict_info.parent_path
            )
        },
        ParentConflictType::ConflictTarget => {
            format!(
                "Parent path {:?} is part of a conflicting item tree.",
                conflict_info.parent_path
            )
        },
    }
}

/// Apply conflict information to an action
fn apply_conflict_to_action(action: &mut TargetAction, conflict_info: ParentConflictInfo) {
    action.action_type = ActionType::Conflict;
    action.link_target_path = None;
    action.conflict_details = Some(generate_conflict_message(&conflict_info, action));
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
    Skipped,           // For simulation or if no action was needed
    ConflictPrevented, // For when a planned conflict action is "executed" (i.e. prevented)
    Failure(String),   // Contains an error message
}

#[derive(Debug, Clone)]
pub struct TargetActionReport {
    pub original_action: TargetAction, // The action that was planned
    pub status: TargetActionReportStatus,
    pub message: Option<String>, // Additional details, e.g., error message or simulation output
}

fn execute_actions(
    actions: &[TargetAction],
    config: &Config,
) -> Result<Vec<TargetActionReport>, RustowError> {
    if actions
        .iter()
        .any(|a| a.action_type == ActionType::Conflict)
    {
        return Ok(build_conflict_reports(actions));
    }

    let mut reports = Vec::new();

    for action in actions {
        let report = if config.simulate {
            execute_simulate_action(action)
        } else {
            execute_real_action(action, config)
        };
        reports.push(report);
    }

    Ok(reports)
}

fn build_conflict_reports(actions: &[TargetAction]) -> Vec<TargetActionReport> {
    actions
        .iter()
        .map(|action| {
            if action.action_type == ActionType::Conflict {
                execute_conflict_action(action)
            } else {
                TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Skipped,
                    message: Some("Skipped due to conflicts in the plan".to_string()),
                }
            }
        })
        .collect()
}

/// Execute an action in simulation mode
fn execute_simulate_action(action: &TargetAction) -> TargetActionReport {
    let message = format!(
        "SIMULATE: Would perform {:?} on target {:?} (source: {:?}, link_target: {:?})",
        action.action_type,
        action.target_path,
        action
            .source_item
            .as_ref()
            .map_or_else(|| PathBuf::from("N/A"), |si| si.source_path.clone()),
        action
            .link_target_path
            .as_ref()
            .map_or_else(|| PathBuf::from("N/A"), |p| p.clone())
    );

    TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::Skipped,
        message: Some(message),
    }
}

/// Execute an action for real
fn execute_real_action(action: &TargetAction, config: &Config) -> TargetActionReport {
    match action.action_type {
        ActionType::Conflict => execute_conflict_action(action),
        ActionType::CreateDirectory => execute_create_directory_action(action, config),
        ActionType::CreateSymlink => execute_create_symlink_action(action, config),
        ActionType::DeleteSymlink => execute_delete_symlink_action(action, config),
        ActionType::DeleteDirectory => execute_delete_directory_action(action, config),
        ActionType::AdoptFile => execute_adopt_file_action(action, config),
        ActionType::AdoptDirectory => execute_adopt_directory_action(action, config),
        ActionType::Skip => execute_skip_action(action),
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
fn execute_create_directory_action(action: &TargetAction, config: &Config) -> TargetActionReport {
    if let Some(error_report) = ensure_target_path_ancestors_not_symlink(action, config, true) {
        return error_report;
    }

    match fs_utils::create_dir_all(&action.target_path) {
        Ok(_) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Success,
            message: Some(format!(
                "Successfully created directory {:?}",
                action.target_path
            )),
        },
        Err(e) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to create directory {:?}: {}",
                action.target_path, e
            )),
        },
    }
}

/// Ensure parent directory exists for symlink creation
fn ensure_parent_directory_exists(action: &TargetAction) -> Option<TargetActionReport> {
    if let Some(parent_dir) = action.target_path.parent() {
        if !fs_utils::path_exists(parent_dir) {
            if let Err(e) = fs_utils::create_dir_all(parent_dir) {
                return Some(TargetActionReport {
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
            }
        }
    }
    None
}

fn ensure_target_path_ancestors_not_symlink(
    action: &TargetAction,
    config: &Config,
    include_target: bool,
) -> Option<TargetActionReport> {
    let symlink_path =
        target_symlink_ancestor_path(&action.target_path, &config.target_dir, include_target)?;

    Some(TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::Failure(format!(
            "Refusing to traverse symlinked target ancestor {:?}",
            symlink_path
        )),
        message: Some(format!(
            "Target path {:?} contains symlinked ancestor {:?}",
            action.target_path, symlink_path
        )),
    })
}

fn target_symlink_ancestor_path(path: &Path, root: &Path, include_target: bool) -> Option<PathBuf> {
    let relative_path = path.strip_prefix(root).ok()?;
    let mut current = root.to_path_buf();
    let mut components = relative_path.components().peekable();

    while let Some(component) = components.next() {
        current.push(component.as_os_str());
        let is_target = components.peek().is_none();
        if (include_target || !is_target) && fs_utils::is_symlink(&current) {
            return Some(current);
        }
    }

    None
}

/// Remove existing target if it exists (for override behavior)
fn remove_existing_target(action: &TargetAction) -> Option<TargetActionReport> {
    if fs_utils::path_exists(&action.target_path) {
        if fs_utils::is_symlink(&action.target_path) {
            if let Err(e) = fs_utils::delete_symlink(&action.target_path) {
                return Some(TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Failure(format!(
                        "Failed to remove existing symlink before override: {}",
                        e
                    )),
                    message: Some(format!(
                        "Failed to remove existing symlink {:?} before creating new one: {}",
                        action.target_path, e
                    )),
                });
            }
        } else {
            // Target exists but is not a symlink - this should have been caught in planning
            return Some(TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Failure(
                    "Target exists and is not a symlink - cannot override".to_string(),
                ),
                message: Some(format!(
                    "Target {:?} exists and is not a symlink - cannot override",
                    action.target_path
                )),
            });
        }
    }
    None
}

/// Create the actual symlink
fn create_symlink_with_target(action: &TargetAction, link_target: &Path) -> TargetActionReport {
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

/// Prepare for symlink creation by ensuring prerequisites are met
fn prepare_symlink_creation(action: &TargetAction, config: &Config) -> Option<TargetActionReport> {
    if let Some(error_report) = ensure_target_path_ancestors_not_symlink(action, config, false) {
        return Some(error_report);
    }

    // Ensure parent directory exists
    if let Some(error_report) = ensure_parent_directory_exists(action) {
        return Some(error_report);
    }

    // Remove existing target if needed
    if let Some(error_report) = remove_existing_target(action) {
        return Some(error_report);
    }

    None // No preparation errors, ready to create symlink
}

/// Execute a create symlink action
fn execute_create_symlink_action(action: &TargetAction, config: &Config) -> TargetActionReport {
    match &action.link_target_path {
        Some(link_target) => {
            // Prepare for symlink creation
            if let Some(error_report) = prepare_symlink_creation(action, config) {
                return error_report;
            }

            // Create the symlink
            create_symlink_with_target(action, link_target)
        },
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
fn execute_delete_symlink_action(action: &TargetAction, config: &Config) -> TargetActionReport {
    if let Some(error_report) = ensure_target_path_ancestors_not_symlink(action, config, false) {
        return error_report;
    }

    match fs_utils::delete_symlink(&action.target_path) {
        Ok(_) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Success,
            message: Some(format!(
                "Successfully deleted symlink {:?}",
                action.target_path
            )),
        },
        Err(e) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to delete symlink {:?}: {}",
                action.target_path, e
            )),
        },
    }
}

/// Check if directory exists for deletion
fn check_directory_exists_for_deletion(action: &TargetAction) -> Option<TargetActionReport> {
    if !fs_utils::path_exists(&action.target_path) {
        return Some(TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Skipped,
            message: Some(format!(
                "Directory {:?} does not exist, skipping deletion",
                action.target_path
            )),
        });
    }
    None
}

/// Validate directory is empty before deletion
fn validate_directory_empty_for_deletion(
    action: &TargetAction,
) -> Result<bool, Box<TargetActionReport>> {
    match is_directory_empty(&action.target_path) {
        Ok(is_empty) => Ok(is_empty),
        Err(e) => Err(Box::new(TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to check if directory {:?} is empty: {}",
                action.target_path, e
            )),
        })),
    }
}

/// Perform the actual directory deletion
fn perform_directory_deletion(action: &TargetAction) -> TargetActionReport {
    match fs_utils::delete_empty_dir(&action.target_path) {
        Ok(_) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Success,
            message: Some(format!(
                "Successfully deleted empty directory {:?}",
                action.target_path
            )),
        },
        Err(e) => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to delete directory {:?}: {}",
                action.target_path, e
            )),
        },
    }
}

/// Execute a delete directory action
fn execute_delete_directory_action(action: &TargetAction, config: &Config) -> TargetActionReport {
    if let Some(error_report) = ensure_target_path_ancestors_not_symlink(action, config, false) {
        return error_report;
    }

    // Check if directory exists first
    if let Some(skip_report) = check_directory_exists_for_deletion(action) {
        return skip_report;
    }

    // Check if directory is empty before attempting deletion
    match validate_directory_empty_for_deletion(action) {
        Ok(true) => {
            // Directory is empty, proceed with deletion
            perform_directory_deletion(action)
        },
        Ok(false) => {
            // Directory is not empty, skip deletion
            TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Skipped,
                message: Some(format!(
                    "Skipped deleting directory {:?}: not empty",
                    action.target_path
                )),
            }
        },
        Err(error_report) => {
            // Error checking if directory is empty
            *error_report
        },
    }
}

fn common_package_directory_for_symlinks(
    dir_path: &Path,
    config: &Config,
) -> Result<Option<PathBuf>, RustowError> {
    let entries = read_sorted_directory_entries(dir_path)?;
    if entries.is_empty() {
        return Ok(None);
    }

    let mut common_package_name: Option<String> = None;
    let mut common_item_parent: Option<PathBuf> = None;

    for entry in entries {
        let entry_path = entry.path();
        if !fs_utils::is_symlink(&entry_path) {
            return Ok(None);
        }

        let Some((package_name, item_path)) =
            lexical_stow_symlink_package_and_item_path(&entry_path, &config.stow_dir)?
        else {
            return Ok(None);
        };

        let item_parent = item_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();

        match (&common_package_name, &common_item_parent) {
            (None, None) => {
                common_package_name = Some(package_name);
                common_item_parent = Some(item_parent);
            },
            (Some(existing_package), Some(existing_parent))
                if existing_package == &package_name && existing_parent == &item_parent => {},
            _ => return Ok(None),
        }
    }

    let Some(package_name) = common_package_name else {
        return Ok(None);
    };
    if !package_is_valid_refold_source(&package_name, config)? {
        return Ok(None);
    }

    let item_parent = common_item_parent.unwrap_or_default();
    let source_dir = config.stow_dir.join(package_name).join(item_parent);

    if fs_utils::is_directory(&source_dir)
        && source_directory_can_refold(dir_path, &source_dir, config)?
    {
        Ok(Some(source_dir))
    } else {
        Ok(None)
    }
}

fn source_directory_can_refold(
    dir_path: &Path,
    source_dir: &Path,
    config: &Config,
) -> Result<bool, RustowError> {
    let Some((package_name, item_parent)) =
        package_and_item_path_for_source_dir(source_dir, config)
    else {
        return Ok(false);
    };
    if !package_is_valid_refold_source(&package_name, config)? {
        return Ok(false);
    }

    let ignore_patterns = load_ignore_patterns_for_package(&package_name, config)?;
    if directory_contains_ignored_descendants(
        &package_name,
        &item_parent,
        config,
        &ignore_patterns,
    )? || directory_contains_deferred_descendants(&package_name, &item_parent, config)?
    {
        return Ok(false);
    }

    for entry in read_directory_entries(dir_path)? {
        let path = read_directory_entry(entry, dir_path)?.path();
        let Ok(target_relative_path) = path.strip_prefix(&config.target_dir) else {
            return Ok(false);
        };

        if should_ignore_item(target_relative_path, &ignore_patterns)
            || should_defer_item(target_relative_path, config)
        {
            return Ok(false);
        }

        if !fs_utils::is_symlink(&path) {
            return Ok(false);
        }

        let Some((target_package, target_item_path)) =
            lexical_stow_symlink_package_and_item_path(&path, &config.stow_dir)?
        else {
            return Ok(false);
        };

        if target_package != package_name {
            return Ok(false);
        }

        let expected_item_parent = if item_parent.as_os_str().is_empty() {
            None
        } else {
            Some(item_parent.as_path())
        };
        if target_item_path.parent() != expected_item_parent {
            return Ok(false);
        }

        if !fs_utils::path_exists(
            &config
                .stow_dir
                .join(&target_package)
                .join(target_item_path.as_path()),
        ) {
            return Ok(false);
        }
    }

    for entry in read_directory_entries(source_dir)? {
        let entry = read_directory_entry(entry, source_dir)?;
        let entry_file_name = entry.file_name();
        let package_relative_path = item_parent.join(entry_file_name);
        let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
            package_relative_path.to_str().unwrap_or(""),
            config.dotfiles,
        ));

        if should_ignore_item(&processed_target_relative_path, &ignore_patterns)
            || should_defer_item(&processed_target_relative_path, config)
        {
            return Ok(false);
        }

        let target_path = config.target_dir.join(&processed_target_relative_path);
        if target_path.parent() != Some(dir_path) || !fs_utils::is_symlink(&target_path) {
            return Ok(false);
        }

        let Some((target_package, target_item_path)) =
            lexical_stow_symlink_package_and_item_path(&target_path, &config.stow_dir)?
        else {
            return Ok(false);
        };

        if target_package != package_name || target_item_path != package_relative_path {
            return Ok(false);
        }
    }

    Ok(true)
}

fn package_and_item_path_for_source_dir(
    source_dir: &Path,
    config: &Config,
) -> Option<(String, PathBuf)> {
    let relative_to_stow = source_dir.strip_prefix(&config.stow_dir).ok()?;
    let mut components = relative_to_stow.components();
    let package_name = match components.next() {
        Some(std::path::Component::Normal(package_name)) => {
            package_name.to_string_lossy().into_owned()
        },
        _ => return None,
    };

    Some((package_name, components.as_path().to_path_buf()))
}

fn package_is_valid_refold_source(
    package_name: &str,
    config: &Config,
) -> Result<bool, RustowError> {
    match canonical_package_path(&config.stow_dir, package_name) {
        Ok(_) => Ok(true),
        Err(RustowError::Stow(StowError::InvalidPackageStructure(_)))
        | Err(RustowError::Fs(FsError::NotFound(_)))
        | Err(RustowError::Fs(FsError::NotADirectory(_))) => Ok(false),
        Err(RustowError::Fs(FsError::Canonicalize { source, .. }))
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(false)
        },
        Err(e) => Err(e),
    }
}

fn directory_contains_deferred_descendants(
    package_name: &str,
    directory_item_path: &Path,
    config: &Config,
) -> Result<bool, RustowError> {
    let source_dir = config.stow_dir.join(package_name).join(directory_item_path);

    for raw_item in fs_utils::walk_package_dir(&source_dir)? {
        let package_relative_path = directory_item_path.join(raw_item.package_relative_path);
        let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
            package_relative_path.to_str().unwrap_or(""),
            config.dotfiles,
        ));

        if should_defer_item(&processed_target_relative_path, config) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn should_defer_item(processed_target_relative_path: &Path, config: &Config) -> bool {
    let target_path_str = processed_target_relative_path.to_string_lossy();
    config
        .defers
        .iter()
        .any(|defer| defer.is_match(&target_path_str))
}

fn lexical_stow_symlink_package_and_item_path(
    link_path: &Path,
    stow_dir: &Path,
) -> Result<Option<(String, PathBuf)>, RustowError> {
    if !fs_utils::is_symlink(link_path) {
        return Ok(None);
    }

    let link_target = fs_utils::read_link(link_path)?;
    let resolved_target =
        normalize_path_components(&resolve_symlink_target(link_path, &link_target));
    let normalized_stow_dir = normalize_path_components(stow_dir);
    let Ok(relative_to_stow) = resolved_target.strip_prefix(&normalized_stow_dir) else {
        return Ok(None);
    };

    let mut components = relative_to_stow.components();
    match components.next() {
        Some(std::path::Component::Normal(package_name)) => Ok(Some((
            package_name.to_string_lossy().into_owned(),
            components.as_path().to_path_buf(),
        ))),
        _ => Ok(None),
    }
}

fn refold_directory(dir_path: &Path, source_dir: &Path) -> TargetActionReport {
    let link_target = calculate_link_target_for_source(source_dir, dir_path);
    let action = TargetAction {
        source_item: None,
        target_path: dir_path.to_path_buf(),
        link_target_path: Some(link_target.clone()),
        action_type: ActionType::CreateSymlink,
        conflict_details: Some(format!("Refolding directory {:?}", dir_path)),
    };

    match read_sorted_directory_entries(dir_path) {
        Ok(entries) => {
            for entry in entries {
                if let Err(e) = fs_utils::delete_symlink(&entry.path()) {
                    return TargetActionReport {
                        original_action: action,
                        status: TargetActionReportStatus::Failure(e.to_string()),
                        message: Some(format!(
                            "Failed to remove symlink {:?} while refolding {:?}: {}",
                            entry.path(),
                            dir_path,
                            e
                        )),
                    };
                }
            }
        },
        Err(e) => {
            return TargetActionReport {
                original_action: action,
                status: TargetActionReportStatus::Failure(e.to_string()),
                message: Some(format!(
                    "Failed to read directory {:?} while refolding: {}",
                    dir_path, e
                )),
            };
        },
    }

    if let Err(e) = fs_utils::delete_empty_dir(dir_path) {
        return TargetActionReport {
            original_action: action,
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to remove directory {:?} while refolding: {}",
                dir_path, e
            )),
        };
    }

    match fs_utils::create_symlink(dir_path, &link_target) {
        Ok(_) => TargetActionReport {
            original_action: action,
            status: TargetActionReportStatus::Success,
            message: Some(format!(
                "Successfully refolded directory {:?} -> {:?}",
                dir_path, link_target
            )),
        },
        Err(e) => TargetActionReport {
            original_action: action,
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to create refolded symlink {:?} -> {:?}: {}",
                dir_path, link_target, e
            )),
        },
    }
}

fn collect_refold_candidate_dirs<'a, I>(actions: I, config: &Config) -> Vec<PathBuf>
where
    I: IntoIterator<Item = &'a TargetAction>,
{
    let mut dirs = HashSet::new();

    for action in actions {
        for candidate in action.target_path.ancestors() {
            if candidate == config.target_dir {
                break;
            }

            if !candidate.starts_with(&config.target_dir)
                || candidate == config.stow_dir
                || candidate.starts_with(&config.stow_dir)
            {
                continue;
            }

            dirs.insert(candidate.to_path_buf());
        }
    }

    let mut dirs: Vec<PathBuf> = dirs.into_iter().collect();
    dirs.sort_by_key(|path| std::cmp::Reverse(path_depth(path)));
    dirs
}

fn refold_foldable_trees<'a, I>(
    config: &Config,
    actions: I,
) -> Result<Vec<TargetActionReport>, RustowError>
where
    I: IntoIterator<Item = &'a TargetAction>,
{
    if config.no_folding {
        return Ok(Vec::new());
    }

    let dirs = collect_refold_candidate_dirs(actions, config);

    let mut reports = Vec::new();
    for dir in dirs {
        if !fs_utils::path_exists(&dir)
            || fs_utils::is_symlink(&dir)
            || !fs_utils::is_directory(&dir)
            || path_has_symlink_ancestor(&dir, &config.target_dir)
        {
            continue;
        }

        if let Some(source_dir) = common_package_directory_for_symlinks(&dir, config)? {
            reports.push(refold_directory(&dir, &source_dir));
        }
    }

    Ok(reports)
}

fn reports_allow_refolding(reports: &[TargetActionReport], config: &Config) -> bool {
    !config.simulate
        && !config.no_folding
        && reports.iter().all(|report| {
            !matches!(
                report.status,
                TargetActionReportStatus::ConflictPrevented | TargetActionReportStatus::Failure(_)
            )
        })
}

/// Execute a skip action
fn execute_skip_action(action: &TargetAction) -> TargetActionReport {
    TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::Skipped,
        message: action
            .conflict_details
            .clone()
            .or_else(|| Some("Action skipped".to_string())),
    }
}

/// Load ignore patterns for a package, with error handling
fn load_ignore_patterns_for_package(
    package_name: &str,
    config: &Config,
) -> Result<IgnorePatterns, RustowError> {
    IgnorePatterns::load(&config.stow_dir, Some(package_name), &config.home_dir)
        .map(|patterns| patterns.with_extra_patterns(&config.ignore_patterns))
        .map_err(|e| {
            RustowError::Ignore(crate::error::IgnoreError::LoadPatternsError(format!(
                "Failed to load ignore patterns for package '{}': {:?}",
                package_name, e
            )))
        })
}

/// Process all packages and collect their actions
fn collect_package_actions<F>(
    config: &Config,
    action_planner: F,
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

/// Apply conflict resolution to planned actions
fn apply_conflict_resolution(actions: &mut [TargetAction], _config: &Config) {
    ConflictResolver::resolve_inter_package_conflicts(actions);
    ConflictResolver::propagate_conflicts_to_children(actions);
}

fn plan_stow_package_actions(config: &Config) -> Result<Vec<TargetAction>, RustowError> {
    let mut all_planned_actions = collect_package_actions(config, plan_actions)?;

    apply_tree_folding(&mut all_planned_actions, config)?;
    apply_conflict_resolution(&mut all_planned_actions, config);

    Ok(all_planned_actions)
}

pub fn stow_packages(config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    if config.packages.is_empty() {
        return Ok(Vec::new());
    }

    let all_planned_actions = plan_stow_package_actions(config)?;
    execute_actions(&all_planned_actions, config)
}

fn plan_delete_package_actions(config: &Config) -> Result<Vec<TargetAction>, RustowError> {
    let mut all_planned_actions = collect_package_actions(config, plan_delete_actions)?;
    sort_deletion_actions(&mut all_planned_actions);

    Ok(all_planned_actions)
}

/// Delete (unstow) packages from the target directory
pub fn delete_packages(config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    if config.packages.is_empty() {
        return Ok(Vec::new());
    }

    let all_planned_actions = plan_delete_package_actions(config)?;
    let mut reports = execute_actions(&all_planned_actions, config)?;
    if reports_allow_refolding(&reports, config) {
        reports.extend(refold_foldable_trees(config, all_planned_actions.iter())?);
    }

    Ok(reports)
}

fn plan_restow_delete_package_actions(config: &Config) -> Result<Vec<TargetAction>, RustowError> {
    let mut all_actions = plan_delete_package_actions(config)?;
    let package_matchers = create_restow_symlink_package_matchers(config)?;
    let mut existing_package_canonical_paths = HashMap::new();
    if config.compat {
        collect_matching_stow_symlinks_under_target_dir(
            &config.target_dir,
            config,
            &package_matchers,
            &mut existing_package_canonical_paths,
            &mut all_actions,
        )?;
    } else {
        collect_matching_stow_symlinks_for_current_package_images(
            config,
            &package_matchers,
            &mut existing_package_canonical_paths,
            &mut all_actions,
        )?;
    }

    sort_deletion_actions(&mut all_actions);
    deduplicate_delete_actions(&mut all_actions);
    Ok(all_actions)
}

fn collect_matching_stow_symlinks_for_current_package_images(
    config: &Config,
    package_matchers: &[RestowSymlinkPackageMatcher],
    existing_package_canonical_paths: &mut HashMap<String, Option<PathBuf>>,
    actions: &mut Vec<TargetAction>,
) -> Result<(), RustowError> {
    let mut candidate_target_dirs = Vec::new();

    for package_matcher in package_matchers {
        let package_path = validated_package_path(&config.stow_dir, &package_matcher.name)?;
        let ignore_patterns = &package_matcher.ignore_patterns;
        let raw_items = load_package_items(&package_path, &package_matcher.name)?;

        for raw_item in raw_items {
            if raw_item.item_type != fs_utils::RawStowItemType::Directory {
                continue;
            }

            let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
                raw_item.package_relative_path.to_str().unwrap_or(""),
                config.dotfiles,
            ));

            if should_ignore_item(&processed_target_relative_path, ignore_patterns) {
                continue;
            }

            candidate_target_dirs.push(config.target_dir.join(processed_target_relative_path));
        }
    }

    candidate_target_dirs.sort_by(|a, b| path_depth(a).cmp(&path_depth(b)).then_with(|| a.cmp(b)));

    let mut deduplicated_target_dirs: Vec<PathBuf> = Vec::new();
    for target_dir in candidate_target_dirs {
        if deduplicated_target_dirs
            .iter()
            .any(|scanned| target_dir.starts_with(scanned))
        {
            continue;
        }

        deduplicated_target_dirs.push(target_dir);
    }

    for target_dir in deduplicated_target_dirs {
        collect_matching_stow_symlinks_under_target_dir(
            &target_dir,
            config,
            package_matchers,
            existing_package_canonical_paths,
            actions,
        )?;
    }

    Ok(())
}

struct RestowSymlinkPackageMatcher {
    name: String,
    canonical_path: Option<PathBuf>,
    ignore_patterns: IgnorePatterns,
}

fn create_restow_symlink_package_matchers(
    config: &Config,
) -> Result<Vec<RestowSymlinkPackageMatcher>, RustowError> {
    config
        .packages
        .iter()
        .map(|package_name| {
            Ok(RestowSymlinkPackageMatcher {
                name: package_name.clone(),
                canonical_path: canonical_package_path(&config.stow_dir, package_name).ok(),
                ignore_patterns: load_ignore_patterns_for_package(package_name, config)?,
            })
        })
        .collect()
}

fn collect_matching_stow_symlinks_under_target_dir(
    target_path: &Path,
    config: &Config,
    package_matchers: &[RestowSymlinkPackageMatcher],
    existing_package_canonical_paths: &mut HashMap<String, Option<PathBuf>>,
    actions: &mut Vec<TargetAction>,
) -> Result<bool, RustowError> {
    if path_has_symlink_ancestor(target_path, &config.target_dir) {
        return Ok(false);
    }

    if fs_utils::is_symlink(target_path) {
        if symlink_matches_any_package_target_path(
            target_path,
            config,
            package_matchers,
            existing_package_canonical_paths,
        )? {
            actions.push(create_delete_symlink_action(target_path.to_path_buf()));
            return Ok(true);
        }

        return Ok(false);
    }

    if !fs_utils::path_exists(target_path) {
        return Ok(false);
    }

    if !fs_utils::is_directory(target_path) {
        return Ok(false);
    }

    let mut contains_package_symlink = false;
    for entry in read_directory_entries(target_path)? {
        let path = read_directory_entry(entry, target_path)?.path();
        if path == config.stow_dir || path.starts_with(&config.stow_dir) {
            continue;
        }

        if fs_utils::is_symlink(&path) {
            if symlink_matches_any_package_target_path(
                &path,
                config,
                package_matchers,
                existing_package_canonical_paths,
            )? {
                actions.push(create_delete_symlink_action(path));
                contains_package_symlink = true;
            }
        } else if fs_utils::is_directory(&path)
            && collect_matching_stow_symlinks_under_target_dir(
                &path,
                config,
                package_matchers,
                existing_package_canonical_paths,
                actions,
            )?
        {
            actions.push(create_delete_directory_action(path));
            contains_package_symlink = true;
        }
    }

    Ok(contains_package_symlink)
}

fn symlink_matches_any_package_target_path(
    target_path: &Path,
    config: &Config,
    package_matchers: &[RestowSymlinkPackageMatcher],
    existing_package_canonical_paths: &mut HashMap<String, Option<PathBuf>>,
) -> Result<bool, RustowError> {
    let Some((existing_package_name, item_path)) =
        lexical_stow_symlink_package_and_item_path(target_path, &config.stow_dir)?
    else {
        return Ok(false);
    };

    let Ok(target_relative_path) = target_path.strip_prefix(&config.target_dir) else {
        return Ok(false);
    };
    let processed_item_path = PathBuf::from(dotfiles::process_item_name(
        item_path.to_str().unwrap_or(""),
        config.dotfiles,
    ));
    if processed_item_path != target_relative_path {
        return Ok(false);
    }

    for package_matcher in package_matchers {
        if target_relative_path_is_ignored(target_relative_path, &package_matcher.ignore_patterns) {
            continue;
        }

        if existing_package_name == package_matcher.name {
            return Ok(true);
        }

        let existing_canonical_path = existing_package_canonical_paths
            .entry(existing_package_name.clone())
            .or_insert_with(|| {
                canonical_package_path(&config.stow_dir, &existing_package_name).ok()
            });
        if matches!(
            (
                existing_canonical_path.as_ref(),
                package_matcher.canonical_path.as_ref(),
            ),
            (Some(existing_path), Some(requested_path)) if existing_path == requested_path
        ) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn target_relative_path_is_ignored(
    target_relative_path: &Path,
    ignore_patterns: &IgnorePatterns,
) -> bool {
    should_ignore_item(target_relative_path, ignore_patterns)
}

pub fn restow_packages(config: &Config) -> Result<Vec<TargetActionReport>, RustowError> {
    let delete_actions = plan_restow_delete_package_actions(config)?;
    let mut stow_actions = plan_stow_package_actions(config)?;

    reconcile_stow_actions_with_delete_phase(
        &mut stow_actions,
        &delete_actions,
        &[],
        &[],
        &config.packages,
        config,
    )?;
    apply_conflict_resolution(&mut stow_actions, config);

    let mut reports = execute_delete_then_stow_actions(&delete_actions, &stow_actions, config)?;
    if reports_allow_refolding(&reports, config) {
        reports.extend(refold_foldable_trees(
            config,
            delete_actions.iter().chain(stow_actions.iter()),
        )?);
    }

    Ok(reports)
}

pub fn mixed_packages(
    config: &Config,
    delete_packages: &[String],
    stow_packages: &[String],
    restow_packages: &[String],
) -> Result<Vec<TargetActionReport>, RustowError> {
    let (delete_packages, stow_packages, restow_packages) =
        normalize_mixed_package_sets(delete_packages, stow_packages, restow_packages);
    let mut delete_actions = Vec::new();

    let delete_config = config_for_packages(config, &delete_packages);
    delete_actions.extend(plan_delete_package_actions(&delete_config)?);

    let restow_delete_config = config_for_packages(config, &restow_packages);
    delete_actions.extend(plan_restow_delete_package_actions(&restow_delete_config)?);
    sort_deletion_actions(&mut delete_actions);
    deduplicate_delete_symlink_actions(&mut delete_actions);

    let mut stow_phase_packages = stow_packages.clone();
    stow_phase_packages.extend_from_slice(&restow_packages);
    let stow_config = config_for_packages(config, &stow_phase_packages);
    let mut stow_actions = plan_stow_package_actions(&stow_config)?;

    reconcile_stow_actions_with_delete_phase(
        &mut stow_actions,
        &delete_actions,
        &delete_packages,
        &stow_packages,
        &restow_packages,
        config,
    )?;
    apply_conflict_resolution(&mut stow_actions, config);

    let mut reports = execute_delete_then_stow_actions(&delete_actions, &stow_actions, config)?;
    if reports_allow_refolding(&reports, config) {
        reports.extend(refold_foldable_trees(
            config,
            delete_actions.iter().chain(stow_actions.iter()),
        )?);
    }

    Ok(reports)
}

fn normalize_mixed_package_sets(
    delete_packages: &[String],
    stow_packages: &[String],
    restow_packages: &[String],
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let delete_set: HashSet<String> = delete_packages.iter().cloned().collect();
    let restow_set: HashSet<String> = restow_packages.iter().cloned().collect();
    let stow_set: HashSet<String> = stow_packages.iter().cloned().collect();

    let mut normalized_restow = Vec::new();
    for package_name in restow_packages {
        if !normalized_restow.contains(package_name) {
            normalized_restow.push(package_name.clone());
        }
    }
    for package_name in delete_packages {
        if stow_set.contains(package_name)
            && !restow_set.contains(package_name)
            && !normalized_restow.contains(package_name)
        {
            normalized_restow.push(package_name.clone());
        }
    }

    let normalized_delete = delete_packages
        .iter()
        .filter(|package_name| {
            !restow_set.contains(*package_name) && !stow_set.contains(*package_name)
        })
        .cloned()
        .collect();
    let mut normalized_stow = Vec::new();
    for package_name in stow_packages {
        if !restow_set.contains(package_name)
            && !delete_set.contains(package_name)
            && !normalized_stow.contains(package_name)
        {
            normalized_stow.push(package_name.clone());
        }
    }

    (normalized_delete, normalized_stow, normalized_restow)
}

fn execute_delete_then_stow_actions(
    delete_actions: &[TargetAction],
    stow_actions: &[TargetAction],
    config: &Config,
) -> Result<Vec<TargetActionReport>, RustowError> {
    if delete_actions
        .iter()
        .chain(stow_actions.iter())
        .any(|action| action.action_type == ActionType::Conflict)
    {
        let mut all_actions = delete_actions.to_vec();
        all_actions.extend_from_slice(stow_actions);
        return execute_actions(&all_actions, config);
    }

    let mut reports = execute_actions(delete_actions, config)?;
    if target_action_reports_have_blocking_status(&reports) {
        return Ok(reports);
    }

    reports.extend(execute_actions(stow_actions, config)?);
    Ok(reports)
}

fn config_for_packages(config: &Config, packages: &[String]) -> Config {
    let mut operation_config = config.clone();
    operation_config.packages = packages.to_vec();
    operation_config
}

fn deduplicate_delete_symlink_actions(actions: &mut Vec<TargetAction>) {
    deduplicate_delete_actions(actions);
}

fn deduplicate_delete_actions(actions: &mut Vec<TargetAction>) {
    let mut seen_symlink_targets = HashSet::new();
    let mut seen_directory_targets = HashSet::new();

    actions.retain(|action| match action.action_type {
        ActionType::DeleteSymlink => seen_symlink_targets.insert(action.target_path.clone()),
        ActionType::DeleteDirectory => seen_directory_targets.insert(action.target_path.clone()),
        _ => true,
    });
}

fn reconcile_stow_actions_with_delete_phase(
    stow_actions: &mut Vec<TargetAction>,
    delete_actions: &[TargetAction],
    delete_packages: &[String],
    stow_packages: &[String],
    restow_packages: &[String],
    config: &Config,
) -> Result<(), RustowError> {
    let removed_targets = collect_targets_removed_by_delete_phase(delete_actions)?;
    let open_directory_targets =
        removed_directory_targets_to_keep_open(stow_actions, &removed_targets, config)?;
    let stowed_packages: HashSet<&str> = stow_packages
        .iter()
        .chain(restow_packages.iter())
        .map(String::as_str)
        .collect();
    let delete_only_packages: HashSet<&str> = delete_packages
        .iter()
        .map(String::as_str)
        .filter(|package_name| !stowed_packages.contains(package_name))
        .collect();

    stow_actions.retain(|action| {
        action.action_type != ActionType::DeleteSymlink
            || !removed_targets.contains(&action.target_path)
    });
    stow_actions.retain(|action| {
        if matches!(
            action.action_type,
            ActionType::CreateSymlink | ActionType::CreateDirectory
        ) {
            if let Some(package_name) = action_package_name(action, config) {
                return !delete_only_packages.contains(package_name.as_str());
            }
        }

        true
    });

    let mut folded_targets = Vec::new();
    for action in stow_actions.iter_mut() {
        if !target_removed_by_delete_phase(&action.target_path, &removed_targets) {
            continue;
        }

        if matches!(action.action_type, ActionType::Conflict | ActionType::Skip) {
            let Some(stow_item) = action.source_item.as_ref() else {
                continue;
            };

            if open_directory_targets.contains(&action.target_path) {
                action.action_type = ActionType::CreateDirectory;
                action.link_target_path = None;
                action.conflict_details = None;
                continue;
            }

            if action.link_target_path.is_none() {
                action.link_target_path = Some(calculate_link_target_for_source(
                    &stow_item.source_path,
                    &action.target_path,
                ));
            }

            if action.link_target_path.is_some() {
                action.action_type = ActionType::CreateSymlink;
                action.conflict_details = None;
                if stow_item.item_type == StowItemType::Directory {
                    if let Some(package_name) = action_package_name(action, config) {
                        folded_targets.push((action.target_path.clone(), package_name));
                    }
                }
            }
        }
    }

    prune_descendants_for_folded_targets(stow_actions, &folded_targets, config);
    Ok(())
}

fn removed_directory_targets_to_keep_open(
    stow_actions: &[TargetAction],
    removed_targets: &HashSet<PathBuf>,
    config: &Config,
) -> Result<HashSet<PathBuf>, RustowError> {
    let mut open_directory_targets = HashSet::new();
    let mut ignore_pattern_cache: HashMap<String, IgnorePatterns> = HashMap::new();
    let mut ignored_descendant_cache: HashMap<(String, PathBuf), bool> = HashMap::new();
    let mut deferred_descendant_cache: HashMap<(String, PathBuf), bool> = HashMap::new();

    for (index, action) in stow_actions.iter().enumerate() {
        if !matches!(action.action_type, ActionType::Conflict | ActionType::Skip)
            || !target_removed_by_delete_phase(&action.target_path, removed_targets)
        {
            continue;
        }

        let Some(stow_item) = action.source_item.as_ref() else {
            continue;
        };
        if stow_item.item_type != StowItemType::Directory {
            continue;
        }

        let Some(package_name) = action_package_name(action, config) else {
            continue;
        };

        let ignore_patterns = if let Some(patterns) = ignore_pattern_cache.get(&package_name) {
            patterns.clone()
        } else {
            let patterns = load_ignore_patterns_for_package(&package_name, config)?;
            ignore_pattern_cache.insert(package_name.clone(), patterns.clone());
            patterns
        };

        let cache_key = (
            package_name.clone(),
            stow_item.package_relative_path.clone(),
        );
        let has_ignored_descendants = if let Some(result) = ignored_descendant_cache.get(&cache_key)
        {
            *result
        } else {
            let result = directory_contains_ignored_descendants(
                &package_name,
                &stow_item.package_relative_path,
                config,
                &ignore_patterns,
            )?;
            ignored_descendant_cache.insert(cache_key.clone(), result);
            result
        };
        let has_deferred_descendants =
            if let Some(result) = deferred_descendant_cache.get(&cache_key) {
                *result
            } else {
                let result = directory_contains_deferred_descendants(
                    &package_name,
                    &stow_item.package_relative_path,
                    config,
                )?;
                deferred_descendant_cache.insert(cache_key, result);
                result
            };

        if config.no_folding
            || has_ignored_descendants
            || has_deferred_descendants
            || target_has_other_package_actions(
                index,
                stow_actions,
                config,
                &action.target_path,
                &package_name,
            )
        {
            open_directory_targets.insert(action.target_path.clone());
        }
    }

    Ok(open_directory_targets)
}

fn collect_targets_removed_by_delete_phase(
    delete_actions: &[TargetAction],
) -> Result<HashSet<PathBuf>, RustowError> {
    let mut removed_targets: HashSet<PathBuf> = delete_actions
        .iter()
        .filter(|action| action.action_type == ActionType::DeleteSymlink)
        .map(|action| action.target_path.clone())
        .collect();

    let mut directory_targets: Vec<PathBuf> = delete_actions
        .iter()
        .filter(|action| action.action_type == ActionType::DeleteDirectory)
        .map(|action| action.target_path.clone())
        .collect();
    directory_targets.sort_by_key(|path| std::cmp::Reverse(path_depth(path)));

    for directory_target in directory_targets {
        if planned_directory_will_be_empty(&directory_target, &removed_targets)? {
            removed_targets.insert(directory_target);
        }
    }

    Ok(removed_targets)
}

fn planned_directory_will_be_empty(
    directory_target: &Path,
    removed_targets: &HashSet<PathBuf>,
) -> Result<bool, RustowError> {
    if !fs_utils::path_exists(directory_target) {
        return Ok(true);
    }

    if !fs_utils::is_directory(directory_target) || fs_utils::is_symlink(directory_target) {
        return Ok(false);
    }

    for entry in read_directory_entries(directory_target)? {
        if !removed_targets.contains(&read_directory_entry(entry, directory_target)?.path()) {
            return Ok(false);
        }
    }

    Ok(true)
}

fn target_removed_by_delete_phase(target_path: &Path, removed_targets: &HashSet<PathBuf>) -> bool {
    target_path
        .ancestors()
        .any(|candidate| removed_targets.contains(candidate))
}

/// Sort deletion actions to ensure proper deletion order
fn sort_deletion_actions(actions: &mut [TargetAction]) {
    actions.sort_by(|a, b| {
        deletion_action_rank(&a.action_type)
            .cmp(&deletion_action_rank(&b.action_type))
            .then_with(|| path_depth(&b.target_path).cmp(&path_depth(&a.target_path)))
            .then_with(|| a.target_path.cmp(&b.target_path))
    });
}

fn deletion_action_rank(action_type: &ActionType) -> u8 {
    match action_type {
        ActionType::DeleteSymlink => 0,
        ActionType::DeleteDirectory => 1,
        _ => 2,
    }
}

fn path_has_symlink_ancestor(path: &Path, root: &Path) -> bool {
    let Ok(relative_path) = path.strip_prefix(root) else {
        return false;
    };

    let mut current = root.to_path_buf();
    let mut components = relative_path.components().peekable();
    while let Some(component) = components.next() {
        current.push(component.as_os_str());
        if components.peek().is_some() && fs_utils::is_symlink(&current) {
            return true;
        }
    }

    false
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

/// Normalize path by resolving .. and . components manually
fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized_components = Vec::new();

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized_components.pop();
            },
            std::path::Component::CurDir => {
                // Skip current directory components
            },
            other => {
                normalized_components.push(other);
            },
        }
    }

    normalized_components.iter().collect()
}

/// Check if a directory is empty
fn is_directory_empty(dir_path: &Path) -> Result<bool, RustowError> {
    Ok(read_directory_entries(dir_path)?
        .next()
        .transpose()
        .map_err(|e| {
            RustowError::Fs(FsError::Io {
                path: dir_path.to_path_buf(),
                source: e,
            })
        })?
        .is_none())
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

/// Create a create directory action without a package source item
fn create_create_directory_action(target_path: PathBuf) -> TargetAction {
    TargetAction {
        source_item: None,
        target_path,
        link_target_path: None,
        action_type: ActionType::CreateDirectory,
        conflict_details: None,
    }
}

fn create_delete_directory_action(target_path: PathBuf) -> TargetAction {
    TargetAction {
        source_item: None,
        target_path,
        link_target_path: None,
        action_type: ActionType::DeleteDirectory,
        conflict_details: None,
    }
}

/// Process all raw items to create deletion actions
fn process_deletion_items(
    raw_items: Vec<fs_utils::RawStowItem>,
    config: &Config,
    current_ignore_patterns: &IgnorePatterns,
    package_name: &str,
) -> Result<Vec<TargetAction>, RustowError> {
    let mut actions = Vec::new();

    for raw_item in raw_items {
        if let Some(action) =
            process_item_for_deletion(raw_item, config, current_ignore_patterns, package_name)?
        {
            actions.push(action);
        }
    }

    prune_descendants_of_folded_delete_actions(&mut actions, config);
    sort_deletion_actions(&mut actions);

    Ok(actions)
}

/// Plan actions for deleting (unstowing) a package
fn plan_delete_actions(
    package_name: &str,
    config: &Config,
    current_ignore_patterns: &IgnorePatterns,
) -> Result<Vec<TargetAction>, RustowError> {
    let package_path = validated_package_path(&config.stow_dir, package_name)?;

    let raw_items = load_package_items(&package_path, package_name)?;
    process_deletion_items(raw_items, config, current_ignore_patterns, package_name)
}

pub fn validate_package_for_operation(
    stow_dir: &Path,
    package_name: &str,
) -> Result<(), RustowError> {
    validate_package_for_operation_with_display(stow_dir, package_name, None, None)
}

pub(crate) fn validate_package_for_operation_with_display(
    stow_dir: &Path,
    package_name: &str,
    package_path_display: Option<&str>,
    stow_dir_display: Option<&str>,
) -> Result<(), RustowError> {
    validated_package_path_with_display(
        stow_dir,
        package_name,
        package_path_display,
        stow_dir_display,
    )
    .map(|_| ())
}

fn validated_package_path(stow_dir: &Path, package_name: &str) -> Result<PathBuf, RustowError> {
    validated_package_path_with_display(stow_dir, package_name, None, None)
}

fn validated_package_path_with_display(
    stow_dir: &Path,
    package_name: &str,
    package_path_display: Option<&str>,
    stow_dir_display: Option<&str>,
) -> Result<PathBuf, RustowError> {
    validate_relative_package_name(package_name)?;

    let package_path = stow_dir.join(package_name);
    validate_package_path(&package_path, package_name, package_path_display)?;

    let canonical_package_path = fs_utils::canonicalize_path(&package_path)?;
    let canonical_stow_dir = fs_utils::canonicalize_path(stow_dir)?;

    if canonical_package_path == canonical_stow_dir {
        return Err(StowError::InvalidPackageStructure(format!(
            "Package '{}' resolves to the stow directory itself",
            package_name
        ))
        .into());
    }

    if !canonical_package_path.starts_with(&canonical_stow_dir) {
        return Err(StowError::InvalidPackageStructure(format!(
            "Package '{}' resolves outside stow directory '{}'",
            package_name,
            stow_dir_display
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| canonical_stow_dir.display().to_string())
        ))
        .into());
    }

    Ok(package_path)
}

fn canonical_package_path(stow_dir: &Path, package_name: &str) -> Result<PathBuf, RustowError> {
    validate_relative_package_name(package_name)?;

    let package_path = stow_dir.join(package_name);
    let canonical_package_path = fs_utils::canonicalize_path(&package_path)?;
    let canonical_stow_dir = fs_utils::canonicalize_path(stow_dir)?;

    if canonical_package_path == canonical_stow_dir {
        return Err(StowError::InvalidPackageStructure(format!(
            "Package '{}' resolves to the stow directory itself",
            package_name
        ))
        .into());
    }

    if !canonical_package_path.starts_with(&canonical_stow_dir) {
        return Err(StowError::InvalidPackageStructure(format!(
            "Package '{}' resolves outside stow directory '{}'",
            package_name,
            canonical_stow_dir.display()
        ))
        .into());
    }

    Ok(canonical_package_path)
}

fn validate_relative_package_name(package_name: &str) -> Result<(), RustowError> {
    let package_path = Path::new(package_name);
    let escapes_stow_dir = package_path.is_absolute()
        || package_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        });

    if package_name.is_empty() || escapes_stow_dir {
        return Err(StowError::InvalidPackageStructure(format!(
            "Invalid package name '{}'",
            package_name
        ))
        .into());
    }

    Ok(())
}

fn target_action_reports_have_blocking_status(reports: &[TargetActionReport]) -> bool {
    reports.iter().any(|report| {
        matches!(
            report.status,
            TargetActionReportStatus::ConflictPrevented | TargetActionReportStatus::Failure(_)
        )
    })
}

/// Validate that the package path exists and is a directory
fn validate_package_path(
    package_path: &Path,
    package_name: &str,
    package_path_display: Option<&str>,
) -> Result<(), RustowError> {
    if !fs_utils::path_exists(package_path) {
        return Err(StowError::PackageNotFound(package_name.to_string()).into());
    }

    if !fs_utils::is_directory(package_path) {
        return Err(StowError::InvalidPackageStructure(format!(
            "Package '{}' is not a directory at {}",
            package_name,
            package_path_display
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{:?}", package_path))
        ))
        .into());
    }

    Ok(())
}

/// Load all items from a package directory
fn load_package_items(
    package_path: &Path,
    package_name: &str,
) -> Result<Vec<fs_utils::RawStowItem>, RustowError> {
    match fs_utils::walk_package_dir(package_path) {
        Ok(items) => Ok(items),
        Err(RustowError::Fs(FsError::NotFound(_))) => {
            Err(StowError::PackageNotFound(package_name.to_string()).into())
        },
        Err(e) => Err(e),
    }
}

/// Process a single item for deletion, returning an action if needed
fn process_item_for_deletion(
    raw_item: fs_utils::RawStowItem,
    config: &Config,
    current_ignore_patterns: &IgnorePatterns,
    package_name: &str,
) -> Result<Option<TargetAction>, RustowError> {
    let processed_target_relative_path = PathBuf::from(dotfiles::process_item_name(
        raw_item.package_relative_path.to_str().unwrap_or(""),
        config.dotfiles,
    ));

    // Check if item should be ignored
    if should_ignore_item(&processed_target_relative_path, current_ignore_patterns) {
        return Ok(None);
    }

    let target_path_abs = config.target_dir.join(&processed_target_relative_path);
    let stow_item = create_stow_item_from_raw(raw_item, processed_target_relative_path);

    let action = if let Some(conflict) =
        create_conflict_for_symlinked_target_ancestor(&stow_item, &target_path_abs, config)
    {
        conflict
    } else if fs_utils::path_exists(&target_path_abs) {
        plan_deletion_for_existing_target(&stow_item, &target_path_abs, config, package_name)?
    } else {
        create_skip_action_for_missing_target(stow_item, target_path_abs)
    };

    Ok(Some(action))
}

fn create_conflict_for_symlinked_target_ancestor(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config,
) -> Option<TargetAction> {
    let symlink_path = target_symlink_ancestor_path(target_path_abs, &config.target_dir, false)?;

    Some(TargetAction {
        source_item: Some(stow_item.clone()),
        target_path: target_path_abs.to_path_buf(),
        link_target_path: None,
        action_type: ActionType::Conflict,
        conflict_details: Some(format!(
            "Refusing to delete through symlinked target ancestor {:?}",
            symlink_path
        )),
    })
}

/// Prepare paths for ignore pattern checking
fn prepare_ignore_check_paths(processed_target_relative_path: &Path) -> (PathBuf, String) {
    let path_for_ignore_check_fullpath = PathBuf::from("/").join(processed_target_relative_path);
    let basename_for_ignore_check = processed_target_relative_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    (path_for_ignore_check_fullpath, basename_for_ignore_check)
}

/// Check if an item should be ignored based on ignore patterns
fn should_ignore_item(
    processed_target_relative_path: &Path,
    current_ignore_patterns: &IgnorePatterns,
) -> bool {
    let (path_for_ignore_check_fullpath, basename_for_ignore_check) =
        prepare_ignore_check_paths(processed_target_relative_path);

    ignore::is_ignored(
        &path_for_ignore_check_fullpath,
        &basename_for_ignore_check,
        current_ignore_patterns,
    )
}

/// Create a StowItem from a RawStowItem
fn create_stow_item_from_raw(
    raw_item: fs_utils::RawStowItem,
    processed_target_relative_path: PathBuf,
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
    config: &Config,
    package_name: &str,
) -> Result<TargetAction, RustowError> {
    let (action_type, conflict_details) = match stow_item.item_type {
        StowItemType::Directory => {
            determine_directory_deletion_action(stow_item, target_path_abs, config, package_name)?
        },
        StowItemType::File | StowItemType::Symlink => {
            determine_file_deletion_action(stow_item, target_path_abs, config, package_name)?
        },
    };

    Ok(TargetAction {
        source_item: Some(stow_item.clone()),
        target_path: target_path_abs.to_path_buf(),
        link_target_path: None,
        action_type,
        conflict_details,
    })
}

fn prune_descendants_of_folded_delete_actions(actions: &mut Vec<TargetAction>, config: &Config) {
    let folded_delete_targets: Vec<(PathBuf, String)> = actions
        .iter()
        .filter(|action| {
            action.action_type == ActionType::DeleteSymlink
                && action
                    .source_item
                    .as_ref()
                    .is_some_and(|item| item.item_type == StowItemType::Directory)
        })
        .filter_map(|action| {
            action_package_name(action, config)
                .map(|package_name| (action.target_path.clone(), package_name))
        })
        .collect();

    prune_descendants_for_folded_targets(actions, &folded_delete_targets, config);
}

fn determine_directory_deletion_action(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config,
    package_name: &str,
) -> Result<(ActionType, Option<String>), RustowError> {
    if fs_utils::is_symlink(target_path_abs) {
        return validate_target_for_deletion(target_path_abs, stow_item, config, package_name);
    }

    if fs_utils::is_directory(target_path_abs) {
        return Ok((ActionType::DeleteDirectory, None));
    }

    Ok((
        ActionType::Skip,
        Some(format!(
            "Target {:?} exists but is not a directory or symlink",
            target_path_abs
        )),
    ))
}

/// Validate if a target is a stow-managed symlink for deletion
fn validate_target_for_deletion(
    target_path_abs: &Path,
    stow_item: &StowItem,
    config: &Config,
    package_name: &str,
) -> Result<(ActionType, Option<String>), RustowError> {
    if !fs_utils::is_symlink(target_path_abs) {
        return Ok((
            ActionType::Skip,
            Some(format!(
                "Target {:?} exists but is not a symlink",
                target_path_abs
            )),
        ));
    }

    match fs_utils::is_stow_symlink(target_path_abs, &config.stow_dir) {
        Ok(Some((existing_package_name, item_path_in_package))) => {
            if is_same_package_for_deletion(&existing_package_name, package_name, config)
                && item_path_in_package == stow_item.package_relative_path
            {
                Ok((ActionType::DeleteSymlink, None))
            } else {
                Ok((
                    ActionType::Skip,
                    Some(format!(
                        "Symlink at {:?} belongs to different package or item: {} {:?}",
                        target_path_abs, existing_package_name, item_path_in_package
                    )),
                ))
            }
        },
        Ok(None) => Ok((
            ActionType::Skip,
            Some(format!(
                "File at {:?} is not a stow-managed symlink",
                target_path_abs
            )),
        )),
        Err(_) => Ok((
            ActionType::Conflict,
            Some(format!("Error checking symlink at {:?}", target_path_abs)),
        )),
    }
}

fn is_same_package_for_deletion(
    existing_package_name: &str,
    requested_package_name: &str,
    config: &Config,
) -> bool {
    is_same_package_name(existing_package_name, requested_package_name, config)
}

fn is_same_package_name(
    existing_package_name: &str,
    requested_package_name: &str,
    config: &Config,
) -> bool {
    if existing_package_name == requested_package_name {
        return true;
    }

    let Ok(existing_package_path) = canonical_package_path(&config.stow_dir, existing_package_name)
    else {
        return false;
    };
    let Ok(requested_package_path) =
        canonical_package_path(&config.stow_dir, requested_package_name)
    else {
        return false;
    };

    existing_package_path == requested_package_path
}

/// Determine the appropriate action for deleting a file or symlink
fn determine_file_deletion_action(
    stow_item: &StowItem,
    target_path_abs: &Path,
    config: &Config,
    package_name: &str,
) -> Result<(ActionType, Option<String>), RustowError> {
    validate_target_for_deletion(target_path_abs, stow_item, config, package_name)
}

/// Create a skip action for a missing target
fn create_skip_action_for_missing_target(
    stow_item: StowItem,
    target_path_abs: PathBuf,
) -> TargetAction {
    TargetAction {
        source_item: Some(stow_item),
        target_path: target_path_abs,
        link_target_path: None,
        action_type: ActionType::Skip,
        conflict_details: Some("Target does not exist, nothing to delete".to_string()),
    }
}

/// Execute an adopt file action (move file from target to stow dir, then create symlink)
fn execute_adopt_file_action(action: &TargetAction, config: &Config) -> TargetActionReport {
    // Extract source item information
    let source_item = match &action.source_item {
        Some(item) => item,
        None => {
            return TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Failure(
                    "AdoptFile action missing source_item".to_string(),
                ),
                message: Some(
                    "AdoptFile action requires source_item to determine destination".to_string(),
                ),
            };
        },
    };

    // Check if target file exists
    if !fs_utils::path_exists(&action.target_path) {
        return TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Skipped,
            message: Some(format!(
                "Target file {:?} does not exist, nothing to adopt",
                action.target_path
            )),
        };
    }

    if let Some(error_report) = ensure_target_path_ancestors_not_symlink(action, config, false) {
        return error_report;
    }

    if let Some(error_report) = ensure_adopt_destination_ancestors_not_symlink(action, source_item)
    {
        return error_report;
    }

    // Ensure the package directory exists
    if let Some(package_dir) = source_item.source_path.parent() {
        if let Err(e) = fs_utils::create_dir_all(package_dir) {
            return TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Failure(e.to_string()),
                message: Some(format!(
                    "Failed to create package directory {:?}: {}",
                    package_dir, e
                )),
            };
        }
    }

    // Move the file from target to package directory
    if let Err(e) = move_file(&action.target_path, &source_item.source_path) {
        return TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to move file from {:?} to {:?}: {}",
                action.target_path, source_item.source_path, e
            )),
        };
    }

    // Create symlink from target to the adopted file
    match &action.link_target_path {
        Some(link_target) => {
            if let Some(error_report) =
                ensure_target_path_ancestors_not_symlink(action, config, false)
            {
                return error_report;
            }

            match fs_utils::create_symlink(&action.target_path, link_target) {
                Ok(_) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Success,
                    message: Some(format!(
                        "Successfully adopted file {:?} and created symlink",
                        action.target_path
                    )),
                },
                Err(e) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Failure(e.to_string()),
                    message: Some(format!(
                        "Adopted file but failed to create symlink {:?} -> {:?}: {}",
                        action.target_path, link_target, e
                    )),
                },
            }
        },
        None => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(
                "AdoptFile action missing link_target_path".to_string(),
            ),
            message: Some(
                "AdoptFile action requires link_target_path to create symlink".to_string(),
            ),
        },
    }
}

/// Execute an adopt directory action (move directory from target to stow dir, then create symlink)
fn execute_adopt_directory_action(action: &TargetAction, config: &Config) -> TargetActionReport {
    // Extract source item information
    let source_item = match &action.source_item {
        Some(item) => item,
        None => {
            return TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Failure(
                    "AdoptDirectory action missing source_item".to_string(),
                ),
                message: Some(
                    "AdoptDirectory action requires source_item to determine destination"
                        .to_string(),
                ),
            };
        },
    };

    // Check if target directory exists
    if !fs_utils::path_exists(&action.target_path) {
        return TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Skipped,
            message: Some(format!(
                "Target directory {:?} does not exist, nothing to adopt",
                action.target_path
            )),
        };
    }

    if let Some(error_report) = ensure_target_path_ancestors_not_symlink(action, config, false) {
        return error_report;
    }

    if let Some(error_report) = ensure_adopt_destination_ancestors_not_symlink(action, source_item)
    {
        return error_report;
    }

    if fs_utils::is_symlink(&action.target_path) {
        return TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(
                "Refusing to adopt symlinked directory".to_string(),
            ),
            message: Some(format!(
                "Refusing to adopt symlinked directory {:?}",
                action.target_path
            )),
        };
    }

    // Ensure the parent package directory exists
    if let Some(package_parent) = source_item.source_path.parent() {
        if let Err(e) = fs_utils::create_dir_all(package_parent) {
            return TargetActionReport {
                original_action: action.clone(),
                status: TargetActionReportStatus::Failure(e.to_string()),
                message: Some(format!(
                    "Failed to create package parent directory {:?}: {}",
                    package_parent, e
                )),
            };
        }
    }

    // Move the directory from target to package directory
    if let Err(e) = move_directory(&action.target_path, &source_item.source_path) {
        return TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(e.to_string()),
            message: Some(format!(
                "Failed to move directory from {:?} to {:?}: {}",
                action.target_path, source_item.source_path, e
            )),
        };
    }

    // Create symlink from target to the adopted directory
    match &action.link_target_path {
        Some(link_target) => {
            if let Some(error_report) =
                ensure_target_path_ancestors_not_symlink(action, config, false)
            {
                return error_report;
            }

            match fs_utils::create_symlink(&action.target_path, link_target) {
                Ok(_) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Success,
                    message: Some(format!(
                        "Successfully adopted directory {:?} and created symlink",
                        action.target_path
                    )),
                },
                Err(e) => TargetActionReport {
                    original_action: action.clone(),
                    status: TargetActionReportStatus::Failure(e.to_string()),
                    message: Some(format!(
                        "Adopted directory but failed to create symlink {:?} -> {:?}: {}",
                        action.target_path, link_target, e
                    )),
                },
            }
        },
        None => TargetActionReport {
            original_action: action.clone(),
            status: TargetActionReportStatus::Failure(
                "AdoptDirectory action missing link_target_path".to_string(),
            ),
            message: Some(
                "AdoptDirectory action requires link_target_path to create symlink".to_string(),
            ),
        },
    }
}

fn ensure_adopt_destination_ancestors_not_symlink(
    action: &TargetAction,
    source_item: &StowItem,
) -> Option<TargetActionReport> {
    let Err(error) =
        ensure_destination_ancestors_not_symlink(&action.target_path, &source_item.source_path)
    else {
        return None;
    };

    Some(TargetActionReport {
        original_action: action.clone(),
        status: TargetActionReportStatus::Failure(error.to_string()),
        message: Some(format!(
            "Failed to move target {:?} to package path {:?}: {}",
            action.target_path, source_item.source_path, error
        )),
    })
}

/// Move a file from source to destination
fn move_file(from: &Path, to: &Path) -> Result<(), crate::error::FsError> {
    ensure_destination_ancestors_not_symlink(from, to)?;

    std::fs::rename(from, to).map_err(|e| crate::error::FsError::MoveItem {
        source_path: from.to_path_buf(),
        destination_path: to.to_path_buf(),
        source_io_error: e,
    })
}

fn ensure_destination_is_not_symlink(from: &Path, to: &Path) -> Result<(), crate::error::FsError> {
    match std::fs::symlink_metadata(to) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(crate::error::FsError::MoveItem {
            source_path: from.to_path_buf(),
            destination_path: to.to_path_buf(),
            source_io_error: std::io::Error::new(
                std::io::ErrorKind::Other,
                "Refusing to merge directory into symlinked destination",
            ),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(crate::error::FsError::MoveItem {
            source_path: from.to_path_buf(),
            destination_path: to.to_path_buf(),
            source_io_error: error,
        }),
    }
}

fn ensure_destination_ancestors_not_symlink(
    from: &Path,
    to: &Path,
) -> Result<(), crate::error::FsError> {
    let mut current = PathBuf::new();

    for component in to.components() {
        current.push(component);
        if current == to {
            break;
        }

        if fs_utils::is_symlink(&current) {
            return Err(crate::error::FsError::MoveItem {
                source_path: from.to_path_buf(),
                destination_path: to.to_path_buf(),
                source_io_error: std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Refusing to move into path containing symlinked ancestor",
                ),
            });
        }
    }

    Ok(())
}

/// Move a directory from source to destination, merging contents if destination exists
fn move_directory(from: &Path, to: &Path) -> Result<(), crate::error::FsError> {
    ensure_destination_ancestors_not_symlink(from, to)?;

    let from_file_type = std::fs::symlink_metadata(from)
        .map_err(|e| crate::error::FsError::MoveItem {
            source_path: from.to_path_buf(),
            destination_path: to.to_path_buf(),
            source_io_error: e,
        })?
        .file_type();

    if from_file_type.is_symlink() || !from_file_type.is_dir() {
        ensure_destination_is_not_symlink(from, to)?;
        return std::fs::rename(from, to).map_err(|e| crate::error::FsError::MoveItem {
            source_path: from.to_path_buf(),
            destination_path: to.to_path_buf(),
            source_io_error: e,
        });
    }

    ensure_destination_is_not_symlink(from, to)?;

    // If destination doesn't exist, simple rename
    if !fs_utils::path_exists(to) {
        return std::fs::rename(from, to).map_err(|e| crate::error::FsError::MoveItem {
            source_path: from.to_path_buf(),
            destination_path: to.to_path_buf(),
            source_io_error: e,
        });
    }

    // If destination exists, we need to merge contents
    move_directory_contents_recursive(from, to)?;

    // Remove the now-empty source directory
    std::fs::remove_dir(from).map_err(|e| crate::error::FsError::MoveItem {
        source_path: from.to_path_buf(),
        destination_path: to.to_path_buf(),
        source_io_error: e,
    })
}

/// Recursively move contents from source directory to destination directory
fn move_directory_contents_recursive(from: &Path, to: &Path) -> Result<(), crate::error::FsError> {
    ensure_destination_ancestors_not_symlink(from, to)?;
    ensure_destination_is_not_symlink(from, to)?;

    // Ensure destination directory exists
    std::fs::create_dir_all(to).map_err(|e| crate::error::FsError::MoveItem {
        source_path: from.to_path_buf(),
        destination_path: to.to_path_buf(),
        source_io_error: e,
    })?;

    // Read all entries in the source directory
    let entries = std::fs::read_dir(from).map_err(|e| crate::error::FsError::MoveItem {
        source_path: from.to_path_buf(),
        destination_path: to.to_path_buf(),
        source_io_error: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| crate::error::FsError::MoveItem {
            source_path: from.to_path_buf(),
            destination_path: to.to_path_buf(),
            source_io_error: e,
        })?;

        let source_path = entry.path();
        let file_name = source_path
            .file_name()
            .ok_or_else(|| crate::error::FsError::MoveItem {
                source_path: from.to_path_buf(),
                destination_path: to.to_path_buf(),
                source_io_error: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid file name",
                ),
            })?;
        let dest_path = to.join(file_name);

        let file_type = entry
            .file_type()
            .map_err(|e| crate::error::FsError::MoveItem {
                source_path: source_path.clone(),
                destination_path: dest_path.clone(),
                source_io_error: e,
            })?;

        if file_type.is_dir() {
            // Recursively move directory contents
            move_directory_contents_recursive(&source_path, &dest_path)?;
            // Remove the now-empty source directory
            std::fs::remove_dir(&source_path).map_err(|e| crate::error::FsError::MoveItem {
                source_path: source_path.clone(),
                destination_path: dest_path.clone(),
                source_io_error: e,
            })?;
        } else {
            // Move file
            std::fs::rename(&source_path, &dest_path).map_err(|e| {
                crate::error::FsError::MoveItem {
                    source_path: source_path.clone(),
                    destination_path: dest_path.clone(),
                    source_io_error: e,
                }
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, StowMode};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_config(target_dir: &Path, stow_dir: &Path) -> Config {
        Config {
            target_dir: target_dir.to_path_buf(),
            stow_dir: stow_dir.to_path_buf(),
            packages: vec!["test_package".to_string()],
            mode: StowMode::Stow,
            stow: false,
            compat: false,
            adopt: false,
            no_folding: false,
            dotfiles: false,
            overrides: vec![],
            defers: vec![],
            ignore_patterns: vec![],
            simulate: false,
            verbosity: 0,
            home_dir: PathBuf::from("/tmp"),
        }
    }

    #[test]
    fn test_execute_delete_then_stow_actions_stops_after_delete_failure() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();
        let not_a_symlink = target_dir.join("not_a_symlink");
        fs::write(&not_a_symlink, "content").unwrap();
        let stow_target = target_dir.join("created_after_delete");
        let config = create_test_config(&target_dir, &stow_dir);

        let delete_actions = vec![TargetAction {
            source_item: None,
            target_path: not_a_symlink,
            link_target_path: None,
            action_type: ActionType::DeleteSymlink,
            conflict_details: None,
        }];
        let stow_actions = vec![TargetAction {
            source_item: None,
            target_path: stow_target.clone(),
            link_target_path: Some(PathBuf::from("../stow/source")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        }];

        let reports =
            execute_delete_then_stow_actions(&delete_actions, &stow_actions, &config).unwrap();

        assert_eq!(reports.len(), 1);
        assert!(matches!(
            reports[0].status,
            TargetActionReportStatus::Failure(_)
        ));
        assert!(!fs_utils::is_symlink(&stow_target));
    }

    #[test]
    fn test_check_directory_for_non_stow_files_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_dir = target_dir.join("test_dir");

        fs::create_dir_all(&test_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let result = check_directory_for_non_stow_files(&test_dir, &config).unwrap();
        assert!(!result, "Empty directory should not contain non-stow files");
    }

    #[test]
    fn test_check_directory_for_non_stow_files_with_regular_file() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_dir = target_dir.join("test_dir");

        fs::create_dir_all(&test_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        // Create a regular file in the directory
        fs::write(test_dir.join("regular_file.txt"), "content").unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let result = check_directory_for_non_stow_files(&test_dir, &config).unwrap();
        assert!(
            result,
            "Directory with regular file should contain non-stow files"
        );
    }

    #[test]
    fn test_check_directory_for_non_stow_files_reports_read_dir_error() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let file_path = target_dir.join("not_a_directory");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&file_path, "content").unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let result = check_directory_for_non_stow_files(&file_path, &config);
        assert!(matches!(
            result,
            Err(RustowError::Fs(FsError::Io { path, .. })) if path == file_path
        ));
    }

    #[test]
    fn test_handle_directory_conflict_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_dir = target_dir.join("test_dir");

        fs::create_dir_all(&test_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let result = handle_directory_conflict(&test_dir, &config).unwrap();
        assert_eq!(result.0, ActionType::CreateDirectory);
        assert!(result.1.is_none());
        assert!(result.2.is_none());
    }

    #[test]
    fn test_handle_directory_conflict_with_non_stow_files() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_dir = target_dir.join("test_dir");

        fs::create_dir_all(&test_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        // Create a regular file in the directory
        fs::write(test_dir.join("regular_file.txt"), "content").unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let result = handle_directory_conflict(&test_dir, &config).unwrap();
        assert_eq!(result.0, ActionType::CreateDirectory);
        assert!(result.1.is_none());
        assert!(result.2.is_none());
    }

    #[test]
    fn test_handle_directory_conflict_with_non_stow_files_and_adopt() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_dir = target_dir.join("test_dir");

        fs::create_dir_all(&test_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        // Create a regular file in the directory
        fs::write(test_dir.join("regular_file.txt"), "content").unwrap();

        let mut config = create_test_config(&target_dir, &stow_dir);
        config.adopt = true;

        let result = handle_directory_conflict(&test_dir, &config).unwrap();
        assert_eq!(result.0, ActionType::AdoptDirectory);
        assert!(result.1.is_some());
        assert!(result.1.unwrap().contains("Adopting existing directory"));
        assert!(result.2.is_none());
    }

    #[test]
    fn test_handle_file_type_conflicts_file_vs_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_dir = target_dir.join("test_dir");

        fs::create_dir_all(&test_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        // Create a StowItem representing a file
        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let link_target = PathBuf::from("../stow/test_package/test_file.txt");

        // Test: trying to create file symlink where directory exists
        let result =
            handle_file_type_conflicts(&stow_item, &test_dir, link_target, &config).unwrap();
        assert_eq!(result.0, ActionType::Conflict);
        assert!(result.1.is_some());
        assert!(result.1.unwrap().contains("Cannot create file symlink"));
    }

    #[test]
    fn test_handle_file_type_conflicts_directory_vs_file() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&test_file, "content").unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        // Create a StowItem representing a directory
        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_dir"),
            source_path: stow_dir.join("test_package").join("test_dir"),
            item_type: StowItemType::Directory,
            target_name_after_dotfiles_processing: PathBuf::from("test_dir"),
        };

        let link_target = PathBuf::from("../stow/test_package/test_dir");

        // Test: trying to create directory where file exists
        let result =
            handle_file_type_conflicts(&stow_item, &test_file, link_target, &config).unwrap();
        assert_eq!(result.0, ActionType::Conflict);
        assert!(result.1.is_some());
        assert!(result.1.unwrap().contains("Cannot create directory"));
    }

    #[test]
    fn test_handle_file_type_conflicts_no_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&test_file, "content").unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        // Create a StowItem representing a file (same type as existing)
        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let link_target = PathBuf::from("../stow/test_package/test_file.txt");

        // Test: file vs file should result in conflict (not stow-managed)
        let result =
            handle_file_type_conflicts(&stow_item, &test_file, link_target, &config).unwrap();
        assert_eq!(result.0, ActionType::Conflict);
        assert!(result.1.is_some());
        assert!(
            result
                .1
                .unwrap()
                .contains("already exists and is not stow-managed")
        );
    }

    #[test]
    fn test_ensure_parent_directory_exists_success() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let target_file = target_dir.join("subdir").join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: target_file,
            link_target_path: Some(PathBuf::from("../stow/test_package/test_file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let result = ensure_parent_directory_exists(&action);
        assert!(
            result.is_none(),
            "Should succeed in creating parent directory"
        );
        assert!(
            target_dir.join("subdir").exists(),
            "Parent directory should be created"
        );
    }

    #[test]
    fn test_remove_existing_target_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let target_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        // Create an existing symlink
        fs_utils::create_symlink(
            &target_file,
            &PathBuf::from("../stow/old_package/test_file.txt"),
        )
        .unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: target_file.clone(),
            link_target_path: Some(PathBuf::from("../stow/test_package/test_file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let result = remove_existing_target(&action);
        assert!(
            result.is_none(),
            "Should succeed in removing existing symlink"
        );
        assert!(!target_file.exists(), "Existing symlink should be removed");
    }

    #[test]
    fn test_remove_existing_target_non_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let target_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::write(&target_file, "content").unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: target_file,
            link_target_path: Some(PathBuf::from("../stow/test_package/test_file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let result = remove_existing_target(&action);
        assert!(result.is_some(), "Should fail when target is not a symlink");

        let error_report = result.unwrap();
        assert!(matches!(
            error_report.status,
            TargetActionReportStatus::Failure(_)
        ));
        assert!(error_report.message.unwrap().contains("cannot override"));
    }

    #[test]
    fn test_create_symlink_with_target_success() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let target_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(stow_dir.join("test_package")).unwrap();
        fs::write(
            stow_dir.join("test_package").join("test_file.txt"),
            "content",
        )
        .unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: target_file.clone(),
            link_target_path: Some(PathBuf::from("../stow/test_package/test_file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let link_target = PathBuf::from("../stow/test_package/test_file.txt");
        let result = create_symlink_with_target(&action, &link_target);

        assert_eq!(result.status, TargetActionReportStatus::Success);
        assert!(target_file.exists(), "Symlink should be created");
        assert!(
            fs_utils::is_symlink(&target_file),
            "Target should be a symlink"
        );
    }

    #[test]
    fn test_check_directory_exists_for_deletion_missing() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let missing_dir = target_dir.join("missing_dir");

        fs::create_dir_all(&target_dir).unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: missing_dir,
            link_target_path: None,
            action_type: ActionType::DeleteDirectory,
            conflict_details: None,
        };

        let result = check_directory_exists_for_deletion(&action);
        assert!(
            result.is_some(),
            "Should return skip report for missing directory"
        );

        let skip_report = result.unwrap();
        assert_eq!(skip_report.status, TargetActionReportStatus::Skipped);
        assert!(skip_report.message.unwrap().contains("does not exist"));
    }

    #[test]
    fn test_check_directory_exists_for_deletion_exists() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let existing_dir = target_dir.join("existing_dir");

        fs::create_dir_all(&existing_dir).unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: existing_dir,
            link_target_path: None,
            action_type: ActionType::DeleteDirectory,
            conflict_details: None,
        };

        let result = check_directory_exists_for_deletion(&action);
        assert!(
            result.is_none(),
            "Should return None for existing directory"
        );
    }

    #[test]
    fn test_validate_directory_empty_for_deletion_empty() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let empty_dir = target_dir.join("empty_dir");

        fs::create_dir_all(&empty_dir).unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: empty_dir,
            link_target_path: None,
            action_type: ActionType::DeleteDirectory,
            conflict_details: None,
        };

        let result = validate_directory_empty_for_deletion(&action);
        assert!(result.is_ok(), "Should succeed for empty directory");
        assert!(result.unwrap(), "Should return true for empty directory");
    }

    #[test]
    fn test_validate_directory_empty_for_deletion_not_empty() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let non_empty_dir = target_dir.join("non_empty_dir");

        fs::create_dir_all(&non_empty_dir).unwrap();
        fs::write(non_empty_dir.join("file.txt"), "content").unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: non_empty_dir,
            link_target_path: None,
            action_type: ActionType::DeleteDirectory,
            conflict_details: None,
        };

        let result = validate_directory_empty_for_deletion(&action);
        assert!(
            result.is_ok(),
            "Should succeed for non-empty directory check"
        );
        assert!(
            !result.unwrap(),
            "Should return false for non-empty directory"
        );
    }

    #[test]
    fn test_perform_directory_deletion_success() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let empty_dir = target_dir.join("empty_dir");

        fs::create_dir_all(&empty_dir).unwrap();

        let action = TargetAction {
            source_item: None,
            target_path: empty_dir.clone(),
            link_target_path: None,
            action_type: ActionType::DeleteDirectory,
            conflict_details: None,
        };

        let result = perform_directory_deletion(&action);
        assert_eq!(result.status, TargetActionReportStatus::Success);
        assert!(!empty_dir.exists(), "Directory should be deleted");
    }

    #[test]
    fn test_validate_stow_symlink_valid() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(stow_dir.join("test_package")).unwrap();
        fs::write(
            stow_dir.join("test_package").join("test_file.txt"),
            "content",
        )
        .unwrap();

        // Create a symlink from target to stow
        let link_target = PathBuf::from("../stow/test_package/test_file.txt");
        fs_utils::create_symlink(&test_file, &link_target).unwrap();

        let result = validate_stow_symlink(&test_file, &stow_dir).unwrap();

        assert!(result.is_some());
        let (package_name, item_path) = result.unwrap();
        assert_eq!(package_name, "test_package");
        assert_eq!(item_path, PathBuf::from("test_file.txt"));
    }

    #[test]
    fn test_validate_stow_symlink_not_stow_managed() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        // Create a symlink to somewhere else
        let link_target = PathBuf::from("../other/file.txt");
        fs_utils::create_symlink(&test_file, &link_target).unwrap();

        let result = validate_stow_symlink(&test_file, &stow_dir).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_is_same_package_and_item_true() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let result = is_same_package_and_item(
            "test_package",
            &PathBuf::from("test_file.txt"),
            &stow_item,
            "test_package",
            &config,
        );

        assert!(result);
    }

    #[test]
    fn test_is_same_package_and_item_different_package() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let result = is_same_package_and_item(
            "other_package",
            &PathBuf::from("test_file.txt"),
            &stow_item,
            "test_package",
            &config,
        );

        assert!(!result);
    }

    #[test]
    fn test_check_parent_path_conflicts_file_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let parent_file = target_dir.join("parent_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::write(&parent_file, "content").unwrap();

        let result = check_parent_path_conflicts(&parent_file, &[]);

        assert!(result.is_some());
        let conflict_info = result.unwrap();
        assert!(matches!(
            conflict_info.conflict_type,
            ParentConflictType::File
        ));
        assert_eq!(conflict_info.parent_path, parent_file);
    }

    #[test]
    fn test_check_parent_path_conflicts_conflict_target() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let parent_dir = target_dir.join("parent_dir");

        fs::create_dir_all(&parent_dir).unwrap();

        let conflicting_action = TargetAction {
            source_item: None,
            target_path: parent_dir.clone(),
            link_target_path: None,
            action_type: ActionType::Conflict,
            conflict_details: Some("Test conflict".to_string()),
        };

        let result = check_parent_path_conflicts(&parent_dir, &[conflicting_action]);

        assert!(result.is_some());
        let conflict_info = result.unwrap();
        assert!(matches!(
            conflict_info.conflict_type,
            ParentConflictType::ConflictTarget
        ));
        assert_eq!(conflict_info.parent_path, parent_dir);
    }

    #[test]
    fn test_check_parent_path_conflicts_no_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let parent_dir = target_dir.join("parent_dir");

        fs::create_dir_all(&parent_dir).unwrap();

        let non_conflicting_action = TargetAction {
            source_item: None,
            target_path: target_dir.join("other_path"),
            link_target_path: None,
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let result = check_parent_path_conflicts(&parent_dir, &[non_conflicting_action]);

        assert!(result.is_none());
    }

    #[test]
    fn test_generate_conflict_message_parent_is_file() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let parent_file = target_dir.join("parent_file.txt");

        let conflict_info = ParentConflictInfo {
            conflict_type: ParentConflictType::File,
            parent_path: parent_file.clone(),
        };

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let action = TargetAction {
            source_item: Some(stow_item),
            target_path: target_dir.join("test_file.txt"),
            link_target_path: None,
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let message = generate_conflict_message(&conflict_info, &action);

        assert!(message.contains("is a file"));
        assert!(message.contains("test_file.txt"));
        assert!(message.contains("needs it to be a directory"));
    }

    #[test]
    fn test_generate_conflict_message_parent_is_conflict_target() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let parent_dir = target_dir.join("parent_dir");

        let conflict_info = ParentConflictInfo {
            conflict_type: ParentConflictType::ConflictTarget,
            parent_path: parent_dir.clone(),
        };

        let action = TargetAction {
            source_item: None,
            target_path: target_dir.join("test_file.txt"),
            link_target_path: None,
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let message = generate_conflict_message(&conflict_info, &action);

        assert!(message.contains("is part of a conflicting item tree"));
        assert!(message.contains(&format!("{:?}", parent_dir)));
    }

    #[test]
    fn test_generate_conflict_message_unknown_source() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let parent_file = target_dir.join("parent_file.txt");

        let conflict_info = ParentConflictInfo {
            conflict_type: ParentConflictType::File,
            parent_path: parent_file.clone(),
        };

        let action = TargetAction {
            source_item: None, // No source item
            target_path: target_dir.join("test_file.txt"),
            link_target_path: None,
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let message = generate_conflict_message(&conflict_info, &action);

        assert!(message.contains("UnknownSource"));
        assert!(message.contains("is a file"));
    }

    #[test]
    fn test_generate_conflict_message_with_no_source_item() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        let conflict_info = ParentConflictInfo {
            conflict_type: ParentConflictType::File,
            parent_path: target_dir.join("parent"),
        };

        let action = TargetAction {
            source_item: None, // No source item
            target_path: target_dir.join("test_file.txt"),
            link_target_path: None,
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        let message = generate_conflict_message(&conflict_info, &action);
        assert!(message.contains("UnknownSource"));
        assert!(message.contains("is a file"));
    }

    #[test]
    fn test_check_file_directory_type_conflicts_file_vs_directory() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_dir = target_dir.join("test_dir");

        fs::create_dir_all(&test_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();

        // Create a StowItem representing a file
        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        // Test: trying to create file symlink where directory exists
        let result = check_file_directory_type_conflicts(&stow_item, &test_dir);
        assert!(result.is_some());
        let (action_type, message) = result.unwrap();
        assert_eq!(action_type, ActionType::Conflict);
        assert!(message.contains("Cannot create file symlink"));
        assert!(message.contains("target is a directory"));
    }

    #[test]
    fn test_check_file_directory_type_conflicts_directory_vs_file() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&test_file, "content").unwrap();

        // Create a StowItem representing a directory
        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_dir"),
            source_path: stow_dir.join("test_package").join("test_dir"),
            item_type: StowItemType::Directory,
            target_name_after_dotfiles_processing: PathBuf::from("test_dir"),
        };

        // Test: trying to create directory where file exists
        let result = check_file_directory_type_conflicts(&stow_item, &test_file);
        assert!(result.is_some());
        let (action_type, message) = result.unwrap();
        assert_eq!(action_type, ActionType::Conflict);
        assert!(message.contains("Cannot create directory"));
        assert!(message.contains("target is a file"));
    }

    #[test]
    fn test_check_file_directory_type_conflicts_no_conflict() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&test_file, "content").unwrap();

        let _config = create_test_config(&target_dir, &stow_dir);

        // Create a StowItem representing a file
        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let result = check_file_directory_type_conflicts(&stow_item, &test_file);
        assert!(result.is_none(), "File-to-file should not conflict");
    }

    #[test]
    fn test_validate_target_for_deletion_not_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let test_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&test_file, "content").unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: stow_dir.join("test_package").join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let result =
            validate_target_for_deletion(&test_file, &stow_item, &config, "test_package").unwrap();
        assert_eq!(result.0, ActionType::Skip);
        assert!(result.1.is_some());
        assert!(result.1.unwrap().contains("exists but is not a symlink"));
    }

    #[test]
    fn test_validate_target_for_deletion_valid_stow_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let package_dir = stow_dir.join("test_package");
        let source_file = package_dir.join("test_file.txt");
        let target_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(&source_file, "content").unwrap();

        // Create a symlink from target to source
        fs_utils::create_symlink(&target_file, &source_file).unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: source_file,
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let result =
            validate_target_for_deletion(&target_file, &stow_item, &config, "test_package")
                .unwrap();
        assert_eq!(result.0, ActionType::DeleteSymlink);
        assert!(result.1.is_none());
    }

    #[test]
    fn test_validate_target_for_deletion_wrong_package_item() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let package_dir = stow_dir.join("test_package");
        let source_file = package_dir.join("different_file.txt");
        let target_file = target_dir.join("test_file.txt");

        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(&source_file, "content").unwrap();

        // Create a symlink from target to a different source file
        fs_utils::create_symlink(&target_file, &source_file).unwrap();

        let config = create_test_config(&target_dir, &stow_dir);

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            source_path: package_dir.join("test_file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("test_file.txt"),
        };

        let result =
            validate_target_for_deletion(&target_file, &stow_item, &config, "test_package")
                .unwrap();
        assert_eq!(result.0, ActionType::Skip);
        assert!(result.1.is_some());
        assert!(
            result
                .1
                .unwrap()
                .contains("belongs to different package or item")
        );
    }

    #[test]
    fn test_prepare_ignore_check_paths_simple_file() {
        let path = Path::new("test_file.txt");
        let (fullpath, basename) = prepare_ignore_check_paths(path);

        assert_eq!(fullpath, PathBuf::from("/test_file.txt"));
        assert_eq!(basename, "test_file.txt");
    }

    #[test]
    fn test_prepare_ignore_check_paths_nested_path() {
        let path = Path::new("dir1/dir2/test_file.txt");
        let (fullpath, basename) = prepare_ignore_check_paths(path);

        assert_eq!(fullpath, PathBuf::from("/dir1/dir2/test_file.txt"));
        assert_eq!(basename, "test_file.txt");
    }

    #[test]
    fn test_prepare_ignore_check_paths_directory() {
        let path = Path::new("test_directory");
        let (fullpath, basename) = prepare_ignore_check_paths(path);

        assert_eq!(fullpath, PathBuf::from("/test_directory"));
        assert_eq!(basename, "test_directory");
    }

    #[test]
    fn test_prepare_ignore_check_paths_nested_directory() {
        let path = Path::new("config/nvim");
        let (fullpath, basename) = prepare_ignore_check_paths(path);

        assert_eq!(fullpath, PathBuf::from("/config/nvim"));
        assert_eq!(basename, "nvim");
    }

    #[test]
    fn test_is_non_stow_entry_regular_file() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let regular_file = temp_dir.path().join("regular_file.txt");

        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&regular_file, "content").unwrap();

        let result = is_non_stow_entry(&regular_file, &stow_dir);
        assert!(result); // Regular file should be considered non-stow
    }

    #[test]
    fn test_is_non_stow_entry_stow_managed_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let package_dir = stow_dir.join("test_package");
        let source_file = package_dir.join("test_file.txt");
        let target_file = temp_dir.path().join("test_file.txt");

        fs::create_dir_all(&package_dir).unwrap();
        fs::write(&source_file, "content").unwrap();

        // Create a symlink from target to source
        fs_utils::create_symlink(&target_file, &source_file).unwrap();

        let result = is_non_stow_entry(&target_file, &stow_dir);
        assert!(!result); // Stow-managed symlink should not be considered non-stow
    }

    #[test]
    fn test_is_non_stow_entry_non_stow_symlink() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let external_file = temp_dir.path().join("external.txt");
        let symlink_file = temp_dir.path().join("symlink_file.txt");

        fs::create_dir_all(&stow_dir).unwrap();
        fs::write(&external_file, "content").unwrap();

        // Create a symlink pointing outside stow directory
        fs_utils::create_symlink(&symlink_file, &external_file).unwrap();

        let result = is_non_stow_entry(&symlink_file, &stow_dir);
        assert!(result); // Non-stow symlink should be considered non-stow
    }

    #[test]
    fn test_prepare_canonical_package_path_valid_package() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let package_dir = stow_dir.join("test_package");

        fs::create_dir_all(&package_dir).unwrap();

        let result = canonical_package_path(&stow_dir, "test_package");
        assert!(result.is_ok());
        let canonical_path = result.unwrap();
        assert!(canonical_path.ends_with("test_package"));
    }

    #[test]
    fn test_prepare_canonical_package_path_nonexistent_package() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");

        fs::create_dir_all(&stow_dir).unwrap();

        let result = canonical_package_path(&stow_dir, "nonexistent_package");
        assert!(result.is_err()); // Should fail for nonexistent package
    }

    #[test]
    fn test_prepare_canonical_package_path_nonexistent_stow_dir() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_stow_dir = temp_dir.path().join("nonexistent");
        let package_name = "test_package";

        let result = canonical_package_path(&nonexistent_stow_dir, package_name);
        assert!(result.is_err());
    }

    #[test]
    fn test_prepare_canonical_package_path_rejects_absolute_package_name() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        fs::create_dir_all(&stow_dir).unwrap();

        let result = canonical_package_path(&stow_dir, "/tmp/outside");
        assert!(result.is_err());
    }

    #[test]
    fn test_prepare_canonical_package_path_rejects_parent_dir_package_name() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        fs::create_dir_all(&stow_dir).unwrap();

        let result = canonical_package_path(&stow_dir, "../outside");
        assert!(result.is_err());
    }

    #[test]
    fn test_prepare_canonical_package_path_rejects_stow_root_package_name() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        fs::create_dir_all(&stow_dir).unwrap();

        let result = canonical_package_path(&stow_dir, ".");
        assert!(result.is_err());
    }

    #[test]
    fn test_prepare_canonical_package_path_rejects_package_symlink_to_stow_root() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        fs::create_dir_all(&stow_dir).unwrap();
        fs_utils::create_symlink(&stow_dir.join("all"), Path::new(".")).unwrap();

        let result = canonical_package_path(&stow_dir, "all");
        assert!(result.is_err());
    }

    #[test]
    fn test_sort_deletion_actions_mixed_types() {
        let mut actions = vec![
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/dir1"),
                link_target_path: None,
                action_type: ActionType::DeleteDirectory,
                conflict_details: None,
            },
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/link1"),
                link_target_path: None,
                action_type: ActionType::DeleteSymlink,
                conflict_details: None,
            },
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/dir2"),
                link_target_path: None,
                action_type: ActionType::DeleteDirectory,
                conflict_details: None,
            },
        ];

        sort_deletion_actions(&mut actions);

        assert!(matches!(actions[0].action_type, ActionType::DeleteSymlink));
        assert!(matches!(
            actions[1].action_type,
            ActionType::DeleteDirectory
        ));
        assert!(matches!(
            actions[2].action_type,
            ActionType::DeleteDirectory
        ));
    }

    #[test]
    fn test_sort_deletion_actions_only_symlinks() {
        let mut actions = vec![
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/link1"),
                link_target_path: None,
                action_type: ActionType::DeleteSymlink,
                conflict_details: None,
            },
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/link2"),
                link_target_path: None,
                action_type: ActionType::DeleteSymlink,
                conflict_details: None,
            },
        ];

        sort_deletion_actions(&mut actions);

        assert!(matches!(actions[0].action_type, ActionType::DeleteSymlink));
        assert!(matches!(actions[1].action_type, ActionType::DeleteSymlink));
    }

    #[test]
    fn test_sort_deletion_actions_orders_equal_depth_by_path() {
        let mut actions = vec![
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/link-b"),
                link_target_path: None,
                action_type: ActionType::DeleteSymlink,
                conflict_details: None,
            },
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/dir-b"),
                link_target_path: None,
                action_type: ActionType::DeleteDirectory,
                conflict_details: None,
            },
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/link-a"),
                link_target_path: None,
                action_type: ActionType::DeleteSymlink,
                conflict_details: None,
            },
            TargetAction {
                source_item: None,
                target_path: PathBuf::from("/tmp/dir-a"),
                link_target_path: None,
                action_type: ActionType::DeleteDirectory,
                conflict_details: None,
            },
        ];

        sort_deletion_actions(&mut actions);

        let sorted_paths: Vec<PathBuf> = actions
            .iter()
            .map(|action| action.target_path.clone())
            .collect();
        assert_eq!(
            sorted_paths,
            vec![
                PathBuf::from("/tmp/link-a"),
                PathBuf::from("/tmp/link-b"),
                PathBuf::from("/tmp/dir-a"),
                PathBuf::from("/tmp/dir-b"),
            ]
        );
    }

    #[test]
    fn test_sort_deletion_actions_empty_list() {
        let mut actions: Vec<TargetAction> = vec![];
        sort_deletion_actions(&mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_apply_conflict_resolution_no_conflicts() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        let mut actions = vec![TargetAction {
            source_item: None,
            target_path: PathBuf::from("/tmp/file1"),
            link_target_path: Some(PathBuf::from("../stow/package/file1")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        }];

        apply_conflict_resolution(&mut actions, &config);

        // Should not change anything when there are no conflicts
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0].action_type, ActionType::CreateSymlink));
        assert!(actions[0].conflict_details.is_none());
    }

    #[test]
    fn test_apply_conflict_resolution_empty_actions() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        let mut actions: Vec<TargetAction> = vec![];

        apply_conflict_resolution(&mut actions, &config);

        // Should handle empty action list gracefully
        assert!(actions.is_empty());
    }

    #[test]
    fn test_apply_conflict_resolution_with_conflicts() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        let mut actions = vec![TargetAction {
            source_item: None,
            target_path: PathBuf::from("/tmp/conflicted_file"),
            link_target_path: Some(PathBuf::from("../stow/package/file")),
            action_type: ActionType::CreateSymlink,
            conflict_details: Some("Mock conflict".to_string()),
        }];

        // Apply conflict resolution (will invoke ConflictResolver)
        apply_conflict_resolution(&mut actions, &config);

        // The function should run without panicking
        // Detailed behavior testing would require more complex setup
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn test_plan_restow_delete_package_actions_empty_packages() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let mut config = create_test_config(&target_dir, &stow_dir);
        config.packages = vec![]; // Empty packages

        let result = plan_restow_delete_package_actions(&config);
        assert!(result.is_ok());
        let actions = result.unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn test_plan_restow_delete_package_actions_nonexistent_package() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let mut config = create_test_config(&target_dir, &stow_dir);
        config.packages = vec!["nonexistent_package".to_string()];

        let result = plan_restow_delete_package_actions(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_restow_delete_package_actions_valid_package() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let package_dir = stow_dir.join("test_package");

        // Create directories
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();

        let mut config = create_test_config(&target_dir, &stow_dir);
        config.packages = vec!["test_package".to_string()];

        let result = plan_restow_delete_package_actions(&config);
        assert!(result.is_ok());
        let actions = result.unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn test_process_deletion_items_empty_list() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        // Load ignore patterns for testing
        let ignore_patterns = load_ignore_patterns_for_package("test_package", &config).unwrap();

        let raw_items = vec![];
        let result = process_deletion_items(raw_items, &config, &ignore_patterns, "test_package");
        assert!(result.is_ok());
        let actions = result.unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn test_process_deletion_items_with_valid_item() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        // Load ignore patterns for testing
        let ignore_patterns = load_ignore_patterns_for_package("test_package", &config).unwrap();

        let raw_items = vec![fs_utils::RawStowItem {
            package_relative_path: PathBuf::from("test_file.txt"),
            absolute_path: stow_dir.join("package").join("test_file.txt"),
            item_type: fs_utils::RawStowItemType::File,
        }];

        let result = process_deletion_items(raw_items, &config, &ignore_patterns, "test_package");
        assert!(result.is_ok());
        let actions = result.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_type, ActionType::Skip); // Target doesn't exist
    }

    #[test]
    fn test_process_deletion_items_with_ignored_item() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        // Load ignore patterns for testing
        let ignore_patterns = load_ignore_patterns_for_package("test_package", &config).unwrap();

        let raw_items = vec![fs_utils::RawStowItem {
            package_relative_path: PathBuf::from("ignored_file.txt"),
            absolute_path: stow_dir.join("package").join("ignored_file.txt"),
            item_type: fs_utils::RawStowItemType::File,
        }];

        let result = process_deletion_items(raw_items, &config, &ignore_patterns, "test_package");
        assert!(result.is_ok());
        let actions = result.unwrap();
        // With current ignore patterns implementation, item should still be processed
        // This test mainly verifies the function doesn't crash with ignore patterns
        assert_eq!(actions.len(), 1);
    }

    #[test]
    fn test_collect_parent_conflict_info_no_conflicts() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        let actions = vec![TargetAction {
            source_item: None,
            target_path: target_dir.join("simple_file.txt"),
            link_target_path: Some(PathBuf::from("../stow/package/simple_file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        }];

        let conflicts = collect_parent_conflict_info(&actions, &config);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_collect_parent_conflict_info_skip_existing_conflicts() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        let actions = vec![TargetAction {
            source_item: None,
            target_path: target_dir.join("conflicted_file.txt"),
            link_target_path: None,
            action_type: ActionType::Conflict,
            conflict_details: Some("Already in conflict".to_string()),
        }];

        let conflicts = collect_parent_conflict_info(&actions, &config);
        assert!(conflicts.is_empty()); // Should skip actions already in conflict
    }

    #[test]
    fn test_collect_parent_conflict_info_with_parent_file_conflict() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let config = create_test_config(&target_dir, &stow_dir);

        // Create a file where a parent directory should be
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("parent"), "file content").unwrap();

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("file.txt"),
            source_path: stow_dir.join("package").join("file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("file.txt"),
        };

        let actions = vec![TargetAction {
            source_item: Some(stow_item),
            target_path: target_dir.join("parent").join("child").join("file.txt"),
            link_target_path: Some(PathBuf::from("../../../stow/package/file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        }];

        let result = collect_parent_conflict_info(&actions, &config);

        assert_eq!(result.len(), 1);
        let (index, conflict_info) = &result[0];
        assert_eq!(*index, 0);
        assert!(matches!(
            conflict_info.conflict_type,
            ParentConflictType::File
        ));
        assert_eq!(conflict_info.parent_path, target_dir.join("parent"));
    }

    #[test]
    fn test_prepare_symlink_creation_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let link_target = PathBuf::from("../stow/package/file.txt");

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("file.txt"),
            source_path: stow_dir.join("package").join("file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("file.txt"),
        };

        let action = TargetAction {
            source_item: Some(stow_item),
            target_path: target_dir.join("subdir").join("file.txt"),
            link_target_path: Some(link_target),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };
        let config = create_test_config(&target_dir, &stow_dir);

        // Should succeed - parent directories don't exist but will be created
        let result = prepare_symlink_creation(&action, &config);
        assert!(
            result.is_none(),
            "Expected success, but got error: {:?}",
            result
        );

        // Verify parent directory was created
        assert!(fs_utils::path_exists(&target_dir.join("subdir")));
    }

    #[test]
    fn test_prepare_symlink_creation_with_existing_symlink() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");

        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::create_dir_all(stow_dir.join("package")).unwrap();
        std::fs::write(stow_dir.join("package").join("file.txt"), "content").unwrap();

        // Create existing symlink
        let existing_target = stow_dir.join("old_package").join("file.txt");
        std::fs::create_dir_all(existing_target.parent().unwrap()).unwrap();
        std::fs::write(&existing_target, "old content").unwrap();
        fs_utils::create_symlink(&target_dir.join("file.txt"), &existing_target).unwrap();

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("file.txt"),
            source_path: stow_dir.join("package").join("file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("file.txt"),
        };

        let action = TargetAction {
            source_item: Some(stow_item),
            target_path: target_dir.join("file.txt"),
            link_target_path: Some(PathBuf::from("../stow/package/file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };
        let config = create_test_config(&target_dir, &stow_dir);

        // Should succeed - existing symlink will be removed
        let result = prepare_symlink_creation(&action, &config);
        assert!(
            result.is_none(),
            "Expected success, but got error: {:?}",
            result
        );

        // Verify old symlink was removed
        assert!(!fs_utils::path_exists(&target_dir.join("file.txt")));
    }

    #[test]
    fn test_prepare_symlink_creation_with_existing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");

        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("file.txt"), "existing content").unwrap();

        let stow_item = StowItem {
            package_relative_path: PathBuf::from("file.txt"),
            source_path: stow_dir.join("package").join("file.txt"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: PathBuf::from("file.txt"),
        };

        let action = TargetAction {
            source_item: Some(stow_item),
            target_path: target_dir.join("file.txt"),
            link_target_path: Some(PathBuf::from("../stow/package/file.txt")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };
        let config = create_test_config(&target_dir, &stow_dir);

        // Should fail - cannot override regular file
        let result = prepare_symlink_creation(&action, &config);
        assert!(
            result.is_some(),
            "Expected failure for existing regular file"
        );

        let report = result.unwrap();
        assert!(matches!(
            report.status,
            TargetActionReportStatus::Failure(_)
        ));
        assert!(report.message.unwrap().contains("cannot override"));
    }

    #[test]
    fn test_execute_adopt_file_action_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let package_dir = stow_dir.join("testpkg");

        // Setup directories
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&package_dir).unwrap();

        // Create existing file in target
        let target_file = target_dir.join("existing_file.txt");
        fs::write(&target_file, "existing content").unwrap();

        // Create package source file path (where the file should be moved to)
        let package_file = package_dir.join("existing_file.txt");

        // Create an AdoptFile action
        let action = TargetAction {
            source_item: Some(StowItem {
                package_relative_path: PathBuf::from("existing_file.txt"),
                source_path: package_file.clone(),
                item_type: StowItemType::File,
                target_name_after_dotfiles_processing: PathBuf::from("existing_file.txt"),
            }),
            target_path: target_file.clone(),
            link_target_path: Some(PathBuf::from("../stow/testpkg/existing_file.txt")),
            action_type: ActionType::AdoptFile,
            conflict_details: None,
        };
        let config = create_test_config(&target_dir, &stow_dir);

        // Execute the action
        let report = execute_real_action(&action, &config);

        // Verify the action was successful
        assert_eq!(report.status, TargetActionReportStatus::Success);

        // Verify file was moved to package directory
        assert!(
            package_file.exists(),
            "File should be moved to package directory"
        );
        assert_eq!(
            fs::read_to_string(&package_file).unwrap(),
            "existing content"
        );

        // Verify symlink was created in target
        assert!(
            target_file.exists(),
            "Symlink should exist in target directory"
        );
        assert!(
            fs::symlink_metadata(&target_file)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        // Verify symlink points to the correct location
        let link_target = fs::read_link(&target_file).unwrap();
        assert_eq!(
            link_target,
            PathBuf::from("../stow/testpkg/existing_file.txt")
        );
    }

    #[test]
    fn test_move_file_rejects_symlinked_destination_ancestor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("source");
        let real_dir = temp_dir.path().join("real");
        let target_dir = temp_dir.path().join("target");

        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&real_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();

        let source_file = source_dir.join("file.txt");
        fs::write(&source_file, "content").unwrap();

        let symlink_parent = target_dir.join("symlink_parent");
        fs_utils::create_symlink(&symlink_parent, &real_dir).unwrap();

        let destination = symlink_parent.join("file.txt");
        let result = move_file(&source_file, &destination);

        assert!(matches!(
            result,
            Err(crate::error::FsError::MoveItem { .. })
        ));
        assert!(
            source_file.exists(),
            "Source file should remain when move is rejected"
        );
        assert!(
            !destination.exists(),
            "Destination file should not be created"
        );
    }

    #[test]
    fn test_move_directory_rejects_symlinked_destination_ancestor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("source_dir");
        let real_dir = temp_dir.path().join("real");
        let target_dir = temp_dir.path().join("target");

        fs::create_dir_all(source_dir.join("nested")).unwrap();
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&real_dir).unwrap();
        fs::write(source_dir.join("nested").join("file.txt"), "content").unwrap();

        let symlink_parent = target_dir.join("symlink_parent");
        fs_utils::create_symlink(&symlink_parent, &real_dir).unwrap();

        let destination_dir = symlink_parent.join("adopted");
        let result = move_directory(&source_dir, &destination_dir);

        assert!(matches!(
            result,
            Err(crate::error::FsError::MoveItem { .. })
        ));
        assert!(
            source_dir.exists(),
            "Source directory should remain when move is rejected"
        );
        assert!(
            !destination_dir.exists(),
            "Destination directory should not be created"
        );
    }

    #[test]
    fn test_plan_adopt_action_for_existing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        let stow_dir = temp_dir.path().join("stow");
        let package_dir = stow_dir.join("testpkg");

        // Setup directories and files
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(package_dir.join("myfile.txt"), "package content").unwrap();
        fs::write(target_dir.join("myfile.txt"), "existing content").unwrap();

        // Create config with adopt enabled
        let mut config = create_test_config(&target_dir, &stow_dir);
        config.adopt = true;

        // Load ignore patterns
        let ignore_patterns =
            IgnorePatterns::load(&stow_dir, Some("testpkg"), &target_dir).unwrap();

        // Plan actions for the package
        let actions = plan_actions("testpkg", &config, &ignore_patterns).unwrap();

        // Should find an AdoptFile action for the conflicting file
        let adopt_action = actions.iter().find(|a| {
            a.action_type == ActionType::AdoptFile
                && a.target_path
                    .file_name()
                    .is_some_and(|name| name == "myfile.txt")
        });

        assert!(
            adopt_action.is_some(),
            "Should plan an AdoptFile action for existing file"
        );
        let adopt_action = adopt_action.unwrap();
        assert_eq!(adopt_action.action_type, ActionType::AdoptFile);
        assert!(adopt_action.source_item.is_some());
    }
}
