use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use rustow::cli::Args;
use rustow::config::{Config, StowMode};
use rustow::stow::{
    ActionType, StowItemType, TargetActionReportStatus, delete_packages, restow_packages,
    stow_packages,
};
use tempfile::{TempDir, tempdir};

lazy_static::lazy_static! {
// ... existing code ...
}

// Helper function to set up a test environment with stow and target directories
fn setup_test_environment() -> (TempDir, PathBuf, PathBuf) {
    let temp_dir: TempDir = tempdir().expect("Failed to create temp dir");
    let stow_dir: PathBuf = temp_dir.path().join("stow_dir");
    let target_dir: PathBuf = temp_dir.path().join("target_dir");
    fs::create_dir_all(&stow_dir).expect("Failed to create stow dir");
    fs::create_dir_all(&target_dir).expect("Failed to create target dir");
    (temp_dir, stow_dir, target_dir)
}

// Helper function to create a sample package within the stow directory
fn create_test_package(stow_dir: &Path, package_name: &str) -> PathBuf {
    let package_dir: PathBuf = stow_dir.join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create package dir");

    // Create some files and directories within the package
    let bin_dir: PathBuf = package_dir.join("bin");
    fs::create_dir_all(&bin_dir).expect("Failed to create bin dir in package");
    let mut script_file: File =
        File::create(bin_dir.join("test_script")).expect("Failed to create test_script in package");
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
fn create_test_config(
    stow_dir: PathBuf,
    target_dir: PathBuf,
    packages: Vec<String>,
    dotfiles: bool,
    verbosity: u8,
) -> Config {
    Config {
        stow_dir,
        target_dir,
        packages,
        mode: StowMode::Stow, // Default to Stow mode for these tests
        stow: false,
        adopt: false,
        no_folding: false,
        dotfiles,
        overrides: Vec::new(),
        defers: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbosity,                      // Use the passed verbosity
        home_dir: std::env::temp_dir(), // Dummy home dir for tests, not critical for these path tests
    }
}

#[test]
fn test_basic_stow_operation_without_dotfiles() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_package";
    create_test_package(&stow_dir, package_name);

    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false, // dotfiles disabled
        0,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();

    assert!(!actions.is_empty(), "Expected some actions to be planned");

    // Verify that dot-bashrc is NOT processed as .bashrc
    let dot_bashrc_action_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .target_path
            .file_name()
            .map_or(false, |name| name == "dot-bashrc")
    });
    assert!(
        dot_bashrc_action_exists,
        "Expected \"dot-bashrc\" action when dotfiles disabled"
    );

    // Verify that dot-config is NOT processed as .config
    let dot_config_action_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .target_path
            .file_name()
            .map_or(false, |name| name == "dot-config")
    });
    assert!(
        dot_config_action_exists,
        "Expected \"dot-config\" action when dotfiles disabled"
    );

    // Verify README.md and LICENSE are ignored (not present in actions)
    let readme_action_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .target_path
            .file_name()
            .map_or(false, |name| name == "README.md")
    });
    assert!(
        !readme_action_exists,
        "README.md should be ignored by default"
    );

    let license_action_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .target_path
            .file_name()
            .map_or(false, |name| name == "LICENSE")
    });
    assert!(
        !license_action_exists,
        "LICENSE should be ignored by default"
    );
}

#[test]
fn test_basic_stow_operation_with_dotfiles() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_package_dots";
    create_test_package(&stow_dir, package_name);

    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
        0,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed with dotfiles: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();

    assert!(
        !actions.is_empty(),
        "Expected some actions with dotfiles enabled"
    );

    // Verify dot-bashrc IS processed as .bashrc
    let bashrc_action_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .target_path
            .file_name()
            .map_or(false, |name| name == ".bashrc")
    });
    assert!(
        bashrc_action_exists,
        "Expected \".bashrc\" action when dotfiles enabled"
    );

    // Verify dot-config IS processed as .config
    let config_action_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .target_path
            .file_name()
            .map_or(false, |name| name == ".config")
    });
    assert!(
        config_action_exists,
        "Expected \".config\" action when dotfiles enabled"
    );

    // Verify nested dotfiles like .config/nvim/init.vim are correctly planned
    let nvim_init_action_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .target_path
            .ends_with(".config/nvim/init.vim")
    });
    assert!(
        nvim_init_action_exists,
        "Expected \".config/nvim/init.vim\" action"
    );
}

#[test]
fn test_ignore_patterns_functionality() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_ignore_pkg";
    let package_dir: PathBuf = create_test_package(&stow_dir, package_name);

    // Create additional files that should be ignored by default patterns
    fs::write(package_dir.join("file.log"), "log content")
        .expect("Failed to create log file for ignore test");
    fs::write(package_dir.join("backup~"), "backup content")
        .expect("Failed to create backup file for ignore test");
    let git_dir: PathBuf = package_dir.join(".git");
    fs::create_dir_all(&git_dir).expect("Failed to create .git dir for ignore test");
    fs::write(git_dir.join("config"), "git config content")
        .expect("Failed to create git config file for ignore test");

    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false, // dotfiles typically don't affect these ignore patterns
        2,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed for ignore test: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();

    // Debug print all actions if tests fail
    // for action in &actions {
    //     println!("Action Target: {:?}", action.target_path);
    // }

    let has_readme: bool = actions
        .iter()
        .any(|r| r.original_action.target_path.ends_with("README.md"));
    assert!(!has_readme, "README.md should be ignored");

    let has_license: bool = actions
        .iter()
        .any(|r| r.original_action.target_path.ends_with("LICENSE"));
    assert!(!has_license, "LICENSE should be ignored by default");

    // let has_log = actions.iter().any(|a| a.target_path.ends_with("file.log"));
    // assert!(!has_log, "*.log files (file.log) should be ignored by default patterns - this might be an incorrect assumption for default Stow behavior");

    let has_backup: bool = actions
        .iter()
        .any(|r| r.original_action.target_path.ends_with("backup~"));
    assert!(
        !has_backup,
        "backup~ files should be ignored by default pattern '.*~'"
    );

    let has_git: bool = actions.iter().any(|r| {
        r.original_action
            .target_path
            .to_string_lossy()
            .contains(".git")
    });
    assert!(
        !has_git,
        ".git directory and its contents should be ignored by default pattern '\\.git'"
    );
}

#[test]
fn test_custom_ignore_patterns() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_custom_ignore";
    let package_dir: PathBuf = create_test_package(&stow_dir, package_name);

    // Create a custom ignore file in the package
    // Patterns should match names *after* dotfiles processing if dotfiles option is enabled.
    let ignore_file_content: &str = "bin/test_script\n.bashrc\n# This is a comment\n.*\\.md"; // Changed "dot-bashrc" to ".bashrc"
    fs::write(package_dir.join(".stow-local-ignore"), ignore_file_content)
        .expect("Failed to create .stow-local-ignore file");

    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
        0,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    // Add debug printing for actions if the test fails
    if actions_result.is_err()
        || (actions_result.is_ok()
            && (actions_result
                .as_ref()
                .unwrap()
                .iter()
                .any(|report| report.original_action.target_path.ends_with("test_script"))
                || actions_result
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|report| report.original_action.target_path.ends_with(".bashrc"))
                || actions_result
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|report| report.original_action.target_path.ends_with("README.md"))
                || !actions_result.as_ref().unwrap().iter().any(|report| {
                    report
                        .original_action
                        .target_path
                        .ends_with(".config/nvim/init.vim")
                })))
    {
        eprintln!("--- DEBUG: test_custom_ignore_patterns --- ACTIONS (on potential failure) ---");
        if let Ok(actions) = &actions_result {
            for report in actions {
                if let Some(item) = &report.original_action.source_item {
                    eprintln!(
                        "  Action: Target: {:?}, SourceItem.rel: {:?}, SourceItem.processed_name: {:?}, LinkTarget: {:?}",
                        report.original_action.target_path,
                        item.package_relative_path, // original name in package
                        item.target_name_after_dotfiles_processing, // name after dot- conversion
                        report.original_action.link_target_path
                    );
                } else {
                    eprintln!(
                        "  Action (no source_item): Target: {:?}, LinkTarget: {:?}",
                        report.original_action.target_path, report.original_action.link_target_path
                    );
                }
            }
        } else if let Err(e) = &actions_result {
            eprintln!("  Error: {:?}", e);
        }
        eprintln!("--- END DEBUG --- ACTIONS ---");
    }

    assert!(
        actions_result.is_ok(),
        "stow_packages failed for custom ignore test: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();

    // Verify that custom ignored files are not included
    let has_test_script: bool = actions
        .iter()
        .any(|r| r.original_action.target_path.ends_with("test_script"));
    assert!(
        !has_test_script,
        "test_script (bin/test_script) should be ignored by custom pattern 'bin/test_script'"
    );

    let has_bashrc: bool = actions
        .iter()
        .any(|r| r.original_action.target_path.ends_with(".bashrc"));
    assert!(
        !has_bashrc,
        ".bashrc (from dot-bashrc) should be ignored by custom pattern '.bashrc'"
    );

    let has_readme: bool = actions
        .iter()
        .any(|r| r.original_action.target_path.ends_with("README.md"));
    assert!(
        !has_readme,
        "README.md should be ignored by custom pattern '.*\\.md'"
    );

    let has_nvim_init: bool = actions.iter().any(|r| {
        r.original_action
            .target_path
            .ends_with(".config/nvim/init.vim")
    });
    assert!(
        has_nvim_init,
        ".config/nvim/init.vim (from dot-config/nvim/init.vim) should NOT be ignored"
    );
}

#[test]
fn test_multiple_packages_stow() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package1_name: &str = "package1";
    let package2_name: &str = "package2";
    create_test_package(&stow_dir, package1_name);
    create_test_package(&stow_dir, package2_name);

    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package1_name.to_string(), package2_name.to_string()],
        true, // dotfiles enabled for thoroughness
        0,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed for multiple packages: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();

    // Check for items from both packages (e.g., their respective .bashrc files)
    let p1_bashrc_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .source_item
            .as_ref()
            .map_or(false, |item| {
                item.source_path.to_string_lossy().contains(package1_name)
            })
            && report.original_action.target_path.ends_with(".bashrc")
    });
    assert!(p1_bashrc_exists, "Expected .bashrc from package1");

    let p2_bashrc_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .source_item
            .as_ref()
            .map_or(false, |item| {
                item.source_path.to_string_lossy().contains(package2_name)
            })
            && report.original_action.target_path.ends_with(".bashrc")
    });
    assert!(p2_bashrc_exists, "Expected .bashrc from package2");
}

#[test]
fn test_empty_package_list() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![], // Empty package list
        false,
        0,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed for empty package list: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();
    assert!(
        actions.is_empty(),
        "Expected no actions for an empty package list"
    );
}

#[test]
fn test_nonexistent_package() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "nonexistent_package";

    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    // Expect an error because the package directory won't be found by scan_package
    assert!(
        actions_result.is_err(),
        "Expected stow_packages to fail for a nonexistent package"
    );
    // Optionally, check for the specific error type if your error enum allows
    // e.g., assert!(matches!(actions_result.err().unwrap(), RustowError::Stow(StowError::PackageNotFound(_))));
}

#[test]
fn test_dotfiles_processing_edge_cases() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    // Test case 1: file named "dot-file" in package, should become ".file" in target
    let package_dir_1: PathBuf = stow_dir.join("package1");
    fs::create_dir_all(&package_dir_1).unwrap();
    fs::write(package_dir_1.join("dot-file"), "content for dot-file").unwrap();

    // Test case 2: file starting with "dot-"
    let package_dir_2: PathBuf = stow_dir.join("package2");
    fs::create_dir_all(&package_dir_2).unwrap();
    fs::write(package_dir_2.join("dot-foo-bar"), "content").unwrap();

    // Test case 3: directory starting with "dot-", containing a file
    let package_dir_3: PathBuf = stow_dir.join("package3");
    fs::create_dir_all(&package_dir_3).unwrap();
    let nested_dir_3: PathBuf = package_dir_3.join("dot-dirOnly");
    fs::create_dir_all(&nested_dir_3).unwrap();
    fs::write(nested_dir_3.join("some_file.txt"), "content").unwrap();

    // Test case 4: file NOT starting with "dot-"
    let package_dir_4: PathBuf = stow_dir.join("package4");
    fs::create_dir_all(&package_dir_4).unwrap();
    fs::write(package_dir_4.join("nodotprefix"), "content").unwrap();

    // Test case 5: directory NOT starting with "dot-", containing a file
    let package_dir_5: PathBuf = stow_dir.join("package5");
    fs::create_dir_all(&package_dir_5).unwrap();
    let nested_dir_5: PathBuf = package_dir_5.join("nodotprefix");
    fs::create_dir_all(&nested_dir_5).unwrap();
    fs::write(nested_dir_5.join("file.txt"), "content").unwrap();

    let config: Config = create_test_config(
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
        0,    // Verbosity 0 for this test after debug logs were removed from core
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed for dotfiles edge cases: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();

    // Verify package1: "dot-file" -> ".file"
    let report_pkg1_dot_file: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action
                .source_item
                .as_ref()
                .map_or(false, |item| {
                    item.package_relative_path == Path::new("dot-file")
                        && item.target_name_after_dotfiles_processing == Path::new(".file")
                })
        })
        .expect("Report for package1/dot-file (target: .file) not found");

    assert_eq!(
        report_pkg1_dot_file.status,
        TargetActionReportStatus::Success,
        "Expected package1/dot-file processing to be Success, but got {:?}. Message: {:?}",
        report_pkg1_dot_file.status,
        report_pkg1_dot_file.message
    );
    let expected_target_pkg1_dot_file: PathBuf = target_dir.join(".file");
    assert!(
        expected_target_pkg1_dot_file.exists(),
        "Target .file for package1 was not created. Report details: {:?}",
        report_pkg1_dot_file
    );
    assert!(
        fs::symlink_metadata(&expected_target_pkg1_dot_file)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target .file for package1 is not a symlink. Report details: {:?}",
        report_pkg1_dot_file
    );
    // Optionally, verify link target if it's consistent and knowable
    // let link_target_pkg1 = fs::read_link(&expected_target_pkg1_dot_file).unwrap();
    // let expected_link_source_pkg1 = stow_dir.join("package1").join("dot-file");
    // let expected_relative_link_pkg1 = pathdiff::diff_paths(expected_link_source_pkg1, expected_target_pkg1_dot_file.parent().unwrap()).unwrap();
    // assert_eq!(link_target_pkg1, expected_relative_link_pkg1, "Symlink target for .file is incorrect");

    // Verify package2: "dot-foo-bar" -> ".foo-bar"
    let report_pkg2_dot_foo_bar: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action
                .source_item
                .as_ref()
                .map_or(false, |item| {
                    item.package_relative_path == Path::new("dot-foo-bar") && // Check original path from package
            item.target_name_after_dotfiles_processing == Path::new(".foo-bar")
                })
        })
        .expect("Report for package2/dot-foo-bar (target: .foo-bar) not found");

    assert_eq!(
        report_pkg2_dot_foo_bar.status,
        TargetActionReportStatus::Success,
        "Expected package2/dot-foo-bar processing to be Success, but got {:?}. Message: {:?}",
        report_pkg2_dot_foo_bar.status,
        report_pkg2_dot_foo_bar.message
    );
    let expected_target_pkg2_dot_foo_bar: PathBuf = target_dir.join(".foo-bar");
    assert!(
        expected_target_pkg2_dot_foo_bar.exists(),
        "Target .foo-bar for package2 was not created. Report details: {:?}",
        report_pkg2_dot_foo_bar
    );
    assert!(
        fs::symlink_metadata(&expected_target_pkg2_dot_foo_bar)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target .foo-bar for package2 is not a symlink. Report details: {:?}",
        report_pkg2_dot_foo_bar
    );
    // Optionally, verify link target for .foo-bar

    // Verify package3: "dot-dirOnly" -> ".dirOnly"
    let report_pkg3_dot_dir_only: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action
                .source_item
                .as_ref()
                .map_or(false, |item| {
                    item.package_relative_path == Path::new("dot-dirOnly")
                        && item.target_name_after_dotfiles_processing == Path::new(".dirOnly")
                })
        })
        .expect("Report for package3/dot-dirOnly (target: .dirOnly) not found");

    assert_eq!(
        report_pkg3_dot_dir_only.status,
        TargetActionReportStatus::Success,
        "Expected package3/dot-dirOnly processing to be Success, but got {:?}. Message: {:?}",
        report_pkg3_dot_dir_only.status,
        report_pkg3_dot_dir_only.message
    );
    let expected_target_pkg3_dot_dir_only: PathBuf = target_dir.join(".dirOnly");
    assert!(
        expected_target_pkg3_dot_dir_only.exists(),
        "Target .dirOnly for package3 was not created. Report details: {:?}",
        report_pkg3_dot_dir_only
    );
    assert!(
        expected_target_pkg3_dot_dir_only.is_dir(),
        "Target .dirOnly for package3 is not a directory. Report details: {:?}",
        report_pkg3_dot_dir_only
    );
    assert_eq!(
        report_pkg3_dot_dir_only.original_action.action_type,
        ActionType::CreateDirectory,
        "ActionType for .dirOnly should be CreateDirectory"
    );

    // Verify package3: "dot-dirOnly/some_file.txt" -> ".dirOnly/some_file.txt"
    let report_pkg3_nested_file: &rustow::stow::TargetActionReport = actions.iter().find(|r| {
        r.original_action.source_item.as_ref().map_or(false, |item| {
            item.package_relative_path == Path::new("dot-dirOnly/some_file.txt") &&
            item.target_name_after_dotfiles_processing == Path::new(".dirOnly/some_file.txt")
        })
    }).expect("Report for package3/dot-dirOnly/some_file.txt (target: .dirOnly/some_file.txt) not found");

    assert_eq!(
        report_pkg3_nested_file.status,
        TargetActionReportStatus::Success,
        "Expected package3/dot-dirOnly/some_file.txt processing to be Success, but got {:?}. Message: {:?}",
        report_pkg3_nested_file.status,
        report_pkg3_nested_file.message
    );
    let expected_target_pkg3_nested_file: PathBuf = target_dir.join(".dirOnly/some_file.txt");
    assert!(
        expected_target_pkg3_nested_file.exists(),
        "Target .dirOnly/some_file.txt for package3 was not created. Report details: {:?}",
        report_pkg3_nested_file
    );
    assert!(
        fs::symlink_metadata(&expected_target_pkg3_nested_file)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target .dirOnly/some_file.txt for package3 is not a symlink. Report details: {:?}",
        report_pkg3_nested_file
    );
    assert_eq!(
        report_pkg3_nested_file.original_action.action_type,
        ActionType::CreateSymlink,
        "ActionType for .dirOnly/some_file.txt should be CreateSymlink"
    );

    // Verify package4: "nodotprefix" -> "nodotprefix"
    let report_pkg4_nodotprefix_file: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action
                .source_item
                .as_ref()
                .map_or(false, |item| {
                    item.package_relative_path == Path::new("nodotprefix")
                        && item.target_name_after_dotfiles_processing == Path::new("nodotprefix")
                        && item.item_type == StowItemType::File // Ensure we are checking the file from package4
                })
        })
        .expect("Report for package4/nodotprefix (target: nodotprefix) not found");

    assert_eq!(
        report_pkg4_nodotprefix_file.status,
        TargetActionReportStatus::ConflictPrevented, // EXPECT CONFLICT
        "Expected package4/nodotprefix processing to be ConflictPrevented, but got {:?}. Message: {:?}",
        report_pkg4_nodotprefix_file.status,
        report_pkg4_nodotprefix_file.message
    );
    assert_eq!(
        report_pkg4_nodotprefix_file.original_action.action_type,
        ActionType::Conflict,
        "ActionType for package4/nodotprefix should be Conflict"
    );
    let expected_target_pkg4_nodotprefix_file: PathBuf = target_dir.join("nodotprefix");
    // In case of conflict, the file from package4 might not be created,
    // or if an earlier package created something, it might remain.
    // For this specific test, package4 is processed before package5.
    // So, if package4 attempts to create a symlink and package5 attempts a directory,
    // one of them will be a conflict. If stow_packages processes them sequentially
    // and the conflict detection is global *after* all plans, then execute_actions sees the Conflict.
    // Let's assume the symlink from package4 *would* have been created if no conflict from package5 existed.
    // However, because of the conflict, its action is marked Conflict, and it should NOT be created by execute_actions.
    // The *other* conflicting item (from package5) will also be marked Conflict.
    // So, the target path should ideally be empty or untouched by these conflicting actions.
    // However, the current execute_actions creates the *first* non-conflicting item if multiple packages target the same path
    // before the inter-package conflict marks them all as Conflict. This needs refinement.
    // For now, we will assert that the final state of the target does not correspond to package4's successful symlink
    // when a conflict with package5 is present and detected.
    // If the conflict is properly handled, neither package4's file symlink nor package5's directory should solely occupy the target.
    // The test output showed package4's symlink IS created, then package5's dir fails.
    // This implies the conflict detection in plan_actions might not be preventing the *first* action if multiple packages are involved.
    // Let's adjust the expectation: package4 creates its link, then package5's dir creation action is marked as Conflict.
    // No, the new `plan_actions` inter-package conflict should mark BOTH as conflict *before* execution.

    // After proper conflict handling, the target path should not be a symlink *from package4* specifically
    // if the conflict with package5 (directory) was identified *before* execution.
    // It's possible the target path remains as it was before any operation from these two packages.
    // For this test, we'll assume if it's a conflict, the symlink from package4 does not get created.
    // This means the assertion `expected_target_pkg4_nodotprefix_file.exists()` might be false.
    // And `is_symlink` would also be false or panic if it doesn't exist.

    // Given the current test failure (package4's symlink IS created),
    // the inter-package conflict detection in `plan_actions` might not be effective across packages,
    // or `execute_actions` doesn't respect it fully for the first item.
    // Let's assume the `plan_actions` conflict detection *should* prevent creation.
    assert!(
        !expected_target_pkg4_nodotprefix_file.exists()
            || !fs::symlink_metadata(&expected_target_pkg4_nodotprefix_file)
                .map_or(false, |m| m.file_type().is_symlink()),
        "Target nodotprefix for package4 SHOULD NOT be a symlink due to conflict. Current state: exists={}, is_symlink={}",
        expected_target_pkg4_nodotprefix_file.exists(),
        expected_target_pkg4_nodotprefix_file.exists()
            && fs::symlink_metadata(&expected_target_pkg4_nodotprefix_file)
                .map_or(false, |m| m.file_type().is_symlink())
    );

    // Verify package5: "nodotprefix/file.txt" -> "nodotprefix/file.txt"
    // First, verify the parent directory "nodotprefix" for package5
    let report_pkg5_nodotprefix_dir: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action
                .source_item
                .as_ref()
                .map_or(false, |item| {
                    item.package_relative_path == Path::new("nodotprefix")
                        && item.target_name_after_dotfiles_processing == Path::new("nodotprefix")
                        && item.item_type == StowItemType::Directory
                })
        })
        .expect("Report for package5/nodotprefix (directory) not found");

    assert_eq!(
        report_pkg5_nodotprefix_dir.status,
        TargetActionReportStatus::ConflictPrevented, // EXPECT CONFLICT
        "Expected package5/nodotprefix (directory) processing to be ConflictPrevented, but got {:?}. Message: {:?}",
        report_pkg5_nodotprefix_dir.status,
        report_pkg5_nodotprefix_dir.message
    );
    assert_eq!(
        report_pkg5_nodotprefix_dir.original_action.action_type,
        ActionType::Conflict,
        "ActionType for package5/nodotprefix (directory) should be Conflict"
    );
    let expected_target_pkg5_nodotprefix_dir: PathBuf = target_dir.join("nodotprefix");
    // If conflict is properly handled, package5's directory should not be created.
    assert!(
        !expected_target_pkg5_nodotprefix_dir.exists()
            || !expected_target_pkg5_nodotprefix_dir.is_dir(),
        "Target nodotprefix for package5 SHOULD NOT be a directory due to conflict. Current state: exists={}, is_dir={}",
        expected_target_pkg5_nodotprefix_dir.exists(),
        expected_target_pkg5_nodotprefix_dir.exists()
            && expected_target_pkg5_nodotprefix_dir.is_dir()
    );

    // Next, verify the nested file "nodotprefix/file.txt" for package5
    let report_pkg5_nested_file: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action
                .source_item
                .as_ref()
                .map_or(false, |item| {
                    item.package_relative_path == Path::new("nodotprefix/file.txt")
                        && item.target_name_after_dotfiles_processing
                            == Path::new("nodotprefix/file.txt")
                })
        })
        .expect("Report for package5/nodotprefix/file.txt not found");

    // If the parent directory `nodotprefix` for package5 is a Conflict,
    // then the nested file should also be treated as a Conflict or at least not Success.
    assert_eq!(
        report_pkg5_nested_file.status,
        TargetActionReportStatus::ConflictPrevented, // EXPECT CONFLICT (due to parent conflict)
        "Expected package5/nodotprefix/file.txt processing to be ConflictPrevented due to parent, but got {:?}. Message: {:?}",
        report_pkg5_nested_file.status,
        report_pkg5_nested_file.message
    );
    assert_eq!(
        report_pkg5_nested_file.original_action.action_type,
        ActionType::Conflict,
        "ActionType for package5/nodotprefix/file.txt should be Conflict due to parent"
    );
    let expected_target_pkg5_nested_file: PathBuf = target_dir.join("nodotprefix/file.txt");
    assert!(
        !expected_target_pkg5_nested_file.exists(),
        "Target nodotprefix/file.txt for package5 SHOULD NOT exist due to parent conflict. Current state: exists={}",
        expected_target_pkg5_nested_file.exists()
    );
}

// Note: True relative path calculation for symlinks is complex and depends on the target OS's symlink behavior.
// These tests will primarily check if the `link_target_path` in `TargetAction` appears plausible (e.g., relative).
// Actual symlink creation and resolution would be tested in end-to-end execution tests (not just planning).
#[test]
fn test_relative_path_calculation_basic() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "relpath_pkg";
    create_test_package(&stow_dir, package_name);

    let config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false, // dotfiles status doesn't fundamentally change relativity expectation
        0,
    );

    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed for relative path test: {:?}",
        actions_result.err()
    );
    let actions: Vec<rustow::stow::TargetActionReport> = actions_result.unwrap();

    for report in actions
        .iter()
        .filter(|r| r.original_action.link_target_path.is_some())
    {
        let link_target: &PathBuf = report.original_action.link_target_path.as_ref().unwrap();
        // A simple check: relative paths should not start with '/' or a drive letter (on Windows)
        assert!(
            !link_target.is_absolute(),
            "Link target path {:?} for target {:?} should be relative",
            link_target,
            report.original_action.target_path
        );
        // More robust check: ensure it navigates upwards (e.g., starts with "..")
        // This depends on the depth. For a top-level file like 'dot-bashrc' -> '../stow_dir/pkg/dot-bashrc'
        // For 'bin/test_script' -> '../../stow_dir/pkg/bin/test_script'
        // This is a basic sanity check.
        assert!(
            link_target.starts_with(".."),
            "Link target path {:?} for target {:?} should typically start with '..' to go from target to stow dir item",
            link_target,
            report.original_action.target_path
        );
    }
}

#[test]
fn test_config_integration_verbosity_and_simulate() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "config_test_pkg";
    create_test_package(&stow_dir, package_name);

    let args: Args = Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        packages: vec![package_name.to_string()],
        simulate: true,
        verbose: 3,
        delete: false,
        restow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: vec![],
        defer_conflicts: vec![],
        ignore_patterns: vec![],
    };

    let config_result: Result<Config, rustow::error::RustowError> = Config::from_args(args);
    assert!(
        config_result.is_ok(),
        "Config creation failed: {:?}",
        config_result.err()
    );
    let config: Config = config_result.unwrap();

    assert!(config.simulate, "Config.simulate should be true");
    assert_eq!(config.verbosity, 3, "Config.verbosity should be 3");

    // stow_packages should still work and plan actions even in simulate mode
    let actions_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        stow_packages(&config);
    assert!(
        actions_result.is_ok(),
        "stow_packages failed with simulate config: {:?}",
        actions_result.err()
    );
    assert!(
        !actions_result.unwrap().is_empty(),
        "Expected actions even in simulate mode"
    );
}

#[test]
fn test_plan_actions_basic_creation_and_conflict() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "plan_test_pkg";
    let package_dir: PathBuf = stow_dir.join(package_name);
    fs::create_dir_all(&package_dir).unwrap();

    fs::write(package_dir.join("file_to_link.txt"), "link me").unwrap();
    let dir_to_create_in_pkg: PathBuf = package_dir.join("dir_to_create");
    fs::create_dir_all(&dir_to_create_in_pkg).unwrap();
    fs::write(dir_to_create_in_pkg.join("nested_file.txt"), "i am nested").unwrap();
    fs::write(
        package_dir.join("file_for_conflict.txt"),
        "conflict file content",
    )
    .unwrap();
    let dir_for_conflict_in_pkg: PathBuf = package_dir.join("dir_for_conflict");
    fs::create_dir_all(&dir_for_conflict_in_pkg).unwrap();
    fs::write(
        dir_for_conflict_in_pkg.join("another_nested.txt"),
        "nested conflict",
    )
    .unwrap();

    let config_empty_target: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );

    let actions_empty_result: Result<
        Vec<rustow::stow::TargetActionReport>,
        rustow::error::RustowError,
    > = stow_packages(&config_empty_target);
    assert!(
        actions_empty_result.is_ok(),
        "stow_packages failed for empty target: {:?}",
        actions_empty_result.err()
    );
    let actions_empty: Vec<rustow::stow::TargetActionReport> = actions_empty_result.unwrap();

    let action_file_to_link: Option<&rustow::stow::TargetActionReport> =
        actions_empty.iter().find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == OsStr::new("file_to_link.txt"))
        });
    assert!(
        action_file_to_link.is_some(),
        "Action for file_to_link.txt not found"
    );
    assert_eq!(
        action_file_to_link.unwrap().original_action.action_type,
        ActionType::CreateSymlink,
        "Expected CreateSymlink for file_to_link.txt"
    );
    assert!(
        action_file_to_link
            .unwrap()
            .original_action
            .link_target_path
            .is_some(),
        "Link target path should exist for CreateSymlink"
    );

    let action_dir_to_create: Option<&rustow::stow::TargetActionReport> =
        actions_empty.iter().find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == OsStr::new("dir_to_create"))
        });
    assert!(
        action_dir_to_create.is_some(),
        "Action for dir_to_create not found"
    );
    assert_eq!(
        action_dir_to_create.unwrap().original_action.action_type,
        ActionType::CreateDirectory,
        "Expected CreateDirectory for dir_to_create"
    );
    assert!(
        action_dir_to_create
            .unwrap()
            .original_action
            .link_target_path
            .is_none(),
        "Link target path should be None for CreateDirectory"
    );

    let action_nested_file: Option<&rustow::stow::TargetActionReport> =
        actions_empty.iter().find(|r| {
            r.original_action
                .target_path
                .ends_with(Path::new("dir_to_create/nested_file.txt"))
        });
    assert!(
        action_nested_file.is_some(),
        "Action for dir_to_create/nested_file.txt not found"
    );
    assert_eq!(
        action_nested_file.unwrap().original_action.action_type,
        ActionType::CreateSymlink,
        "Expected CreateSymlink for nested_file.txt"
    );

    let target_file_conflict_path: PathBuf = target_dir.join("file_for_conflict.txt");
    // Remove any existing symlink first, then create a regular file
    if target_file_conflict_path.exists() {
        fs::remove_file(&target_file_conflict_path).unwrap();
    }
    fs::write(&target_file_conflict_path, "existing target file content").unwrap();

    let config_file_conflict: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );
    let actions_file_conflict_result: Result<
        Vec<rustow::stow::TargetActionReport>,
        rustow::error::RustowError,
    > = stow_packages(&config_file_conflict);
    assert!(
        actions_file_conflict_result.is_ok(),
        "stow_packages failed for file conflict: {:?}",
        actions_file_conflict_result.err()
    );
    let actions_file_conflict: Vec<rustow::stow::TargetActionReport> =
        actions_file_conflict_result.unwrap();

    let action_conflicting_file: Option<&rustow::stow::TargetActionReport> =
        actions_file_conflict.iter().find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == OsStr::new("file_for_conflict.txt"))
        });
    assert!(
        action_conflicting_file.is_some(),
        "Action for file_for_conflict.txt not found in conflict scenario"
    );
    assert_eq!(
        action_conflicting_file.unwrap().original_action.action_type,
        ActionType::Conflict,
        "Expected Conflict for file_for_conflict.txt"
    );
    assert!(
        action_conflicting_file
            .unwrap()
            .original_action
            .conflict_details
            .is_some(),
        "Conflict details should be present"
    );
    assert!(
        action_conflicting_file
            .unwrap()
            .original_action
            .link_target_path
            .is_none(),
        "Link target should be None for Conflict"
    );

    fs::remove_file(target_file_conflict_path).unwrap();

    let target_dir_conflict_path: PathBuf = target_dir.join("dir_for_conflict");
    fs::create_dir_all(&target_dir_conflict_path).unwrap();
    fs::write(
        target_dir_conflict_path.join("existing_file_in_target_dir.txt"),
        "dummy",
    )
    .unwrap();

    let config_dir_conflict: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );
    let actions_dir_conflict_result: Result<
        Vec<rustow::stow::TargetActionReport>,
        rustow::error::RustowError,
    > = stow_packages(&config_dir_conflict);
    assert!(
        actions_dir_conflict_result.is_ok(),
        "stow_packages failed for dir conflict: {:?}",
        actions_dir_conflict_result.err()
    );
    let actions_dir_conflict: Vec<rustow::stow::TargetActionReport> =
        actions_dir_conflict_result.unwrap();

    let action_conflicting_dir: Option<&rustow::stow::TargetActionReport> =
        actions_dir_conflict.iter().find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == OsStr::new("dir_for_conflict"))
        });
    assert!(
        action_conflicting_dir.is_some(),
        "Action for dir_for_conflict not found in conflict scenario"
    );
    assert_eq!(
        action_conflicting_dir.unwrap().original_action.action_type,
        ActionType::Conflict,
        "Expected Conflict for dir_for_conflict"
    );
    assert!(
        action_conflicting_dir
            .unwrap()
            .original_action
            .conflict_details
            .is_some(),
        "Conflict details should be present for dir conflict"
    );
    assert!(
        action_conflicting_dir
            .unwrap()
            .original_action
            .link_target_path
            .is_none(),
        "Link target should be None for dir Conflict"
    );

    let action_nested_in_conflicting_dir: Option<&rustow::stow::TargetActionReport> =
        actions_dir_conflict.iter().find(|r| {
            r.original_action
                .target_path
                .ends_with(Path::new("dir_for_conflict/another_nested.txt"))
        });
    assert!(
        action_nested_in_conflicting_dir.is_some(),
        "Action for dir_for_conflict/another_nested.txt not found"
    );
    assert_eq!(
        action_nested_in_conflicting_dir
            .unwrap()
            .original_action
            .action_type,
        ActionType::Conflict,
        "Expected Conflict for item in conflicting dir"
    );

    fs::remove_dir_all(target_dir_conflict_path).unwrap();
}

#[test]
fn test_execute_actions_basic_creation() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    // --- Scenario 1: Create Directory ---
    let pkg1_name: &str = "pkg_exec_dir";
    let package1_dir: PathBuf = stow_dir.join(pkg1_name);
    fs::create_dir_all(&package1_dir).unwrap();
    fs::create_dir_all(package1_dir.join("my_dir")).unwrap(); // Item to be created as directory

    let mut config1: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![pkg1_name.to_string()],
        false, // dotfiles disabled for simplicity here
        0,     // verbosity
    );

    // Plan actions
    let planned_actions1_result: Result<
        Vec<rustow::stow::TargetActionReport>,
        rustow::error::RustowError,
    > = rustow::stow::stow_packages(&config1); // This now plans AND executes
    // We need to separate planning and execution for this test,
    // or adapt stow_packages if it now returns reports.
    // For now, assuming stow_packages is refactored or we call plan then execute.
    // Let's assume `stow_packages` now returns reports.

    assert!(
        planned_actions1_result.is_ok(),
        "stow_packages (plan+exec) for dir creation failed: {:?}",
        planned_actions1_result.err()
    );
    let reports1: Vec<rustow::stow::TargetActionReport> = planned_actions1_result.unwrap();

    assert_eq!(
        reports1.len(),
        1,
        "Expected 1 report for single directory creation"
    );
    let report_dir: &rustow::stow::TargetActionReport = reports1
        .iter()
        .find(|r| r.original_action.target_path.ends_with("my_dir"))
        .expect("Report for my_dir not found");

    assert_eq!(
        report_dir.status,
        rustow::stow::TargetActionReportStatus::Success,
        "Directory creation status was not Success"
    );
    assert!(
        target_dir.join("my_dir").exists(),
        "Target directory my_dir was not created"
    );
    assert!(
        target_dir.join("my_dir").is_dir(),
        "Target my_dir is not a directory"
    );

    // --- Scenario 1.1: Create Directory (Simulate) ---
    let pkg1_sim_name: &str = "pkg_exec_dir_sim";
    let package1_sim_dir: PathBuf = stow_dir.join(pkg1_sim_name);
    fs::create_dir_all(&package1_sim_dir).unwrap();
    fs::create_dir_all(package1_sim_dir.join("my_dir_sim")).unwrap();

    config1.packages = vec![pkg1_sim_name.to_string()];
    config1.simulate = true;
    let reports1_sim_result: Result<
        Vec<rustow::stow::TargetActionReport>,
        rustow::error::RustowError,
    > = rustow::stow::stow_packages(&config1);
    assert!(
        reports1_sim_result.is_ok(),
        "stow_packages (simulate) for dir creation failed: {:?}",
        reports1_sim_result.err()
    );
    let reports1_sim: Vec<rustow::stow::TargetActionReport> = reports1_sim_result.unwrap();

    assert_eq!(reports1_sim.len(), 1);
    let report_dir_sim: &rustow::stow::TargetActionReport = reports1_sim
        .iter()
        .find(|r| r.original_action.target_path.ends_with("my_dir_sim"))
        .expect("Report for my_dir_sim not found");
    assert_eq!(
        report_dir_sim.status,
        rustow::stow::TargetActionReportStatus::Skipped
    );
    assert!(
        report_dir_sim
            .message
            .as_ref()
            .unwrap()
            .contains("SIMULATE")
    );
    assert!(
        !target_dir.join("my_dir_sim").exists(),
        "Target directory my_dir_sim should not have been created in simulate mode"
    );
    config1.simulate = false; // Reset for next tests

    // --- Scenario 2: Create Symlink (File) ---
    let pkg2_name: &str = "pkg_exec_file_link";
    let package2_dir: PathBuf = stow_dir.join(pkg2_name);
    fs::create_dir_all(&package2_dir).unwrap();
    fs::write(package2_dir.join("my_file.txt"), "Hello Rustow!").unwrap();

    let mut config2: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![pkg2_name.to_string()],
        false,
        0,
    );
    let reports2_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        rustow::stow::stow_packages(&config2);
    assert!(
        reports2_result.is_ok(),
        "stow_packages for file link failed: {:?}",
        reports2_result.err()
    );
    let reports2: Vec<rustow::stow::TargetActionReport> = reports2_result.unwrap();

    assert_eq!(
        reports2.len(),
        1,
        "Expected 1 report for file link creation"
    );
    let report_file_link: &rustow::stow::TargetActionReport = reports2
        .iter()
        .find(|r| r.original_action.target_path.ends_with("my_file.txt"))
        .expect("Report for my_file.txt not found");
    assert_eq!(
        report_file_link.status,
        rustow::stow::TargetActionReportStatus::Success,
        "File link creation status not Success"
    );
    let target_file_path: PathBuf = target_dir.join("my_file.txt");
    assert!(
        target_file_path.exists(),
        "Target file link my_file.txt was not created"
    );
    assert!(
        fs::symlink_metadata(&target_file_path)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target my_file.txt is not a symlink"
    );
    let link_target: PathBuf = fs::read_link(&target_file_path).unwrap();
    // Assuming relative link from target_dir to stow_dir/pkg_exec_file_link/my_file.txt
    let expected_link_target_relative: PathBuf = PathBuf::from("..")
        .join(stow_dir.file_name().unwrap())
        .join(pkg2_name)
        .join("my_file.txt");
    assert_eq!(
        link_target, expected_link_target_relative,
        "Symlink target is incorrect"
    );

    // --- Scenario 2.1: Create Symlink (File) (Simulate) ---
    let pkg2_sim_name: &str = "pkg_exec_file_link_sim";
    let package2_sim_dir: PathBuf = stow_dir.join(pkg2_sim_name);
    fs::create_dir_all(&package2_sim_dir).unwrap();
    fs::write(package2_sim_dir.join("my_file_sim.txt"), "Simulate Hello!").unwrap();

    config2.packages = vec![pkg2_sim_name.to_string()];
    config2.simulate = true;
    let reports2_sim_result: Result<
        Vec<rustow::stow::TargetActionReport>,
        rustow::error::RustowError,
    > = rustow::stow::stow_packages(&config2);
    assert!(
        reports2_sim_result.is_ok(),
        "stow_packages (simulate) for file link failed: {:?}",
        reports2_sim_result.err()
    );
    let reports2_sim: Vec<rustow::stow::TargetActionReport> = reports2_sim_result.unwrap();

    assert_eq!(reports2_sim.len(), 1);
    let report_file_link_sim: &rustow::stow::TargetActionReport = reports2_sim
        .iter()
        .find(|r| r.original_action.target_path.ends_with("my_file_sim.txt"))
        .expect("Report for my_file_sim.txt not found");
    assert_eq!(
        report_file_link_sim.status,
        rustow::stow::TargetActionReportStatus::Skipped
    );
    assert!(
        !target_dir.join("my_file_sim.txt").exists(),
        "Target file my_file_sim.txt should not exist in simulate mode"
    );
    config2.simulate = false;

    // --- Scenario 3: Create Symlink (Nested File, Parent Dir Auto-Creation) ---
    let pkg3_name: &str = "pkg_exec_nested_link";
    let package3_dir: PathBuf = stow_dir.join(pkg3_name);
    let nested_parent_dir_in_pkg: PathBuf = package3_dir.join("parent_dir");
    fs::create_dir_all(&nested_parent_dir_in_pkg).unwrap();
    fs::write(
        nested_parent_dir_in_pkg.join("nested_file.txt"),
        "Nested Hello!",
    )
    .unwrap();

    let config3: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![pkg3_name.to_string()],
        false,
        0,
    );
    let reports3_result: Result<Vec<rustow::stow::TargetActionReport>, rustow::error::RustowError> =
        rustow::stow::stow_packages(&config3);
    assert!(
        reports3_result.is_ok(),
        "stow_packages for nested link failed: {:?}",
        reports3_result.err()
    );
    let reports3: Vec<rustow::stow::TargetActionReport> = reports3_result.unwrap();

    // Expected: 1 action for parent_dir (CreateDirectory), 1 for nested_file.txt (CreateSymlink)
    assert_eq!(
        reports3.len(),
        2,
        "Expected 2 reports for nested link creation (dir + file)"
    );

    let report_parent_dir: &rustow::stow::TargetActionReport = reports3
        .iter()
        .find(|r| r.original_action.target_path.ends_with("parent_dir"))
        .expect("Report for parent_dir not found");
    assert_eq!(
        report_parent_dir.original_action.action_type,
        ActionType::CreateDirectory
    );
    assert_eq!(
        report_parent_dir.status,
        rustow::stow::TargetActionReportStatus::Success,
        "Parent directory creation status not Success"
    );
    let target_parent_dir_path: PathBuf = target_dir.join("parent_dir");
    assert!(
        target_parent_dir_path.exists(),
        "Target parent_dir was not created"
    );
    assert!(
        target_parent_dir_path.is_dir(),
        "Target parent_dir is not a directory"
    );

    let report_nested_link: &rustow::stow::TargetActionReport = reports3
        .iter()
        .find(|r| r.original_action.target_path.ends_with("nested_file.txt"))
        .expect("Report for nested_file.txt not found");
    assert_eq!(
        report_nested_link.original_action.action_type,
        ActionType::CreateSymlink
    );
    assert_eq!(
        report_nested_link.status,
        rustow::stow::TargetActionReportStatus::Success,
        "Nested link creation status not Success"
    );
    let target_nested_file_path: PathBuf = target_parent_dir_path.join("nested_file.txt");
    assert!(
        target_nested_file_path.exists(),
        "Target nested_file.txt was not created"
    );
    assert!(
        fs::symlink_metadata(&target_nested_file_path)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target nested_file.txt is not a symlink"
    );
    let nested_link_target: PathBuf = fs::read_link(&target_nested_file_path).unwrap();
    // Assuming relative link from target_dir/parent_dir to stow_dir/pkg_exec_nested_link/parent_dir/nested_file.txt
    let expected_nested_link_target: PathBuf = PathBuf::from("..")
        .join("..")
        .join(stow_dir.file_name().unwrap())
        .join(pkg3_name)
        .join("parent_dir")
        .join("nested_file.txt");
    assert_eq!(
        nested_link_target, expected_nested_link_target,
        "Nested symlink target is incorrect"
    );

    // --- Scenario 3.1: Nested Link (Simulate) ---
    let pkg3_sim_name: &str = "pkg_exec_nested_link_sim";
    let package3_sim_dir: PathBuf = stow_dir.join(pkg3_sim_name);
    let nested_parent_sim_dir_in_pkg: PathBuf = package3_sim_dir.join("parent_dir_sim");
    fs::create_dir_all(&nested_parent_sim_dir_in_pkg).unwrap();
    fs::write(
        nested_parent_sim_dir_in_pkg.join("nested_file_sim.txt"),
        "Nested Sim Hello!",
    )
    .unwrap();

    let mut config3_sim: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![pkg3_sim_name.to_string()],
        false,
        0,
    );
    config3_sim.simulate = true;
    let reports3_sim_result: Result<
        Vec<rustow::stow::TargetActionReport>,
        rustow::error::RustowError,
    > = rustow::stow::stow_packages(&config3_sim);
    assert!(
        reports3_sim_result.is_ok(),
        "stow_packages (simulate) for nested link failed: {:?}",
        reports3_sim_result.err()
    );
    let reports3_sim: Vec<rustow::stow::TargetActionReport> = reports3_sim_result.unwrap();

    assert_eq!(reports3_sim.len(), 2);
    let report_parent_dir_sim: &rustow::stow::TargetActionReport = reports3_sim
        .iter()
        .find(|r| r.original_action.target_path.ends_with("parent_dir_sim"))
        .expect("Report for parent_dir_sim not found (simulate)");
    assert_eq!(
        report_parent_dir_sim.status,
        rustow::stow::TargetActionReportStatus::Skipped
    );
    let report_nested_link_sim: &rustow::stow::TargetActionReport = reports3_sim
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .ends_with("nested_file_sim.txt")
        })
        .expect("Report for nested_file_sim.txt not found (simulate)");
    assert_eq!(
        report_nested_link_sim.status,
        rustow::stow::TargetActionReportStatus::Skipped
    );
    assert!(
        !target_dir.join("parent_dir_sim").exists(),
        "Target parent_dir_sim should not exist in simulate mode"
    );
    assert!(
        !target_dir
            .join("parent_dir_sim/nested_file_sim.txt")
            .exists(),
        "Target nested_file_sim.txt should not exist in simulate mode"
    );

    // TODO: Add Scenario 4 (which is more specific about --no-folding if it differs)
    // For now, Scenario 3 covers parent dir creation implicitly handled by CreateSymlink's logic.
}

// Add more tests as needed:
// - Conflicting files/directories (needs fs_utils to check existence in target for planning)
// - `--adopt` functionality (needs more involved setup and fs_utils checks)
// - `--no-folding` (needs directory structures that would normally fold)
// - Delete and Restow operations (would need to plan Delete actions or sequence of Delete/Create)

/// Test delete mode functionality
#[test]
fn test_delete_mode_basic() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_delete_pkg";
    create_test_package(&stow_dir, package_name);

    // First, stow the package to create symlinks
    let stow_config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );

    let stow_result = stow_packages(&stow_config);
    assert!(
        stow_result.is_ok(),
        "Initial stow failed: {:?}",
        stow_result.err()
    );

    // Verify symlinks were created
    assert!(
        target_dir.join("bin").exists(),
        "bin directory should exist after stow"
    );
    assert!(
        target_dir.join("bin/test_script").exists(),
        "test_script symlink should exist after stow"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/test_script"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "test_script should be a symlink"
    );

    // Now test delete mode
    let mut delete_config = stow_config.clone();
    delete_config.mode = StowMode::Delete;

    let delete_result = delete_packages(&delete_config);
    assert!(
        delete_result.is_ok(),
        "Delete operation failed: {:?}",
        delete_result.err()
    );
    let delete_reports = delete_result.unwrap();

    // Verify symlinks were removed
    assert!(
        !target_dir.join("bin/test_script").exists(),
        "test_script symlink should be removed after delete"
    );

    // Check that directories are cleaned up if empty
    // Note: The bin directory might still exist if it's not empty or if our implementation doesn't clean it up
    // This depends on the specific implementation of delete_packages

    // Verify reports indicate successful deletion
    let script_delete_report = delete_reports
        .iter()
        .find(|r| r.original_action.target_path.ends_with("test_script"));
    assert!(
        script_delete_report.is_some(),
        "Should have a delete report for test_script"
    );
    assert_eq!(
        script_delete_report.unwrap().status,
        TargetActionReportStatus::Success,
        "Delete operation should be successful"
    );
}

/// Test delete mode with dotfiles
#[test]
fn test_delete_mode_with_dotfiles() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_delete_dotfiles_pkg";
    create_test_package(&stow_dir, package_name);

    // First, stow the package with dotfiles enabled
    let stow_config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
        0,
    );

    let stow_result = stow_packages(&stow_config);
    assert!(
        stow_result.is_ok(),
        "Initial stow with dotfiles failed: {:?}",
        stow_result.err()
    );

    // Verify dotfiles symlinks were created
    assert!(
        target_dir.join(".bashrc").exists(),
        ".bashrc symlink should exist after stow"
    );
    assert!(
        target_dir.join(".config").exists(),
        ".config directory should exist after stow"
    );
    assert!(
        fs::symlink_metadata(target_dir.join(".bashrc"))
            .unwrap()
            .file_type()
            .is_symlink(),
        ".bashrc should be a symlink"
    );

    // Now test delete mode with dotfiles
    let mut delete_config = stow_config.clone();
    delete_config.mode = StowMode::Delete;

    let delete_result = delete_packages(&delete_config);
    assert!(
        delete_result.is_ok(),
        "Delete operation with dotfiles failed: {:?}",
        delete_result.err()
    );

    // Verify dotfiles symlinks were removed
    assert!(
        !target_dir.join(".bashrc").exists(),
        ".bashrc symlink should be removed after delete"
    );

    // Note: .config directory structure handling depends on implementation
    // It might remain if it contains other files or if our delete logic doesn't handle nested structures
}

/// Test delete mode when target doesn't exist (should skip gracefully)
#[test]
fn test_delete_mode_nonexistent_target() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_delete_nonexistent_pkg";
    create_test_package(&stow_dir, package_name);

    // Don't stow first - try to delete when nothing is stowed
    let delete_config: Config = Config {
        stow_dir,
        target_dir,
        packages: vec![package_name.to_string()],
        mode: StowMode::Delete,
        stow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        overrides: Vec::new(),
        defers: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbosity: 0,
        home_dir: std::env::temp_dir(),
    };

    let delete_result = delete_packages(&delete_config);
    assert!(
        delete_result.is_ok(),
        "Delete operation should succeed even when targets don't exist: {:?}",
        delete_result.err()
    );

    let delete_reports = delete_result.unwrap();

    // All operations should be skipped since targets don't exist
    for report in &delete_reports {
        assert_eq!(
            report.status,
            TargetActionReportStatus::Skipped,
            "All delete operations should be skipped when targets don't exist"
        );
    }
}

/// Test delete mode with non-stow symlinks (should skip them)
#[test]
fn test_delete_mode_non_stow_symlinks() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_delete_non_stow_pkg";
    create_test_package(&stow_dir, package_name);

    // Create a non-stow symlink in the target directory
    let external_file = _temp_dir.path().join("external_file.txt");
    fs::write(&external_file, "external content").unwrap();

    fs::create_dir_all(target_dir.join("bin")).unwrap();
    std::os::unix::fs::symlink(&external_file, target_dir.join("bin/test_script")).unwrap();

    // Now try to delete - should skip the non-stow symlink
    let delete_config: Config = Config {
        stow_dir,
        target_dir: target_dir.clone(),
        packages: vec![package_name.to_string()],
        mode: StowMode::Delete,
        stow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        overrides: Vec::new(),
        defers: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbosity: 0,
        home_dir: std::env::temp_dir(),
    };

    let delete_result = delete_packages(&delete_config);
    assert!(
        delete_result.is_ok(),
        "Delete operation should succeed with non-stow symlinks: {:?}",
        delete_result.err()
    );

    // The non-stow symlink should still exist
    assert!(
        target_dir.join("bin/test_script").exists(),
        "Non-stow symlink should not be deleted"
    );

    let delete_reports = delete_result.unwrap();
    let script_report = delete_reports
        .iter()
        .find(|r| r.original_action.target_path.ends_with("test_script"));
    assert!(
        script_report.is_some(),
        "Should have a report for test_script"
    );
    assert_eq!(
        script_report.unwrap().status,
        TargetActionReportStatus::Skipped,
        "Non-stow symlink should be skipped"
    );
}

/// Test restow mode functionality
#[test]
fn test_restow_mode_basic() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_restow_pkg";
    let package_dir = create_test_package(&stow_dir, package_name);

    // First, stow the package
    let stow_config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );

    let stow_result = stow_packages(&stow_config);
    assert!(
        stow_result.is_ok(),
        "Initial stow failed: {:?}",
        stow_result.err()
    );

    // Verify initial symlinks
    assert!(
        target_dir.join("bin/test_script").exists(),
        "test_script should exist after initial stow"
    );

    // Modify the package (add a new file)
    fs::write(package_dir.join("bin/new_script"), "#!/bin/bash\necho New").unwrap();

    // Remove an old file to test cleanup
    fs::remove_file(package_dir.join("bin/test_script")).unwrap();

    // Debug: Check package directory contents after modification
    println!("Files in package_dir/bin after modification:");
    if let Ok(entries) = fs::read_dir(package_dir.join("bin")) {
        for entry in entries {
            if let Ok(entry) = entry {
                println!("  {:?}", entry.path());
            }
        }
    }

    // Now test restow mode
    let mut restow_config = stow_config.clone();
    restow_config.mode = StowMode::Restow;
    restow_config.verbosity = 2; // Enable debug output

    let restow_result = restow_packages(&restow_config);
    assert!(
        restow_result.is_ok(),
        "Restow operation failed: {:?}",
        restow_result.err()
    );

    // Debug: Print restow reports
    let restow_reports = restow_result.unwrap();
    println!("Restow reports:");
    for (i, report) in restow_reports.iter().enumerate() {
        println!("  Report {}: {:?}", i, report);
    }

    // Debug: Check what files exist in target directory
    println!("Files in target_dir after restow:");
    if let Ok(entries) = fs::read_dir(&target_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                println!("  {:?}", entry.path());
            }
        }
    }
    if let Ok(entries) = fs::read_dir(target_dir.join("bin")) {
        println!("Files in target_dir/bin after restow:");
        for entry in entries {
            if let Ok(entry) = entry {
                println!("  {:?}", entry.path());
            }
        }
    }

    // Verify old symlink is removed and new one is created
    assert!(
        !target_dir.join("bin/test_script").exists(),
        "Old test_script should be removed after restow"
    );
    assert!(
        target_dir.join("bin/new_script").exists(),
        "New new_script should exist after restow"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/new_script"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "new_script should be a symlink"
    );
}

/// Test restow mode with dotfiles
#[test]
fn test_restow_mode_with_dotfiles() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_restow_dotfiles_pkg";
    let package_dir = create_test_package(&stow_dir, package_name);

    // First, stow with dotfiles
    let stow_config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
        0,
    );

    let stow_result = stow_packages(&stow_config);
    assert!(
        stow_result.is_ok(),
        "Initial stow with dotfiles failed: {:?}",
        stow_result.err()
    );

    // Verify initial dotfiles
    assert!(
        target_dir.join(".bashrc").exists(),
        ".bashrc should exist after initial stow"
    );

    // Modify the package (add a new dotfile)
    fs::write(package_dir.join("dot-vimrc"), "\" Test vimrc").unwrap();

    // Remove an old dotfile
    fs::remove_file(package_dir.join("dot-bashrc")).unwrap();

    // Now test restow mode with dotfiles
    let mut restow_config = stow_config.clone();
    restow_config.mode = StowMode::Restow;

    let restow_result = restow_packages(&restow_config);
    assert!(
        restow_result.is_ok(),
        "Restow operation with dotfiles failed: {:?}",
        restow_result.err()
    );

    // Verify old dotfile is removed and new one is created
    assert!(
        !target_dir.join(".bashrc").exists(),
        "Old .bashrc should be removed after restow"
    );
    assert!(
        target_dir.join(".vimrc").exists(),
        "New .vimrc should exist after restow"
    );
    assert!(
        fs::symlink_metadata(target_dir.join(".vimrc"))
            .unwrap()
            .file_type()
            .is_symlink(),
        ".vimrc should be a symlink"
    );
}

/// Test simulate mode with delete operations
#[test]
fn test_delete_mode_simulate() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_delete_simulate_pkg";
    create_test_package(&stow_dir, package_name);

    // First, stow the package
    let stow_config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );

    let stow_result = stow_packages(&stow_config);
    assert!(
        stow_result.is_ok(),
        "Initial stow failed: {:?}",
        stow_result.err()
    );

    // Verify symlinks exist
    assert!(
        target_dir.join("bin/test_script").exists(),
        "test_script should exist before simulate delete"
    );

    // Test delete in simulate mode
    let delete_config: Config = Config {
        stow_dir,
        target_dir: target_dir.clone(),
        packages: vec![package_name.to_string()],
        mode: StowMode::Delete,
        stow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        overrides: Vec::new(),
        defers: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: true, // Simulate mode
        verbosity: 0,
        home_dir: std::env::temp_dir(),
    };

    let delete_result = delete_packages(&delete_config);
    assert!(
        delete_result.is_ok(),
        "Simulate delete operation failed: {:?}",
        delete_result.err()
    );

    let delete_reports = delete_result.unwrap();

    // Verify symlinks still exist (not actually deleted)
    assert!(
        target_dir.join("bin/test_script").exists(),
        "test_script should still exist after simulate delete"
    );

    // Verify reports indicate simulation
    for report in &delete_reports {
        assert_eq!(
            report.status,
            TargetActionReportStatus::Skipped,
            "All operations should be skipped in simulate mode"
        );
        if let Some(message) = &report.message {
            assert!(
                message.contains("SIMULATE"),
                "Simulate messages should contain 'SIMULATE'"
            );
        }
    }
}

/// Test end-to-end CLI integration with different modes
#[test]
fn test_cli_integration_modes() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name: &str = "test_cli_pkg";
    create_test_package(&stow_dir, package_name);

    // Test stow mode via CLI args
    let stow_args = Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbose: 0,
        packages: vec![package_name.to_string()],
    };

    let stow_config = Config::from_args(stow_args).unwrap();
    assert_eq!(stow_config.mode, StowMode::Stow, "Mode should be Stow");

    let stow_result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbose: 0,
        packages: vec![package_name.to_string()],
    });
    assert!(
        stow_result.is_ok(),
        "CLI stow operation failed: {:?}",
        stow_result.err()
    );
    assert!(
        target_dir.join("bin/test_script").exists(),
        "Symlink should be created via CLI"
    );

    // Test delete mode via CLI args
    let delete_result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: true, // Delete mode
        restow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbose: 0,
        packages: vec![package_name.to_string()],
    });
    assert!(
        delete_result.is_ok(),
        "CLI delete operation failed: {:?}",
        delete_result.err()
    );
    assert!(
        !target_dir.join("bin/test_script").exists(),
        "Symlink should be removed via CLI delete"
    );

    // Test restow mode via CLI args
    let restow_result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: true, // Restow mode
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbose: 0,
        packages: vec![package_name.to_string()],
    });
    assert!(
        restow_result.is_ok(),
        "CLI restow operation failed: {:?}",
        restow_result.err()
    );
    assert!(
        target_dir.join("bin/test_script").exists(),
        "Symlink should be recreated via CLI restow"
    );
}

#[test]
fn test_conflict_resolution_override_option() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();

    // Setup two packages that conflict on the same file
    let package1_dir = stow_dir.join("package1");
    let package2_dir = stow_dir.join("package2");
    fs::create_dir_all(&package1_dir).unwrap();
    fs::create_dir_all(&package2_dir).unwrap();

    // Both packages have a file with the same target path
    fs::write(
        package1_dir.join("conflicting_file.txt"),
        "content from package1",
    )
    .unwrap();
    fs::write(
        package2_dir.join("conflicting_file.txt"),
        "content from package2",
    )
    .unwrap();

    // First, stow package1 to establish the initial symlink
    let config1 = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["package1".to_string()],
        false,
        0,
    );
    let result1 = stow_packages(&config1);
    assert!(
        result1.is_ok(),
        "Failed to stow package1: {:?}",
        result1.err()
    );

    // Verify package1's symlink was created
    let target_file = target_dir.join("conflicting_file.txt");
    assert!(
        target_file.exists(),
        "Target file should exist after stowing package1"
    );
    assert!(
        fs::symlink_metadata(&target_file)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target should be a symlink"
    );

    // Now try to stow package2 without --override (should conflict)
    let config2_no_override = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["package2".to_string()],
        false,
        0,
    );
    let result2_no_override = stow_packages(&config2_no_override);
    assert!(
        result2_no_override.is_ok(),
        "stow_packages should succeed but report conflicts"
    );

    let reports2_no_override = result2_no_override.unwrap();
    let conflict_report = reports2_no_override
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == "conflicting_file.txt")
        })
        .expect("Should find report for conflicting_file.txt");

    assert_eq!(
        conflict_report.original_action.action_type,
        ActionType::Conflict,
        "Should be marked as conflict without --override"
    );

    // Now try with --override option
    let mut config2_with_override = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["package2".to_string()],
        false,
        0,
    );
    // Add override pattern that matches the conflicting file
    config2_with_override.overrides = vec![regex::Regex::new("conflicting_file\\.txt").unwrap()];

    let result2_with_override = stow_packages(&config2_with_override);
    assert!(
        result2_with_override.is_ok(),
        "stow_packages should succeed with --override: {:?}",
        result2_with_override.err()
    );

    let reports2_with_override = result2_with_override.unwrap();
    let override_report = reports2_with_override
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == "conflicting_file.txt")
        })
        .expect("Should find report for conflicting_file.txt with override");

    assert_eq!(
        override_report.original_action.action_type,
        ActionType::CreateSymlink,
        "Should be CreateSymlink with --override"
    );
    assert_eq!(
        override_report.status,
        TargetActionReportStatus::Success,
        "Should succeed with --override"
    );

    // Verify the symlink now points to package2
    let link_target = fs::read_link(&target_file).unwrap();
    assert!(
        link_target.to_string_lossy().contains("package2"),
        "Symlink should now point to package2, but points to: {:?}",
        link_target
    );
}

#[test]
fn test_conflict_resolution_defer_option() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();

    // Setup two packages that conflict on the same file
    let package1_dir = stow_dir.join("package1");
    let package2_dir = stow_dir.join("package2");
    fs::create_dir_all(&package1_dir).unwrap();
    fs::create_dir_all(&package2_dir).unwrap();

    // Both packages have a file with the same target path
    fs::write(
        package1_dir.join("deferred_file.txt"),
        "content from package1",
    )
    .unwrap();
    fs::write(
        package2_dir.join("deferred_file.txt"),
        "content from package2",
    )
    .unwrap();

    // First, stow package1 to establish the initial symlink
    let config1 = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["package1".to_string()],
        false,
        0,
    );
    let result1 = stow_packages(&config1);
    assert!(
        result1.is_ok(),
        "Failed to stow package1: {:?}",
        result1.err()
    );

    // Verify package1's symlink was created
    let target_file = target_dir.join("deferred_file.txt");
    assert!(
        target_file.exists(),
        "Target file should exist after stowing package1"
    );
    let original_link_target = fs::read_link(&target_file).unwrap();

    // Now try to stow package2 with --defer option
    let mut config2_with_defer = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["package2".to_string()],
        false,
        0,
    );
    // Add defer pattern that matches the conflicting file
    config2_with_defer.defers = vec![regex::Regex::new("deferred_file\\.txt").unwrap()];

    let result2_with_defer = stow_packages(&config2_with_defer);
    assert!(
        result2_with_defer.is_ok(),
        "stow_packages should succeed with --defer: {:?}",
        result2_with_defer.err()
    );

    let reports2_with_defer = result2_with_defer.unwrap();
    let defer_report = reports2_with_defer
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == "deferred_file.txt")
        })
        .expect("Should find report for deferred_file.txt with defer");

    assert_eq!(
        defer_report.original_action.action_type,
        ActionType::Skip,
        "Should be Skip with --defer"
    );
    assert_eq!(
        defer_report.status,
        TargetActionReportStatus::Skipped,
        "Should be skipped with --defer"
    );

    // Verify the symlink still points to package1 (unchanged)
    let current_link_target = fs::read_link(&target_file).unwrap();
    assert_eq!(
        current_link_target, original_link_target,
        "Symlink should remain unchanged with --defer"
    );
    assert!(
        current_link_target.to_string_lossy().contains("package1"),
        "Symlink should still point to package1, but points to: {:?}",
        current_link_target
    );
}

#[test]
fn test_conflict_resolution_pattern_matching() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();

    let package_dir = stow_dir.join("test_package");
    fs::create_dir_all(&package_dir).unwrap();

    // Create multiple files, some matching patterns, some not
    fs::write(package_dir.join("override_me.txt"), "override content").unwrap();
    fs::write(package_dir.join("defer_me.txt"), "defer content").unwrap();
    fs::write(package_dir.join("normal_file.txt"), "normal content").unwrap();

    // Create existing files in target to cause conflicts
    fs::write(target_dir.join("override_me.txt"), "existing override").unwrap();
    fs::write(target_dir.join("defer_me.txt"), "existing defer").unwrap();
    fs::write(target_dir.join("normal_file.txt"), "existing normal").unwrap();

    // Configure with specific override and defer patterns
    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["test_package".to_string()],
        false,
        0,
    );
    config.overrides = vec![regex::Regex::new("override_.*\\.txt").unwrap()];
    config.defers = vec![regex::Regex::new("defer_.*\\.txt").unwrap()];

    let result = stow_packages(&config);
    assert!(
        result.is_ok(),
        "stow_packages should succeed: {:?}",
        result.err()
    );

    let reports = result.unwrap();

    // Check override_me.txt - should be CreateSymlink (overridden)
    let override_report = reports
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == "override_me.txt")
        })
        .expect("Should find report for override_me.txt");
    assert_eq!(
        override_report.original_action.action_type,
        ActionType::CreateSymlink,
        "override_me.txt should be CreateSymlink due to --override pattern"
    );

    // Check defer_me.txt - should be Skip (deferred)
    let defer_report = reports
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == "defer_me.txt")
        })
        .expect("Should find report for defer_me.txt");
    assert_eq!(
        defer_report.original_action.action_type,
        ActionType::Skip,
        "defer_me.txt should be Skip due to --defer pattern"
    );

    // Check normal_file.txt - should be Conflict (no pattern matches)
    let normal_report = reports
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .map_or(false, |name| name == "normal_file.txt")
        })
        .expect("Should find report for normal_file.txt");
    assert_eq!(
        normal_report.original_action.action_type,
        ActionType::Conflict,
        "normal_file.txt should be Conflict (no pattern matches)"
    );
}

#[test]
fn test_adopt_option_with_existing_file() {
    let temp_base = tempdir().unwrap();
    let stow_dir = temp_base.path().join("stow");
    let target_dir = temp_base.path().join("target");
    let package_dir = stow_dir.join("testpkg");

    // Create directories
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&package_dir).unwrap();

    // Create a file in the package
    let package_file = package_dir.join("config.txt");
    fs::write(&package_file, "package content").unwrap();

    // Create an existing file in target with different content
    let target_file = target_dir.join("config.txt");
    fs::write(&target_file, "existing content").unwrap();

    // Run rustow with --adopt option
    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: true,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbose: 0,
        packages: vec!["testpkg".to_string()],
    });

    // Should succeed
    assert!(result.is_ok(), "rustow --adopt should succeed");

    // Verify the existing file was moved to package directory
    // (package file should now contain the original target content)
    let package_content = fs::read_to_string(&package_file).unwrap();
    assert_eq!(
        package_content, "existing content",
        "Package file should contain adopted content"
    );

    // Verify symlink was created in target
    assert!(target_file.exists(), "Target file should still exist");
    assert!(
        fs::symlink_metadata(&target_file)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target should be a symlink"
    );

    // Verify symlink points to the package file
    let link_target = fs::read_link(&target_file).unwrap();
    assert!(
        link_target
            .to_string_lossy()
            .contains("stow/testpkg/config.txt"),
        "Symlink should point to package file, got: {:?}",
        link_target
    );
}

#[test]
fn test_adopt_option_with_existing_directory() {
    let temp_base = tempdir().unwrap();
    let stow_dir = temp_base.path().join("stow");
    let target_dir = temp_base.path().join("target");
    let package_dir = stow_dir.join("testpkg");

    // Create directories
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&package_dir).unwrap();

    // Create a directory structure in the package
    let package_config_dir = package_dir.join("config");
    fs::create_dir_all(&package_config_dir).unwrap();
    fs::write(package_config_dir.join("app.conf"), "package config").unwrap();

    // Create an existing directory in target with different content
    let target_config_dir = target_dir.join("config");
    fs::create_dir_all(&target_config_dir).unwrap();
    fs::write(target_config_dir.join("existing.conf"), "existing config").unwrap();

    // Run rustow with --adopt option
    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: true,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: false,
        verbose: 0,
        packages: vec!["testpkg".to_string()],
    });

    // Should succeed
    assert!(
        result.is_ok(),
        "rustow --adopt should succeed with directory"
    );

    // Verify the existing directory was moved to package directory
    let adopted_file = package_config_dir.join("existing.conf");
    assert!(
        adopted_file.exists(),
        "Existing file should be moved to package directory"
    );
    assert_eq!(
        fs::read_to_string(&adopted_file).unwrap(),
        "existing config"
    );

    // Verify symlink was created in target
    assert!(
        target_config_dir.exists(),
        "Target directory should still exist"
    );
    assert!(
        fs::symlink_metadata(&target_config_dir)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target should be a symlink"
    );

    // Verify symlink points to the package directory
    let link_target = fs::read_link(&target_config_dir).unwrap();
    assert!(
        link_target
            .to_string_lossy()
            .contains("stow/testpkg/config"),
        "Symlink should point to package directory, got: {:?}",
        link_target
    );
}

#[test]
fn test_adopt_option_simulation_mode() {
    let temp_base = tempdir().unwrap();
    let stow_dir = temp_base.path().join("stow");
    let target_dir = temp_base.path().join("target");
    let package_dir = stow_dir.join("testpkg");

    // Create directories
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&package_dir).unwrap();

    // Create a file in the package
    let package_file = package_dir.join("test.txt");
    fs::write(&package_file, "package content").unwrap();

    // Create an existing file in target
    let target_file = target_dir.join("test.txt");
    fs::write(&target_file, "existing content").unwrap();

    // Run rustow with --adopt and --simulate
    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: true,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        simulate: true,
        verbose: 1,
        packages: vec!["testpkg".to_string()],
    });

    // Should succeed
    assert!(result.is_ok(), "rustow --adopt --simulate should succeed");

    // In simulation mode, files should NOT be modified
    let package_content = fs::read_to_string(&package_file).unwrap();
    assert_eq!(
        package_content, "package content",
        "Package file should be unchanged in simulation"
    );

    let target_content = fs::read_to_string(&target_file).unwrap();
    assert_eq!(
        target_content, "existing content",
        "Target file should be unchanged in simulation"
    );

    // Target should still be a regular file, not a symlink
    assert!(
        !fs::symlink_metadata(&target_file)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target should remain a regular file in simulation mode"
    );
}
