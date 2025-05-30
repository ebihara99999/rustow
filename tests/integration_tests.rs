use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{tempdir, TempDir};

use rustow::cli::Args;
use rustow::config::{Config, StowMode};
use rustow::stow::stow_packages; // Assuming stow_packages is the main entry point
// Add other necessary imports from your crate, e.g., for error types or specific structs

// Helper function to set up a test environment with stow and target directories
fn setup_test_environment() -> (TempDir, PathBuf, PathBuf) {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let stow_dir = temp_dir.path().join("stow_dir");
    let target_dir = temp_dir.path().join("target_dir");
    fs::create_dir_all(&stow_dir).expect("Failed to create stow dir");
    fs::create_dir_all(&target_dir).expect("Failed to create target dir");
    (temp_dir, stow_dir, target_dir)
}

// Helper function to create a sample package within the stow directory
fn create_test_package(stow_dir: &Path, package_name: &str) -> PathBuf {
    let package_dir = stow_dir.join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create package dir");

    // Create some files and directories within the package
    let bin_dir = package_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("Failed to create bin dir in package");
    let mut script_file = File::create(bin_dir.join("test_script"))
        .expect("Failed to create test_script in package");
    writeln!(script_file, "#!/bin/bash\necho Hello").expect("Failed to write to test_script");

    // Dotfiles
    fs::write(package_dir.join("dot-bashrc"), "# Test bashrc content")
        .expect("Failed to create dot-bashrc in package");
    let dot_config_dir = package_dir.join("dot-config");
    fs::create_dir_all(&dot_config_dir).expect("Failed to create dot-config dir in package");
    let nvim_dir = dot_config_dir.join("nvim");
    fs::create_dir_all(&nvim_dir).expect("Failed to create nvim dir in package");
    fs::write(nvim_dir.join("init.vim"), "Test nvim config")
        .expect("Failed to create init.vim in package");

    // Files that should be ignored by default patterns
    fs::write(package_dir.join("README.md"), "# Test Package README")
        .expect("Failed to create README.md in package");
    fs::write(package_dir.join("LICENSE"), "MIT License content")
        .expect("Failed to create LICENSE in package");

    package_dir
}

// Helper function to create a basic Config for tests
fn create_test_config(stow_dir: PathBuf, target_dir: PathBuf, packages: Vec<String>, dotfiles: bool) -> Config {
    // We need to create a minimal Args struct to build the Config
    // Assuming your Config::from_args can handle this minimal set up
    let args = Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        delete: false,
        restow: false,
        adopt: false,
        no_folding: false,
        dotfiles,
        override_conflicts: vec![],
        defer_conflicts: vec![],
        simulate: true, // Important: use simulate true for tests not to change FS beyond temp_dir
        verbose: 0,
        packages,
    };

    Config::from_args(args).expect("Failed to create Config from Args for test")
}


#[test]
fn test_basic_stow_operation_without_dotfiles() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "test_package";
    create_test_package(&stow_dir, package_name);

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false, // dotfiles disabled
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    assert!(!actions.is_empty(), "Expected some actions to be planned");

    // Verify that dot-bashrc is NOT processed as .bashrc
    let dot_bashrc_action_exists = actions.iter().any(|action| 
        action.target_path.file_name().map_or(false, |name| name == "dot-bashrc")
    );
    assert!(dot_bashrc_action_exists, "Expected \"dot-bashrc\" action when dotfiles disabled");

    // Verify that dot-config is NOT processed as .config
    let dot_config_action_exists = actions.iter().any(|action| 
        action.target_path.file_name().map_or(false, |name| name == "dot-config")
    );
    assert!(dot_config_action_exists, "Expected \"dot-config\" action when dotfiles disabled");

    // Verify README.md and LICENSE are ignored (not present in actions)
    let readme_action_exists = actions.iter().any(|action| 
        action.target_path.file_name().map_or(false, |name| name == "README.md")
    );
    assert!(!readme_action_exists, "README.md should be ignored by default");

    let license_action_exists = actions.iter().any(|action| 
        action.target_path.file_name().map_or(false, |name| name == "LICENSE")
    );
    assert!(!license_action_exists, "LICENSE should be ignored by default");
}

#[test]
fn test_basic_stow_operation_with_dotfiles() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "test_package_dots";
    create_test_package(&stow_dir, package_name);

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed with dotfiles: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    assert!(!actions.is_empty(), "Expected some actions with dotfiles enabled");

    // Verify dot-bashrc IS processed as .bashrc
    let bashrc_action_exists = actions.iter().any(|action| 
        action.target_path.file_name().map_or(false, |name| name == ".bashrc")
    );
    assert!(bashrc_action_exists, "Expected \".bashrc\" action when dotfiles enabled");

    // Verify dot-config IS processed as .config
    let config_action_exists = actions.iter().any(|action| 
        action.target_path.file_name().map_or(false, |name| name == ".config")
    );
    assert!(config_action_exists, "Expected \".config\" action when dotfiles enabled");

    // Verify nested dotfiles like .config/nvim/init.vim are correctly planned
    let nvim_init_action_exists = actions.iter().any(|action| 
        action.target_path.ends_with(".config/nvim/init.vim")
    );
    assert!(nvim_init_action_exists, "Expected \".config/nvim/init.vim\" action");
}

#[test]
fn test_ignore_patterns_functionality() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "test_ignore_pkg";
    let package_dir = create_test_package(&stow_dir, package_name);

    // Create additional files that should be ignored by default patterns
    fs::write(package_dir.join("file.log"), "log content")
        .expect("Failed to create log file for ignore test");
    fs::write(package_dir.join("backup~"), "backup content")
        .expect("Failed to create backup file for ignore test");
    let git_dir = package_dir.join(".git");
    fs::create_dir_all(&git_dir).expect("Failed to create .git dir for ignore test");
    fs::write(git_dir.join("config"), "git config content")
        .expect("Failed to create git config file for ignore test");

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false, // dotfiles typically don't affect these ignore patterns
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed for ignore test: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    // Debug print all actions if tests fail
    // for action in &actions {
    //     println!("Action Target: {:?}", action.target_path);
    // }

    let has_readme = actions.iter().any(|a| a.target_path.ends_with("README.md"));
    assert!(!has_readme, "README.md should be ignored");

    let has_license = actions.iter().any(|a| a.target_path.ends_with("LICENSE"));
    assert!(!has_license, "LICENSE should be ignored");

    let has_log = actions.iter().any(|a| a.target_path.ends_with("file.log"));
    assert!(!has_log, "*.log files (file.log) should be ignored");

    let has_backup = actions.iter().any(|a| a.target_path.ends_with("backup~"));
    assert!(!has_backup, "backup~ files should be ignored");

    let has_git = actions.iter().any(|a| a.target_path.to_string_lossy().contains(".git"));
    assert!(!has_git, ".git directory and its contents should be ignored");
}

#[test]
fn test_custom_ignore_patterns() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "test_custom_ignore";
    let package_dir = create_test_package(&stow_dir, package_name);

    // Create a custom ignore file in the package
    let ignore_file_content = "bin/test_script\ndot-bashrc\n# This is a comment\n*.md";
    fs::write(package_dir.join(".stow-local-ignore"), ignore_file_content)
        .expect("Failed to create .stow-local-ignore file");

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // Enable dotfiles to test interaction with custom ignores on dot-prefixed items
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed for custom ignore test: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    // Verify that custom ignored files are not included
    let has_test_script = actions.iter().any(|a| a.target_path.ends_with("test_script"));
    assert!(!has_test_script, "test_script should be ignored by custom pattern");

    // dot-bashrc is in ignore file, so .bashrc (after dotfile processing) should be ignored
    let has_bashrc = actions.iter().any(|a| a.target_path.ends_with(".bashrc"));
    assert!(!has_bashrc, ".bashrc (from dot-bashrc) should be ignored by custom pattern");

    // README.md was created, and *.md is in custom ignore
    let has_readme = actions.iter().any(|a| a.target_path.ends_with("README.md"));
    assert!(!has_readme, "README.md should be ignored by custom pattern \"*.md\"");

    // .config/nvim/init.vim was not in ignore file, should still be processed
    let has_nvim_init = actions.iter().any(|a| a.target_path.ends_with(".config/nvim/init.vim"));
    assert!(has_nvim_init, ".config/nvim/init.vim should NOT be ignored");
}

#[test]
fn test_multiple_packages_stow() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package1_name = "package1";
    let package2_name = "package2";
    create_test_package(&stow_dir, package1_name);
    create_test_package(&stow_dir, package2_name);

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package1_name.to_string(), package2_name.to_string()],
        true, // dotfiles enabled for thoroughness
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed for multiple packages: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    // Check for items from both packages (e.g., their respective .bashrc files)
    let p1_bashrc_exists = actions.iter().any(|action| 
        action.source_item.as_ref().map_or(false, |item| item.source_path.to_string_lossy().contains(package1_name)) &&
        action.target_path.ends_with(".bashrc")
    );
    assert!(p1_bashrc_exists, "Expected .bashrc from package1");

    let p2_bashrc_exists = actions.iter().any(|action| 
        action.source_item.as_ref().map_or(false, |item| item.source_path.to_string_lossy().contains(package2_name)) &&
        action.target_path.ends_with(".bashrc")
    );
    assert!(p2_bashrc_exists, "Expected .bashrc from package2");
}

#[test]
fn test_empty_package_list() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![], // Empty package list
        false,
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed for empty package list: {:?}", actions_result.err());
    let actions = actions_result.unwrap();
    assert!(actions.is_empty(), "Expected no actions for an empty package list");
}

#[test]
fn test_nonexistent_package() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "nonexistent_package";

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
    );

    let actions_result = stow_packages(&config);
    // Expect an error because the package directory won't be found by scan_package
    assert!(actions_result.is_err(), "Expected stow_packages to fail for a nonexistent package");
    // Optionally, check for the specific error type if your error enum allows
    // e.g., assert!(matches!(actions_result.err().unwrap(), RustowError::Stow(StowError::PackageNotFound(_))));
}

#[test]
fn test_dotfiles_processing_edge_cases() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "edge_case_dots";
    let package_dir = stow_dir.join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create edge case package dir");

    // Files/dirs to test
    fs::write(package_dir.join("dot-"), "content").expect("Failed to write 'dot-' file"); // dot- only
    fs::write(package_dir.join("dot-foo-bar"), "content").expect("Failed to write 'dot-foo-bar' file");
    fs::create_dir_all(package_dir.join("dot-dirOnly")).expect("Failed to create 'dot-dirOnly'");
    fs::create_dir_all(package_dir.join("nodotprefix")).expect("Failed to create 'nodotprefix'");
    fs::write(package_dir.join("nodotprefix/file.txt"), "content").expect("Failed to write file in nodotprefix");

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed for dotfiles edge cases: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    // ---- START DEBUG PRINT (Original) ----
    println!("--- DEBUG: test_dotfiles_processing_edge_cases --- ACTIONS ---");
    for action in &actions {
        if let Some(item) = &action.source_item {
            println!(
                "  Source: {:?}, Processed Name: {:?}, Target Path: {:?}",
                item.package_relative_path,
                item.target_name_after_dotfiles_processing,
                action.target_path
            );
        } else {
            println!("  Action with no source item: Target Path: {:?}", action.target_path);
        }
    }
    println!("--- END DEBUG --- ACTIONS ---");
    // ---- END DEBUG PRINT (Original) ----

    // ---- START NEW DETAILED DEBUG PRINT ----
    use std::ffi::OsStr;
    println!("--- DEBUG: file_name() results ---");
    for action in &actions {
        if let Some(file_name_os_str) = action.target_path.file_name() {
            let file_name_str = file_name_os_str.to_string_lossy();
            println!(
                "  Target: {:?}, FileName: {:?}, Is it '.': {}",
                action.target_path,
                file_name_str,
                file_name_os_str == OsStr::new(".")
            );
        } else {
            println!("  Target: {:?}, FileName: None", action.target_path);
        }
    }
    println!("--- END DEBUG: file_name() results ---");
    // ---- END NEW DETAILED DEBUG PRINT ----

    let has_dot_empty = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.target_name_after_dotfiles_processing == "."
        } else {
            false
        }
    });
    assert!(has_dot_empty, "Expected 'dot-' to become '.' (checked via target_name_after_dotfiles_processing)");

    let has_dot_foo_bar = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.target_name_after_dotfiles_processing == ".foo-bar"
        } else {
            false
        }
    });
    assert!(has_dot_foo_bar, "Expected 'dot-foo-bar' to become '.foo-bar' (checked via target_name_after_dotfiles_processing)");

    let has_dot_dir_only = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.target_name_after_dotfiles_processing == ".dirOnly"
        } else {
            false
        }
    });
    assert!(has_dot_dir_only, "Expected 'dot-dirOnly' to become '.dirOnly' (checked via target_name_after_dotfiles_processing)");

    // For items not starting with 'dot-', their target_name_after_dotfiles_processing should be the same as their original relative path's file_name (if it's a file)
    // or the last component of the path (if it's a directory).
    // This assertion needs to be more careful if package_relative_path contains directories.

    // Check 'nodotprefix' directory - its target_name_after_dotfiles_processing should be 'nodotprefix'
    let has_nodotprefix_dir = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            // Assuming 'nodotprefix' is a top-level entry in the package
            item.package_relative_path == std::path::PathBuf::from("nodotprefix") && 
            item.target_name_after_dotfiles_processing == "nodotprefix"
        } else {
            false
        }
    });
    assert!(has_nodotprefix_dir, "Expected 'nodotprefix' directory to remain unchanged and have correct processed name");
    
    // Check 'nodotprefix/file.txt' - its target_name_after_dotfiles_processing should be 'nodotprefix/file.txt'
    let has_nodotprefix_file = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.package_relative_path == std::path::PathBuf::from("nodotprefix/file.txt") &&
            item.target_name_after_dotfiles_processing == "nodotprefix/file.txt"
        } else {
            false
        }
    });
    assert!(has_nodotprefix_file, "Expected 'nodotprefix/file.txt' to remain unchanged and have correct processed name");
}

// Note: True relative path calculation for symlinks is complex and depends on the target OS's symlink behavior.
// These tests will primarily check if the `link_target_path` in `TargetAction` appears plausible (e.g., relative).
// Actual symlink creation and resolution would be tested in end-to-end execution tests (not just planning).
#[test]
fn test_relative_path_calculation_basic() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "relpath_pkg";
    create_test_package(&stow_dir, package_name);

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false, // dotfiles status doesn't fundamentally change relativity expectation
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed for relative path test: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    for action in actions.iter().filter(|a| a.link_target_path.is_some()) {
        let link_target = action.link_target_path.as_ref().unwrap();
        // A simple check: relative paths should not start with '/' or a drive letter (on Windows)
        assert!(!link_target.is_absolute(), 
            "Link target path {:?} for target {:?} should be relative", 
            link_target, action.target_path);
        // More robust check: ensure it navigates upwards (e.g., starts with "..")
        // This depends on the depth. For a top-level file like 'dot-bashrc' -> '../stow_dir/pkg/dot-bashrc'
        // For 'bin/test_script' -> '../../stow_dir/pkg/bin/test_script'
        // This is a basic sanity check.
        assert!(link_target.starts_with(".."), 
            "Link target path {:?} for target {:?} should typically start with '..' to go from target to stow dir item", 
            link_target, action.target_path);
    }
}

#[test]
fn test_config_integration_verbosity_and_simulate() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "config_test_pkg";
    create_test_package(&stow_dir, package_name);

    let args = Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        packages: vec![package_name.to_string()],
        simulate: true,
        verbose: 3,
        delete: false, restow: false, adopt: false, no_folding: false, dotfiles: false, 
        override_conflicts: vec![], defer_conflicts: vec![],
    };

    let config_result = Config::from_args(args);
    assert!(config_result.is_ok(), "Config creation failed: {:?}", config_result.err());
    let config = config_result.unwrap();

    assert!(config.simulate, "Config.simulate should be true");
    assert_eq!(config.verbosity, 3, "Config.verbosity should be 3");

    // stow_packages should still work and plan actions even in simulate mode
    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed with simulate config: {:?}", actions_result.err());
    assert!(!actions_result.unwrap().is_empty(), "Expected actions even in simulate mode");
}

// Add more tests as needed: 
// - Conflicting files/directories (needs fs_utils to check existence in target for planning)
// - `--adopt` functionality (needs more involved setup and fs_utils checks)
// - `--no-folding` (needs directory structures that would normally fold)
// - Delete and Restow operations (would need to plan Delete actions or sequence of Delete/Create) 
