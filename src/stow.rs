use crate::config::Config;
use crate::error::{Result, RustowError, StowError};
use crate::fs_utils::{RawStowItem, RawStowItemType};
use crate::ignore::filter_items;
use crate::ignore::IgnorePatterns;
use std::path::PathBuf;
use std::fs;
use tempfile::TempDir;

/// Represents the type of a stow item
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StowItemType {
    File,
    Directory,
    Symlink,
}

/// Represents an item within a package that can be stowed
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StowItem {
    pub package_relative_path: PathBuf, // Path relative to package root
    pub source_path: PathBuf,           // Absolute path in stow directory
    pub item_type: StowItemType,
    pub target_name_after_dotfiles_processing: String, // Name after --dotfiles processing
}

/// Represents the type of action to be performed on a target
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionType {
    CreateSymlink,      // Create a symbolic link
    DeleteSymlink,      // Delete a symbolic link
    CreateDirectory,    // Create a directory (for folding)
    DeleteDirectory,    // Delete an empty directory (during unstow)
    AdoptFile,          // Move file to stow directory (--adopt)
    AdoptDirectory,     // Move directory to stow directory (--adopt)
    Skip,               // Skip operation (--defer)
    Conflict,           // Conflict detected
}

/// Represents an action to be performed on a target path
#[derive(Debug, Clone)]
pub struct TargetAction {
    pub source_item: Option<StowItem>,      // Source stow item (None for delete operations)
    pub target_path: PathBuf,               // Absolute path in target directory
    pub link_target_path: Option<PathBuf>,  // Path that symlink should point to (relative)
    pub action_type: ActionType,
    pub conflict_details: Option<String>,   // Details about conflicts
}

/// Represents a package with its items
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub path: PathBuf,
    pub items: Vec<StowItem>,
}

/// Scan a package directory and create StowItems
pub fn scan_package(
    package_name: &str,
    stow_dir: &PathBuf,
    config: &Config,
) -> Result<Package> {
    let package_path = stow_dir.join(package_name);
    
    // Check if package directory exists
    if !crate::fs_utils::path_exists(&package_path) {
        return Err(RustowError::Stow(StowError::PackageNotFound(
            package_name.to_string(),
        )));
    }
    
    if !crate::fs_utils::is_directory(&package_path) {
        return Err(RustowError::Stow(StowError::InvalidPackageStructure(
            format!("Package path is not a directory: {}", package_path.display()),
        )));
    }
    
    // Walk the package directory to get raw items
    let raw_items = crate::fs_utils::walk_package_dir(&package_path)?;
    
    // Convert raw items to minimal stowable items for filtering
    let minimal_items: Vec<crate::ignore::MinimalStowableItem> = raw_items
        .iter()
        .map(|raw_item| crate::ignore::MinimalStowableItem {
            package_relative_path: raw_item.package_relative_path.clone(),
            basename: raw_item
                .package_relative_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        })
        .collect();
    
    // Filter items using ignore patterns
    let filtered_items = filter_items(minimal_items, &config.ignore_patterns);
    
    // Convert filtered items back to StowItems
    let mut stow_items = Vec::new();
    for filtered_item in filtered_items {
        // Find the corresponding raw item
        if let Some(raw_item) = raw_items
            .iter()
            .find(|r| r.package_relative_path == filtered_item.package_relative_path)
        {
            let stow_item = process_raw_item(raw_item, package_name, stow_dir, config.dotfiles);
            stow_items.push(stow_item);
        }
    }
    
    Ok(Package {
        name: package_name.to_string(),
        path: package_path,
        items: stow_items,
    })
}

/// Main function to stow packages
pub fn stow_packages(config: &Config) -> Result<Vec<TargetAction>> {
    let mut all_actions = Vec::new();
    
    // Scan each package
    for package_name in &config.packages {
        let package = scan_package(package_name, &config.stow_dir, config)?;
        
        // For now, just create basic CreateSymlink actions for each item
        // TODO: Implement proper action planning with conflict detection, folding, etc.
        for item in &package.items {
            let target_path = calculate_target_path(&item, &config.target_dir, config.dotfiles)?;
            let link_target_path = calculate_relative_link_path(&item.source_path, &target_path)?;
            
            let action = TargetAction {
                source_item: Some(item.clone()),
                target_path,
                link_target_path: Some(link_target_path),
                action_type: ActionType::CreateSymlink,
                conflict_details: None,
            };
            
            all_actions.push(action);
        }
    }
    
    Ok(all_actions)
}

/// Calculate the target path for a StowItem
fn calculate_target_path(item: &StowItem, target_dir: &PathBuf, dotfiles_enabled: bool) -> Result<PathBuf> {
    let mut target_path = target_dir.clone();
    
    // Process each component of the path with dotfiles processing
    for component in item.package_relative_path.components() {
        if let std::path::Component::Normal(os_str) = component {
            let component_str = os_str.to_string_lossy();
            // Apply dotfiles processing to each component
            let processed_component = crate::dotfiles::process_item_name(&component_str, dotfiles_enabled);
            target_path = target_path.join(processed_component);
        }
    }
    
    Ok(target_path)
}

/// Calculate the relative path from target to source for symlink creation
fn calculate_relative_link_path(source_path: &PathBuf, target_path: &PathBuf) -> Result<PathBuf> {
    // This is a simplified implementation
    // In a real implementation, we'd calculate the proper relative path
    // For now, we'll use a placeholder
    
    // Get the target directory
    let target_parent = target_path.parent().ok_or_else(|| {
        RustowError::Stow(StowError::InvalidPackageStructure(
            "Target path has no parent directory".to_string(),
        ))
    })?;
    
    // Calculate relative path from target_parent to source_path
    // This is a simplified version - in practice we'd use proper path resolution
    match pathdiff::diff_paths(source_path, target_parent) {
        Some(relative_path) => Ok(relative_path),
        None => Err(RustowError::Stow(StowError::OperationFailed(
            format!(
                "Could not calculate relative path from {} to {}",
                target_parent.display(),
                source_path.display()
            ),
        ))),
    }
}

/// Convert RawStowItem to StowItem with dotfiles processing
fn process_raw_item(
    raw_item: &RawStowItem,
    _package_name: &str,
    _stow_dir: &PathBuf,
    dotfiles_enabled: bool,
) -> StowItem {
    let item_type = match raw_item.item_type {
        RawStowItemType::File => StowItemType::File,
        RawStowItemType::Directory => StowItemType::Directory,
        RawStowItemType::Symlink => StowItemType::Symlink,
    };

    // Apply dotfiles processing to the final component of the path
    let target_name = if let Some(file_name) = raw_item.package_relative_path.file_name() {
        let name_str = file_name.to_string_lossy();
        crate::dotfiles::process_item_name(&name_str, dotfiles_enabled)
    } else {
        // This shouldn't happen for valid paths, but handle gracefully
        raw_item.package_relative_path.to_string_lossy().to_string()
    };

    StowItem {
        package_relative_path: raw_item.package_relative_path.clone(),
        source_path: raw_item.absolute_path.clone(),
        item_type,
        target_name_after_dotfiles_processing: target_name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, StowMode};
    use crate::ignore::IgnorePatterns;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_config(temp_dir: &TempDir) -> Config {
        let stow_dir = temp_dir.path().join("stow");
        let target_dir = temp_dir.path().join("target");
        
        Config {
            target_dir,
            stow_dir,
            packages: vec!["test_package".to_string()],
            mode: StowMode::Stow,
            adopt: false,
            no_folding: false,
            dotfiles: false,
            overrides: vec![],
            defers: vec![],
            simulate: false,
            verbosity: 0,
            ignore_patterns: IgnorePatterns::load(
                &temp_dir.path().join("stow"),
                None,
                &temp_dir.path().join("home"),
            ).unwrap(),
        }
    }

    #[test]
    fn test_scan_package_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);
        
        let result = scan_package("nonexistent_package", &config.stow_dir, &config);
        assert!(result.is_err());
        
        if let Err(RustowError::Stow(StowError::PackageNotFound(name))) = result {
            assert_eq!(name, "nonexistent_package");
        } else {
            panic!("Expected PackageNotFound error");
        }
    }

    #[test]
    fn test_scan_package_simple_structure() {
        let temp_dir = TempDir::new().unwrap();
        let config = create_test_config(&temp_dir);
        
        // Create package directory structure
        let package_dir = config.stow_dir.join("test_package");
        fs::create_dir_all(&package_dir).unwrap();
        
        // Create some files
        let bin_dir = package_dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(bin_dir.join("script1"), "#!/bin/bash\necho hello").unwrap();
        fs::write(package_dir.join("README.md"), "# Test Package").unwrap();
        
        let result = scan_package("test_package", &config.stow_dir, &config);
        assert!(result.is_ok());
        
        let package = result.unwrap();
        assert_eq!(package.name, "test_package");
        assert_eq!(package.path, package_dir);
        
        // Should have 2 items (bin directory and script1 file)
        // README.md should be filtered out by default ignore patterns
        assert_eq!(package.items.len(), 2);
        
        // Check that we have the expected items
        let item_paths: Vec<_> = package.items
            .iter()
            .map(|item| item.package_relative_path.clone())
            .collect();
        assert!(item_paths.contains(&PathBuf::from("bin")));
        assert!(item_paths.contains(&PathBuf::from("bin/script1")));
    }

    #[test]
    fn test_scan_package_with_dotfiles() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = create_test_config(&temp_dir);
        config.dotfiles = true;
        
        // Create package directory structure
        let package_dir = config.stow_dir.join("test_package");
        fs::create_dir_all(&package_dir).unwrap();
        
        // Create dotfiles
        fs::write(package_dir.join("dot-bashrc"), "# bashrc content").unwrap();
        fs::write(package_dir.join("dot-vimrc"), "\" vimrc content").unwrap();
        
        let result = scan_package("test_package", &config.stow_dir, &config);
        assert!(result.is_ok());
        
        let package = result.unwrap();
        assert_eq!(package.items.len(), 2);
        
        // Check dotfiles processing
        for item in &package.items {
            if item.package_relative_path == PathBuf::from("dot-bashrc") {
                assert_eq!(item.target_name_after_dotfiles_processing, ".bashrc");
            } else if item.package_relative_path == PathBuf::from("dot-vimrc") {
                assert_eq!(item.target_name_after_dotfiles_processing, ".vimrc");
            }
        }
    }

    #[test]
    fn test_calculate_target_path() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        
        let item = StowItem {
            package_relative_path: PathBuf::from("bin/script1"),
            source_path: PathBuf::from("/stow/pkg/bin/script1"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: "script1".to_string(),
        };
        
        let result = calculate_target_path(&item, &target_dir, true);
        assert!(result.is_ok());
        
        let target_path = result.unwrap();
        assert_eq!(target_path, target_dir.join("bin").join("script1"));
    }

    #[test]
    fn test_calculate_target_path_with_dotfiles() {
        let temp_dir = TempDir::new().unwrap();
        let target_dir = temp_dir.path().join("target");
        
        let item = StowItem {
            package_relative_path: PathBuf::from("dot-bashrc"),
            source_path: PathBuf::from("/stow/pkg/dot-bashrc"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: ".bashrc".to_string(),
        };
        
        let result = calculate_target_path(&item, &target_dir, true);
        assert!(result.is_ok());
        
        let target_path = result.unwrap();
        assert_eq!(target_path, target_dir.join(".bashrc"));
    }

    #[test]
    fn test_stow_packages_empty_config() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = create_test_config(&temp_dir);
        config.packages = vec![]; // Empty packages list
        
        let result = stow_packages(&config);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_process_raw_item_without_dotfiles() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let package_path = stow_dir.join("test_package");
        
        let raw_item = RawStowItem {
            absolute_path: package_path.join("bin").join("test_script"),
            package_relative_path: PathBuf::from("bin/test_script"),
            item_type: RawStowItemType::File,
        };

        let stow_item = process_raw_item(&raw_item, "test_package", &stow_dir, false);
        
        assert_eq!(stow_item.package_relative_path, PathBuf::from("bin/test_script"));
        assert_eq!(stow_item.source_path, package_path.join("bin").join("test_script"));
        assert_eq!(stow_item.item_type, StowItemType::File);
        assert_eq!(stow_item.target_name_after_dotfiles_processing, "test_script");
    }

    #[test]
    fn test_process_raw_item_with_dotfiles() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let package_path = stow_dir.join("test_package");
        
        let raw_item = RawStowItem {
            absolute_path: package_path.join("dot-bashrc"),
            package_relative_path: PathBuf::from("dot-bashrc"),
            item_type: RawStowItemType::File,
        };

        let stow_item = process_raw_item(&raw_item, "test_package", &stow_dir, true);
        
        assert_eq!(stow_item.package_relative_path, PathBuf::from("dot-bashrc"));
        assert_eq!(stow_item.source_path, package_path.join("dot-bashrc"));
        assert_eq!(stow_item.item_type, StowItemType::File);
        assert_eq!(stow_item.target_name_after_dotfiles_processing, ".bashrc");
    }

    #[test]
    fn test_target_action_creation() {
        let temp_dir = TempDir::new().unwrap();
        let stow_dir = temp_dir.path().join("stow");
        let target_dir = temp_dir.path().join("target");
        
        let stow_item = StowItem {
            package_relative_path: PathBuf::from("bin/test_script"),
            source_path: stow_dir.join("test_package").join("bin").join("test_script"),
            item_type: StowItemType::File,
            target_name_after_dotfiles_processing: "test_script".to_string(),
        };

        let action = TargetAction {
            source_item: Some(stow_item),
            target_path: target_dir.join("bin").join("test_script"),
            link_target_path: Some(PathBuf::from("../stow/test_package/bin/test_script")),
            action_type: ActionType::CreateSymlink,
            conflict_details: None,
        };

        assert_eq!(action.action_type, ActionType::CreateSymlink);
        assert!(action.source_item.is_some());
        assert!(action.conflict_details.is_none());
    }
} 
