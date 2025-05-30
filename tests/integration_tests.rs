use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{tempdir, TempDir};

use rustow::cli::Args;
use rustow::config::Config;
use rustow::stow::stow_packages; // Assuming stow_packages is the main entry point
use rustow::stow::ActionType;
use std::ffi::OsStr; // Import OsStr
use rustow::config::StowMode; // Add this import
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

// Modified to accept verbosity
fn create_test_config(stow_dir: PathBuf, target_dir: PathBuf, packages: Vec<String>, dotfiles: bool, verbosity: u8) -> Config {
    Config {
        stow_dir,
        target_dir,
        packages,
        mode: StowMode::Stow, // Default to Stow mode for these tests
        adopt: false,
        no_folding: false,
        dotfiles,
        overrides: Vec::new(),
        defers: Vec::new(),
        simulate: false,
        verbosity, // Use the passed verbosity
        home_dir: std::env::temp_dir(), // Dummy home dir for tests, not critical for these path tests
    }
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
        0
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
        0
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
        2
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

    // let has_log = actions.iter().any(|a| a.target_path.ends_with("file.log"));
    // assert!(!has_log, "*.log files (file.log) should be ignored by default patterns - this might be an incorrect assumption for default Stow behavior");

    let has_backup = actions.iter().any(|a| a.target_path.ends_with("backup~"));
    assert!(!has_backup, "backup~ files should be ignored by default pattern '.*~'");

    let has_git = actions.iter().any(|a| a.target_path.to_string_lossy().contains(".git"));
    assert!(!has_git, ".git directory and its contents should be ignored by default pattern '\\.git'");
}

#[test]
fn test_custom_ignore_patterns() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "test_custom_ignore";
    let package_dir = create_test_package(&stow_dir, package_name);

    // Create a custom ignore file in the package
    // Patterns should match names *after* dotfiles processing if dotfiles option is enabled.
    let ignore_file_content = "bin/test_script\n.bashrc\n# This is a comment\n.*\\.md"; // Changed "dot-bashrc" to ".bashrc"
    fs::write(package_dir.join(".stow-local-ignore"), ignore_file_content)
        .expect("Failed to create .stow-local-ignore file");

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
        0
    );

    let actions_result = stow_packages(&config);
    // Add debug printing for actions if the test fails
    if actions_result.is_err() || 
       (actions_result.is_ok() && (
           actions_result.as_ref().unwrap().iter().any(|a| a.target_path.ends_with("test_script")) || 
           actions_result.as_ref().unwrap().iter().any(|a| a.target_path.ends_with(".bashrc")) || 
           actions_result.as_ref().unwrap().iter().any(|a| a.target_path.ends_with("README.md")) || 
           !actions_result.as_ref().unwrap().iter().any(|a| a.target_path.ends_with(".config/nvim/init.vim"))
       ))
    {
        eprintln!("--- DEBUG: test_custom_ignore_patterns --- ACTIONS (on potential failure) ---");
        if let Ok(actions) = &actions_result {
            for action in actions {
                 if let Some(item) = &action.source_item {
                    eprintln!(
                        "  Action: Target: {:?}, SourceItem.rel: {:?}, SourceItem.processed_name: {:?}, LinkTarget: {:?}",
                        action.target_path,
                        item.package_relative_path, // original name in package
                        item.target_name_after_dotfiles_processing, // name after dot- conversion
                        action.link_target_path
                    );
                } else {
                    eprintln!("  Action (no source_item): Target: {:?}, LinkTarget: {:?}", action.target_path, action.link_target_path);
                }
            }
        } else if let Err(e) = &actions_result {
            eprintln!("  Error: {:?}", e);
        }
        eprintln!("--- END DEBUG --- ACTIONS ---");
    }

    assert!(actions_result.is_ok(), "stow_packages failed for custom ignore test: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    // Verify that custom ignored files are not included
    let has_test_script = actions.iter().any(|a| a.target_path.ends_with("test_script"));
    assert!(!has_test_script, "test_script (bin/test_script) should be ignored by custom pattern 'bin/test_script'");

    let has_bashrc = actions.iter().any(|a| a.target_path.ends_with(".bashrc"));
    assert!(!has_bashrc, ".bashrc (from dot-bashrc) should be ignored by custom pattern '.bashrc'");

    let has_readme = actions.iter().any(|a| a.target_path.ends_with("README.md"));
    assert!(!has_readme, "README.md should be ignored by custom pattern '.*\\.md'");

    let has_nvim_init = actions.iter().any(|a| a.target_path.ends_with(".config/nvim/init.vim"));
    assert!(has_nvim_init, ".config/nvim/init.vim (from dot-config/nvim/init.vim) should NOT be ignored");
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
        0
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
        0
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
        0
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

    // Test case 1: file named "dot-file" in package, should become ".file" in target
    let package_dir_1 = stow_dir.join("package1");
    fs::create_dir_all(&package_dir_1).unwrap();
    fs::write(package_dir_1.join("dot-file"), "content for dot-file").unwrap();

    // Test case 2: file starting with "dot-"
    let package_dir_2 = stow_dir.join("package2");
    fs::create_dir_all(&package_dir_2).unwrap();
    fs::write(package_dir_2.join("dot-foo-bar"), "content").unwrap();

    // Test case 3: directory starting with "dot-", containing a file
    let package_dir_3 = stow_dir.join("package3");
    fs::create_dir_all(&package_dir_3).unwrap();
    let nested_dir_3 = package_dir_3.join("dot-dirOnly");
    fs::create_dir_all(&nested_dir_3).unwrap();
    fs::write(nested_dir_3.join("some_file.txt"), "content").unwrap();

    // Test case 4: file NOT starting with "dot-"
    let package_dir_4 = stow_dir.join("package4");
    fs::create_dir_all(&package_dir_4).unwrap();
    fs::write(package_dir_4.join("nodotprefix"), "content").unwrap();

    // Test case 5: directory NOT starting with "dot-", containing a file
    let package_dir_5 = stow_dir.join("package5");
    fs::create_dir_all(&package_dir_5).unwrap();
    let nested_dir_5 = package_dir_5.join("nodotprefix");
    fs::create_dir_all(&nested_dir_5).unwrap();
    fs::write(nested_dir_5.join("file.txt"), "content").unwrap();

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![
            "package1".to_string(),
            "package2".to_string(),
            "package3".to_string(),
            "package4".to_string(),
            "package5".to_string(),
        ],
        true, // enable dotfiles processing
        4     // Set verbosity to 4 for debug logs
    );

    let actions_result = stow_packages(&config);
    assert!(actions_result.is_ok(), "stow_packages failed for dotfiles edge cases: {:?}", actions_result.err());
    let actions = actions_result.unwrap();

    // Verify package1: "dot-file" -> ".file"
    let has_dot_file_processed = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.package_relative_path == Path::new("dot-file") &&
            item.target_name_after_dotfiles_processing == Path::new(".file")
        } else {
            false
        }
    });
    assert!(has_dot_file_processed, "Expected target name '.file' for package1/dot-file");

    // Verify package2: "dot-foo-bar" -> ".foo-bar"
    let has_dot_foo_bar = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.target_name_after_dotfiles_processing == Path::new(".foo-bar")
        } else {
            false
        }
    });
    assert!(has_dot_foo_bar, "Expected target name '.foo-bar' for package2/dot-foo-bar");

    // Verify package3: "dot-dirOnly" -> ".dirOnly"
    let has_dot_dir_only = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.package_relative_path == Path::new("dot-dirOnly") &&
            item.target_name_after_dotfiles_processing == Path::new(".dirOnly")
        } else {
            false
        }
    });
    assert!(has_dot_dir_only, "Expected target name '.dirOnly' for package3/dot-dirOnly");
    
    // Verify package3: "dot-dirOnly/some_file.txt" -> ".dirOnly/some_file.txt"
    let has_dot_dir_only_file = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.package_relative_path == Path::new("dot-dirOnly/some_file.txt") &&
            item.target_name_after_dotfiles_processing == Path::new(".dirOnly/some_file.txt")
        } else {
            false
        }
    });
    assert!(has_dot_dir_only_file, "Expected target name '.dirOnly/some_file.txt' for package3/dot-dirOnly/some_file.txt");

    // Verify package4: "nodotprefix" -> "nodotprefix"
    let has_nodotprefix_file = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            // Assuming 'nodotprefix' is a top-level entry in the package
            item.package_relative_path == std::path::PathBuf::from("nodotprefix") && 
            item.target_name_after_dotfiles_processing == Path::new("nodotprefix")
        } else {
            false
        }
    });
    assert!(has_nodotprefix_file, "Expected target name 'nodotprefix' for package4/nodotprefix");

    // Verify package5: "nodotprefix/file.txt" -> "nodotprefix/file.txt"
    let has_nodotprefix_nested_file = actions.iter().any(|a| {
        if let Some(item) = &a.source_item {
            item.package_relative_path == std::path::PathBuf::from("nodotprefix/file.txt") &&
            item.target_name_after_dotfiles_processing == Path::new("nodotprefix/file.txt")
        } else {
            false
        }
    });
    assert!(has_nodotprefix_nested_file, "Expected target name 'nodotprefix/file.txt' for package5/nodotprefix/file.txt");

    // Print details for debugging if needed
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
        0
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

#[test]
fn test_plan_actions_basic_creation_and_conflict() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "plan_test_pkg";
    let package_dir = stow_dir.join(package_name);
    fs::create_dir_all(&package_dir).unwrap();

    fs::write(package_dir.join("file_to_link.txt"), "link me").unwrap();
    let dir_to_create_in_pkg = package_dir.join("dir_to_create");
    fs::create_dir_all(&dir_to_create_in_pkg).unwrap();
    fs::write(dir_to_create_in_pkg.join("nested_file.txt"), "i am nested").unwrap();
    fs::write(package_dir.join("file_for_conflict.txt"), "conflict file content").unwrap();
    let dir_for_conflict_in_pkg = package_dir.join("dir_for_conflict");
    fs::create_dir_all(&dir_for_conflict_in_pkg).unwrap();
    fs::write(dir_for_conflict_in_pkg.join("another_nested.txt"), "nested conflict").unwrap();

    let config_empty_target = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0
    );

    let actions_empty_result = stow_packages(&config_empty_target);
    assert!(actions_empty_result.is_ok(), "stow_packages failed for empty target: {:?}", actions_empty_result.err());
    let actions_empty = actions_empty_result.unwrap();

    let action_file_to_link = actions_empty.iter().find(|a| a.target_path.file_name().map_or(false, |name| name == OsStr::new("file_to_link.txt")));
    assert!(action_file_to_link.is_some(), "Action for file_to_link.txt not found");
    assert_eq!(action_file_to_link.unwrap().action_type, ActionType::CreateSymlink, "Expected CreateSymlink for file_to_link.txt");
    assert!(action_file_to_link.unwrap().link_target_path.is_some(), "Link target path should exist for CreateSymlink");

    let action_dir_to_create = actions_empty.iter().find(|a| a.target_path.file_name().map_or(false, |name| name == OsStr::new("dir_to_create")));
    assert!(action_dir_to_create.is_some(), "Action for dir_to_create not found");
    assert_eq!(action_dir_to_create.unwrap().action_type, ActionType::CreateDirectory, "Expected CreateDirectory for dir_to_create");
    assert!(action_dir_to_create.unwrap().link_target_path.is_none(), "Link target path should be None for CreateDirectory");

    let action_nested_file = actions_empty.iter().find(|a| a.target_path.ends_with(Path::new("dir_to_create/nested_file.txt")));
    assert!(action_nested_file.is_some(), "Action for dir_to_create/nested_file.txt not found");
    assert_eq!(action_nested_file.unwrap().action_type, ActionType::CreateSymlink, "Expected CreateSymlink for nested_file.txt");

    let target_file_conflict_path = target_dir.join("file_for_conflict.txt");
    fs::write(&target_file_conflict_path, "existing target file content").unwrap();

    let config_file_conflict = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0
    );
    let actions_file_conflict_result = stow_packages(&config_file_conflict);
    assert!(actions_file_conflict_result.is_ok(), "stow_packages failed for file conflict: {:?}", actions_file_conflict_result.err());
    let actions_file_conflict = actions_file_conflict_result.unwrap();

    let action_conflicting_file = actions_file_conflict.iter().find(|a| a.target_path.file_name().map_or(false, |name| name == OsStr::new("file_for_conflict.txt")));
    assert!(action_conflicting_file.is_some(), "Action for file_for_conflict.txt not found in conflict scenario");
    assert_eq!(action_conflicting_file.unwrap().action_type, ActionType::Conflict, "Expected Conflict for file_for_conflict.txt");
    assert!(action_conflicting_file.unwrap().conflict_details.is_some(), "Conflict details should be present");
    assert!(action_conflicting_file.unwrap().link_target_path.is_none(), "Link target should be None for Conflict");

    fs::remove_file(target_file_conflict_path).unwrap();

    let target_dir_conflict_path = target_dir.join("dir_for_conflict");
    fs::create_dir_all(&target_dir_conflict_path).unwrap();
    fs::write(target_dir_conflict_path.join("existing_file_in_target_dir.txt"), "dummy").unwrap();

    let config_dir_conflict = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0
    );
    let actions_dir_conflict_result = stow_packages(&config_dir_conflict);
    assert!(actions_dir_conflict_result.is_ok(), "stow_packages failed for dir conflict: {:?}", actions_dir_conflict_result.err());
    let actions_dir_conflict = actions_dir_conflict_result.unwrap();
    
    let action_conflicting_dir = actions_dir_conflict.iter().find(|a| a.target_path.file_name().map_or(false, |name| name == OsStr::new("dir_for_conflict")));
    assert!(action_conflicting_dir.is_some(), "Action for dir_for_conflict not found in conflict scenario");
    assert_eq!(action_conflicting_dir.unwrap().action_type, ActionType::Conflict, "Expected Conflict for dir_for_conflict");
    assert!(action_conflicting_dir.unwrap().conflict_details.is_some(), "Conflict details should be present for dir conflict");
    assert!(action_conflicting_dir.unwrap().link_target_path.is_none(), "Link target should be None for dir Conflict");

    let action_nested_in_conflicting_dir = actions_dir_conflict.iter().find(|a| a.target_path.ends_with(Path::new("dir_for_conflict/another_nested.txt")));
    assert!(action_nested_in_conflicting_dir.is_some(), "Action for dir_for_conflict/another_nested.txt not found");
    assert_eq!(action_nested_in_conflicting_dir.unwrap().action_type, ActionType::Conflict, "Expected Conflict for item in conflicting dir");

    fs::remove_dir_all(target_dir_conflict_path).unwrap();
}

// Add more tests as needed: 
// - Conflicting files/directories (needs fs_utils to check existence in target for planning)
// - `--adopt` functionality (needs more involved setup and fs_utils checks)
// - `--no-folding` (needs directory structures that would normally fold)
// - Delete and Restow operations (would need to plan Delete actions or sequence of Delete/Create) 
