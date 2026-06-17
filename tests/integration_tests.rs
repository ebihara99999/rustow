use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rustow::cli::Args;
use rustow::config::{Config, StowMode};
use rustow::stow::{
    ActionType, StowItemType, TargetActionReportStatus, delete_packages, restow_packages,
    stow_packages,
};
use tempfile::{TempDir, tempdir};

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
        compat: false,
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

fn run_rustow<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let temp_dir = tempdir().expect("Failed to create isolated rustow run temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    fs::create_dir_all(&home_dir).expect("Failed to create isolated rustow HOME");
    fs::create_dir_all(&cwd).expect("Failed to create isolated rustow cwd");

    Command::new(env!("CARGO_BIN_EXE_rustow"))
        .args(args)
        .current_dir(cwd)
        .env("HOME", home_dir)
        .env_remove("STOW_DIR")
        .output()
        .expect("Failed to run rustow binary")
}

fn run_rustow_with<I, S>(args: I, cwd: &Path, envs: &[(&str, &str)]) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_rustow"))
        .args(args)
        .current_dir(cwd)
        .env_remove("STOW_DIR")
        .envs(envs.iter().copied())
        .output()
        .expect("Failed to run rustow binary")
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
            .is_some_and(|name| name == "dot-bashrc")
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
            .is_some_and(|name| name == "dot-config")
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
            .is_some_and(|name| name == "README.md")
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
            .is_some_and(|name| name == "LICENSE")
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

    let mut config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
        0,
    );
    config.no_folding = true;

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
            .is_some_and(|name| name == ".bashrc")
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
            .is_some_and(|name| name == ".config")
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

    let mut config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        true, // dotfiles enabled
        0,
    );
    config.no_folding = true;

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
            .is_some_and(|item| item.source_path.to_string_lossy().contains(package1_name))
            && report.original_action.target_path.ends_with(".bashrc")
    });
    assert!(p1_bashrc_exists, "Expected .bashrc from package1");

    let p2_bashrc_exists: bool = actions.iter().any(|report| {
        report
            .original_action
            .source_item
            .as_ref()
            .is_some_and(|item| item.source_path.to_string_lossy().contains(package2_name))
            && report.original_action.target_path.ends_with(".bashrc")
    });
    assert!(p2_bashrc_exists, "Expected .bashrc from package2");
}

#[test]
fn test_default_tree_folding_creates_directory_symlink() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package_name = "fold_pkg";
    let package_dir = stow_dir.join(package_name);
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "#!/bin/sh\n").unwrap();

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );

    let reports = stow_packages(&config).unwrap();

    let bin_report = reports
        .iter()
        .find(|report| report.original_action.target_path == target_dir.join("bin"))
        .expect("bin folding report should exist");
    assert_eq!(
        bin_report.original_action.action_type,
        ActionType::CreateSymlink
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "bin should be a folded symlink"
    );
    assert!(target_dir.join("bin/tool").exists());
    assert!(
        reports
            .iter()
            .all(|report| report.original_action.target_path != target_dir.join("bin/tool")),
        "folded descendants should not be linked individually"
    );
}

#[test]
fn test_no_folding_keeps_directory_open() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_name = "no_fold_pkg";
    let package_dir = stow_dir.join(package_name);
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "#!/bin/sh\n").unwrap();

    let folded_target_dir = target_dir.join("folded");
    let open_target_dir = target_dir.join("open");
    fs::create_dir_all(&folded_target_dir).unwrap();
    fs::create_dir_all(&open_target_dir).unwrap();

    let folded_config = create_test_config(
        stow_dir.clone(),
        folded_target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );
    let folded_reports = stow_packages(&folded_config).unwrap();
    assert!(
        folded_reports
            .iter()
            .any(|report| report.original_action.target_path == folded_target_dir.join("bin")),
        "Expected an action for folded target path"
    );
    assert!(
        fs::symlink_metadata(folded_target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "bin should be folded to symlink with default options"
    );

    let mut open_config = create_test_config(
        stow_dir.clone(),
        open_target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );
    open_config.no_folding = true;
    let open_reports = stow_packages(&open_config).unwrap();
    assert!(
        open_reports
            .iter()
            .any(|report| report.original_action.target_path == open_target_dir.join("bin")),
        "Expected an action for open target path"
    );
    assert!(
        open_target_dir.join("bin").is_dir(),
        "bin should remain a directory"
    );
    assert!(
        !fs::symlink_metadata(open_target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "bin should not be a symlink when --no-folding is enabled"
    );

    assert!(
        fs::symlink_metadata(open_target_dir.join("bin/tool"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "Nested target should still be symlinked"
    );
}

#[test]
fn test_split_open_folded_tree_for_second_package() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package1_dir = stow_dir.join("perl");
    let package2_dir = stow_dir.join("emacs");
    fs::create_dir_all(package1_dir.join("bin")).unwrap();
    fs::create_dir_all(package2_dir.join("bin")).unwrap();
    fs::write(package1_dir.join("bin/perl"), "perl").unwrap();
    fs::write(package2_dir.join("bin/emacs"), "emacs").unwrap();

    let package1_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["perl".to_string()],
        false,
        0,
    );
    stow_packages(&package1_config).unwrap();
    assert!(
        fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "first package should fold bin"
    );

    let package2_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["emacs".to_string()],
        false,
        0,
    );
    let reports = stow_packages(&package2_config).unwrap();

    assert!(
        reports.iter().any(|report| {
            report.original_action.target_path == target_dir.join("bin")
                && report.original_action.action_type == ActionType::DeleteSymlink
        }),
        "split-open should delete the old folded bin symlink"
    );
    assert!(
        target_dir.join("bin").is_dir()
            && !fs::symlink_metadata(target_dir.join("bin"))
                .unwrap()
                .file_type()
                .is_symlink(),
        "bin should become a real directory after split-open"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/perl"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "old package entry should be relinked"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/emacs"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "new package entry should be linked"
    );
}

#[test]
fn test_no_folding_split_open_with_existing_folded_tree() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package1_dir = stow_dir.join("perl");
    let package2_dir = stow_dir.join("emacs");
    fs::create_dir_all(package1_dir.join("bin")).unwrap();
    fs::create_dir_all(package2_dir.join("bin")).unwrap();
    fs::write(package1_dir.join("bin/perl"), "perl").unwrap();
    fs::write(package2_dir.join("bin/emacs"), "emacs").unwrap();

    let package1_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["perl".to_string()],
        false,
        0,
    );
    stow_packages(&package1_config).unwrap();
    assert!(
        fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "first package should fold bin"
    );

    let mut package2_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["emacs".to_string()],
        false,
        0,
    );
    package2_config.no_folding = true;
    let reports = stow_packages(&package2_config).unwrap();

    assert!(
        reports.iter().any(|report| {
            report.original_action.target_path == target_dir.join("bin")
                && report.original_action.action_type == ActionType::DeleteSymlink
        }),
        "split-open should delete the old folded bin symlink"
    );
    assert!(
        target_dir.join("bin").is_dir()
            && !fs::symlink_metadata(target_dir.join("bin"))
                .unwrap()
                .file_type()
                .is_symlink(),
        "bin should become a real directory after split-open"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/perl"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "old package entry should be relinked"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/emacs"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "new package entry should be linked"
    );
}

#[test]
fn test_no_folding_split_open_preserves_package_alias_target() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let real_package_dir = stow_dir.join("realpkg");
    let alias_package_dir = stow_dir.join("aliaspkg");
    let second_package_dir = stow_dir.join("second");

    fs::create_dir_all(real_package_dir.join("bin")).unwrap();
    fs::create_dir_all(second_package_dir.join("bin")).unwrap();
    fs::write(real_package_dir.join("bin/tool"), "tool").unwrap();
    fs::write(second_package_dir.join("bin/config"), "config").unwrap();

    rustow::fs_utils::create_symlink(&alias_package_dir, Path::new("realpkg")).unwrap();

    let alias_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["aliaspkg".to_string()],
        false,
        0,
    );
    stow_packages(&alias_config).unwrap();
    assert!(
        fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "alias package first stow should fold bin"
    );

    let mut split_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["second".to_string()],
        false,
        0,
    );
    split_config.no_folding = true;
    let reports = stow_packages(&split_config).unwrap();

    assert!(
        reports.iter().any(|report| {
            report.original_action.target_path == target_dir.join("bin")
                && report.original_action.action_type == ActionType::DeleteSymlink
        }),
        "split-open should delete folded bin symlink"
    );
    assert!(
        target_dir.join("bin").is_dir(),
        "bin should become a real directory after split-open"
    );
    assert!(
        !fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "bin should be directory with --no-folding"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/tool"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "existing alias-managed file should keep symlink target"
    );
    assert!(
        fs::read_link(target_dir.join("bin/tool"))
            .unwrap()
            .ends_with("aliaspkg/bin/tool"),
        "alias symlink target should remain package alias"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/config"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "new package file should be linked"
    );
}

#[test]
fn test_split_open_preserves_package_symlink_alias_targets() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let real_package_dir = stow_dir.join("realpkg");
    let second_package_dir = stow_dir.join("second");
    fs::create_dir_all(real_package_dir.join("bin")).unwrap();
    fs::create_dir_all(second_package_dir.join("bin")).unwrap();
    fs::write(real_package_dir.join("bin/tool"), "real").unwrap();
    fs::write(second_package_dir.join("bin/other"), "other").unwrap();
    rustow::fs_utils::create_symlink(&stow_dir.join("aliaspkg"), Path::new("realpkg")).unwrap();

    let alias_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["aliaspkg".to_string()],
        false,
        0,
    );
    stow_packages(&alias_config).unwrap();
    assert_eq!(
        fs::read_link(target_dir.join("bin")).unwrap(),
        PathBuf::from("../stow_dir/aliaspkg/bin")
    );

    let second_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["second".to_string()],
        false,
        0,
    );
    let result = stow_packages(&second_config);

    assert!(result.is_ok(), "split-open failed: {:?}", result.err());
    assert!(target_dir.join("bin").is_dir());
    assert_eq!(
        fs::read_link(target_dir.join("bin/tool")).unwrap(),
        PathBuf::from("../../stow_dir/aliaspkg/bin/tool")
    );
    assert!(target_dir.join("bin/other").exists());
}

#[test]
fn test_delete_refolds_single_remaining_package_tree() {
    let (_temp_dir, stow_dir, target_dir) = setup_test_environment();
    let package1_dir = stow_dir.join("perl");
    let package2_dir = stow_dir.join("emacs");
    fs::create_dir_all(package1_dir.join("bin")).unwrap();
    fs::create_dir_all(package2_dir.join("bin")).unwrap();
    fs::write(package1_dir.join("bin/perl"), "perl").unwrap();
    fs::write(package2_dir.join("bin/emacs"), "emacs").unwrap();

    stow_packages(&create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["perl".to_string()],
        false,
        0,
    ))
    .unwrap();
    stow_packages(&create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["emacs".to_string()],
        false,
        0,
    ))
    .unwrap();

    let mut delete_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["emacs".to_string()],
        false,
        0,
    );
    delete_config.mode = StowMode::Delete;
    delete_packages(&delete_config).unwrap();

    assert!(
        fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "bin should refold to the remaining package"
    );
    assert!(target_dir.join("bin/perl").exists());
    assert!(!target_dir.join("bin/emacs").exists());
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
    fs::write(package_dir_4.join("nodotprefix-file"), "content").unwrap();

    // Test case 5: directory NOT starting with "dot-", containing a file
    let package_dir_5: PathBuf = stow_dir.join("package5");
    fs::create_dir_all(&package_dir_5).unwrap();
    let nested_dir_5: PathBuf = package_dir_5.join("nodotprefix");
    fs::create_dir_all(&nested_dir_5).unwrap();
    fs::write(nested_dir_5.join("file.txt"), "content").unwrap();

    let mut config: Config = create_test_config(
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
    config.no_folding = true;

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
            r.original_action.source_item.as_ref().is_some_and(|item| {
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
            r.original_action.source_item.as_ref().is_some_and(|item| {
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
            r.original_action.source_item.as_ref().is_some_and(|item| {
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
        r.original_action.source_item.as_ref().is_some_and(|item| {
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

    // Verify package4: "nodotprefix-file" -> "nodotprefix-file"
    let report_pkg4_nodotprefix_file: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action.source_item.as_ref().is_some_and(|item| {
                item.package_relative_path == Path::new("nodotprefix-file")
                    && item.target_name_after_dotfiles_processing == Path::new("nodotprefix-file")
                    && item.item_type == StowItemType::File
            })
        })
        .expect("Report for package4/nodotprefix-file (target: nodotprefix-file) not found");

    assert_eq!(
        report_pkg4_nodotprefix_file.status,
        TargetActionReportStatus::Success,
        "Expected package4/nodotprefix-file processing to be Success, but got {:?}. Message: {:?}",
        report_pkg4_nodotprefix_file.status,
        report_pkg4_nodotprefix_file.message
    );
    assert_eq!(
        report_pkg4_nodotprefix_file.original_action.action_type,
        ActionType::CreateSymlink,
        "ActionType for package4/nodotprefix-file should be CreateSymlink"
    );
    let expected_target_pkg4_nodotprefix_file: PathBuf = target_dir.join("nodotprefix-file");
    assert!(
        expected_target_pkg4_nodotprefix_file.exists(),
        "Target nodotprefix-file for package4 was not created. Report details: {:?}",
        report_pkg4_nodotprefix_file
    );
    assert!(
        fs::symlink_metadata(&expected_target_pkg4_nodotprefix_file)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target nodotprefix-file for package4 is not a symlink. Report details: {:?}",
        report_pkg4_nodotprefix_file
    );

    // Verify package5: "nodotprefix/file.txt" -> "nodotprefix/file.txt"
    // First, verify the parent directory "nodotprefix" for package5
    let report_pkg5_nodotprefix_dir: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action.source_item.as_ref().is_some_and(|item| {
                item.package_relative_path == Path::new("nodotprefix")
                    && item.target_name_after_dotfiles_processing == Path::new("nodotprefix")
                    && item.item_type == StowItemType::Directory
            })
        })
        .expect("Report for package5/nodotprefix (directory) not found");

    assert_eq!(
        report_pkg5_nodotprefix_dir.status,
        TargetActionReportStatus::Success,
        "Expected package5/nodotprefix (directory) processing to be Success, but got {:?}. Message: {:?}",
        report_pkg5_nodotprefix_dir.status,
        report_pkg5_nodotprefix_dir.message
    );
    assert_eq!(
        report_pkg5_nodotprefix_dir.original_action.action_type,
        ActionType::CreateDirectory,
        "ActionType for package5/nodotprefix (directory) should be CreateDirectory"
    );
    let expected_target_pkg5_nodotprefix_dir: PathBuf = target_dir.join("nodotprefix");
    assert!(
        expected_target_pkg5_nodotprefix_dir.exists(),
        "Target nodotprefix for package5 was not created. Report details: {:?}",
        report_pkg5_nodotprefix_dir
    );
    assert!(
        expected_target_pkg5_nodotprefix_dir.is_dir(),
        "Target nodotprefix for package5 is not a directory. Report details: {:?}",
        report_pkg5_nodotprefix_dir
    );

    // Next, verify the nested file "nodotprefix/file.txt" for package5
    let report_pkg5_nested_file: &rustow::stow::TargetActionReport = actions
        .iter()
        .find(|r| {
            r.original_action.source_item.as_ref().is_some_and(|item| {
                item.package_relative_path == Path::new("nodotprefix/file.txt")
                    && item.target_name_after_dotfiles_processing
                        == Path::new("nodotprefix/file.txt")
            })
        })
        .expect("Report for package5/nodotprefix/file.txt not found");

    assert_eq!(
        report_pkg5_nested_file.status,
        TargetActionReportStatus::Success,
        "Expected package5/nodotprefix/file.txt processing to be Success, but got {:?}. Message: {:?}",
        report_pkg5_nested_file.status,
        report_pkg5_nested_file.message
    );
    assert_eq!(
        report_pkg5_nested_file.original_action.action_type,
        ActionType::CreateSymlink,
        "ActionType for package5/nodotprefix/file.txt should be CreateSymlink"
    );
    let expected_target_pkg5_nested_file: PathBuf = target_dir.join("nodotprefix/file.txt");
    assert!(
        expected_target_pkg5_nested_file.exists(),
        "Target nodotprefix/file.txt for package5 was not created. Report details: {:?}",
        report_pkg5_nested_file
    );
    assert!(
        fs::symlink_metadata(&expected_target_pkg5_nested_file)
            .unwrap()
            .file_type()
            .is_symlink(),
        "Target nodotprefix/file.txt for package5 is not a symlink. Report details: {:?}",
        report_pkg5_nested_file
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
        compat: false,
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

    let mut config_empty_target: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );
    config_empty_target.no_folding = true;

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
                .is_some_and(|name| name == OsStr::new("file_to_link.txt"))
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
                .is_some_and(|name| name == OsStr::new("dir_to_create"))
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
                .is_some_and(|name| name == OsStr::new("file_for_conflict.txt"))
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
                .is_some_and(|name| name == OsStr::new("dir_for_conflict"))
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
    config1.no_folding = true;

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

    let mut config3: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![pkg3_name.to_string()],
        false,
        0,
    );
    config3.no_folding = true;
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
    config3_sim.no_folding = true;
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

    // Scenario 4: --no-folding compatibility differences are covered by
    // test_no_folding_keeps_directory_open.
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
    let mut stow_config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );
    stow_config.no_folding = true;

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
        compat: false,
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
        compat: false,
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
    let mut stow_config: Config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec![package_name.to_string()],
        false,
        0,
    );
    stow_config.no_folding = true;

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
        for entry in entries.flatten() {
            println!("  {:?}", entry.path());
        }
    }

    // Now test restow mode
    let mut restow_config = stow_config.clone();
    restow_config.mode = StowMode::Restow;
    restow_config.compat = true;
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
        for entry in entries.flatten() {
            println!("  {:?}", entry.path());
        }
    }
    if let Ok(entries) = fs::read_dir(target_dir.join("bin")) {
        println!("Files in target_dir/bin after restow:");
        for entry in entries.flatten() {
            println!("  {:?}", entry.path());
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
    restow_config.compat = true;

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
        fs::symlink_metadata(target_dir.join(".bashrc")).is_err(),
        "Old .bashrc symlink should be removed after restow"
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

#[test]
fn test_restow_prunes_obsolete_top_level_symlink() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(package_dir.join("old"), "old").unwrap();
    fs::write(package_dir.join("current"), "current").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    config.compat = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(&target_dir.join("old")));

    fs::remove_file(package_dir.join("old")).unwrap();
    fs::write(package_dir.join("new"), "new").unwrap();

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(fs::symlink_metadata(target_dir.join("old")).is_err());
    assert!(target_dir.join("current").exists());
    assert!(target_dir.join("new").exists());
}

#[test]
fn test_restow_prunes_obsolete_nested_symlink_for_each_shared_directory_package_order() {
    for package_order in [
        vec!["a".to_string(), "b".to_string()],
        vec!["b".to_string(), "a".to_string()],
    ] {
        let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) =
            setup_test_environment();
        let package_a_dir = stow_dir.join("a");
        let package_b_dir = stow_dir.join("b");

        fs::create_dir_all(package_a_dir.join("config")).unwrap();
        fs::create_dir_all(package_b_dir.join("config/nvim")).unwrap();
        fs::write(package_a_dir.join("config/a-current"), "a").unwrap();
        fs::write(package_b_dir.join("config/nvim/current"), "current").unwrap();
        fs::write(package_b_dir.join("config/nvim/old"), "old").unwrap();

        let mut config = create_test_config(
            stow_dir.clone(),
            target_dir.clone(),
            package_order.clone(),
            false,
            0,
        );
        config.no_folding = true;
        stow_packages(&config).unwrap();
        assert!(rustow::fs_utils::is_symlink(
            &target_dir.join("config/nvim/old")
        ));

        fs::remove_file(package_b_dir.join("config/nvim/old")).unwrap();
        fs::write(package_b_dir.join("config/nvim/new"), "new").unwrap();

        let mut restow_config = config.clone();
        restow_config.mode = StowMode::Restow;
        let result = restow_packages(&restow_config);

        assert!(
            result.is_ok(),
            "restow failed for order {:?}: {:?}",
            package_order,
            result.err()
        );
        assert!(
            fs::symlink_metadata(target_dir.join("config/nvim/old")).is_err(),
            "old symlink remained for order {:?}",
            package_order
        );
        assert!(target_dir.join("config/a-current").exists());
        assert!(target_dir.join("config/nvim/current").exists());
        assert!(target_dir.join("config/nvim/new").exists());
    }
}

#[test]
fn test_restow_prunes_obsolete_nested_symlink_when_package_subtree_is_removed() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("config/nvim")).unwrap();
    fs::write(package_dir.join("config/nvim/old"), "old").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    config.compat = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/nvim/old")
    ));

    let unrelated_dir = target_dir.join("unrelated");
    fs::create_dir_all(&unrelated_dir).unwrap();
    rustow::fs_utils::create_symlink(
        &unrelated_dir.join("old"),
        &stow_dir.join("pkg/config/nvim/old"),
    )
    .unwrap();
    fs::remove_dir_all(package_dir.join("config")).unwrap();

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(fs::symlink_metadata(target_dir.join("config/nvim/old")).is_err());
    assert!(rustow::fs_utils::is_symlink(&unrelated_dir.join("old")));
}

#[test]
fn test_restow_preserves_obsolete_nested_symlink_when_package_subtree_is_removed_without_compat() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("config/nvim")).unwrap();
    fs::write(package_dir.join("config/nvim/old"), "old").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/nvim/old")
    ));

    let unrelated_dir = target_dir.join("unrelated");
    fs::create_dir_all(&unrelated_dir).unwrap();
    rustow::fs_utils::create_symlink(
        &unrelated_dir.join("old"),
        &stow_dir.join("pkg/config/nvim/old"),
    )
    .unwrap();
    fs::remove_dir_all(package_dir.join("config")).unwrap();

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/nvim/old")
    ));
    assert!(rustow::fs_utils::is_symlink(&unrelated_dir.join("old")));
}

#[test]
fn test_mixed_restow_prunes_obsolete_nested_symlink_when_package_subtree_is_removed() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let side_dir = stow_dir.join("side");
    fs::create_dir_all(package_dir.join("config/nvim")).unwrap();
    fs::create_dir_all(side_dir.join("share")).unwrap();
    fs::write(package_dir.join("config/nvim/old"), "old").unwrap();
    fs::write(side_dir.join("share/side"), "side").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    config.compat = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/nvim/old")
    ));
    fs::remove_dir_all(package_dir.join("config")).unwrap();

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "--no-folding".to_string(),
        "-p".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-R".to_string(),
        "pkg".to_string(),
        "-S".to_string(),
        "side".to_string(),
    ]);
    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(result.is_ok(), "mixed restow failed: {:?}", result.err());
    assert!(fs::symlink_metadata(target_dir.join("config/nvim/old")).is_err());
    assert!(target_dir.join("share/side").exists());
}

#[test]
fn test_restow_preserves_ignored_obsolete_nested_symlink() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("config")).unwrap();
    fs::write(package_dir.join("config/secret"), "secret").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/secret")
    ));
    fs::remove_dir_all(package_dir.join("config")).unwrap();

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    restow_config.ignore_patterns = vec![regex::Regex::new("secret").unwrap()];
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/secret")
    ));
}

#[test]
fn test_restow_preserves_removed_package_top_level_symlink_when_not_compat() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(package_dir.join("old"), "old").unwrap();
    fs::write(package_dir.join("current"), "current").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    stow_packages(&config).unwrap();

    fs::remove_file(package_dir.join("old")).unwrap();
    fs::write(package_dir.join("new"), "new").unwrap();

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(fs::symlink_metadata(target_dir.join("old")).is_ok());
    assert!(target_dir.join("current").exists());
    assert!(target_dir.join("new").exists());
}

#[test]
fn test_mixed_restow_preserves_ignored_obsolete_nested_symlink() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let side_dir = stow_dir.join("side");
    fs::create_dir_all(package_dir.join("config")).unwrap();
    fs::create_dir_all(side_dir.join("share")).unwrap();
    fs::write(package_dir.join("config/secret"), "secret").unwrap();
    fs::write(side_dir.join("share/side"), "side").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/secret")
    ));
    fs::remove_dir_all(package_dir.join("config")).unwrap();

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "--no-folding".to_string(),
        "--ignore".to_string(),
        "secret".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-R".to_string(),
        "pkg".to_string(),
        "-S".to_string(),
        "side".to_string(),
    ]);
    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(result.is_ok(), "mixed restow failed: {:?}", result.err());
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("config/secret")
    ));
    assert!(target_dir.join("share/side").exists());
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
        compat: false,
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
        compat: false,
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
        compat: false,
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
        compat: false,
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
        compat: false,
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
fn test_cli_mixed_delete_and_stow_operations() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(new_package_dir.join("bin")).unwrap();
    fs::write(old_package_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_package_dir.join("bin/new_tool"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("bin/old_tool").exists());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "newpkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(result.is_ok(), "mixed operation failed: {:?}", result.err());
    assert!(
        !target_dir.join("bin/old_tool").exists(),
        "old package should be unstowed"
    );
    assert!(
        target_dir.join("bin/new_tool").exists(),
        "new package should be stowed"
    );
}

#[test]
fn test_public_run_rejects_ambiguous_mixed_args_without_mutation() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(new_package_dir.join("bin")).unwrap();
    fs::write(old_package_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_package_dir.join("bin/new_tool"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();

    let args = Args::parse_from([
        "rustow",
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "-D",
        "oldpkg",
        "-S",
        "newpkg",
    ]);
    let result = rustow::run(args);

    assert!(matches!(
        result,
        Err(rustow::error::RustowError::Config(
            rustow::error::ConfigError::InvalidOperation(_)
        ))
    ));
    assert!(target_dir.join("bin/old_tool").exists());
    assert!(!target_dir.join("bin/new_tool").exists());
}

#[test]
fn test_public_run_with_empty_operation_groups_rejects_ambiguous_mixed_args_without_mutation() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(new_package_dir.join("bin")).unwrap();
    fs::write(old_package_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_package_dir.join("bin/new_tool"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();

    let args = Args::parse_from([
        "rustow",
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "-D",
        "oldpkg",
        "-S",
        "newpkg",
    ]);
    let result = rustow::run_with_operation_groups(args, Vec::new());

    assert!(matches!(
        result,
        Err(rustow::error::RustowError::Config(
            rustow::error::ConfigError::InvalidOperation(_)
        ))
    ));
    assert!(target_dir.join("bin/old_tool").exists());
    assert!(!target_dir.join("bin/new_tool").exists());
}

#[test]
fn test_cli_mixed_same_package_stow_and_delete_leaves_package_stowed() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/old"), "old").unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(&target_dir.join("bin/old")));

    fs::remove_file(package_dir.join("bin/old")).unwrap();
    fs::write(package_dir.join("bin/new"), "new").unwrap();

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "--no-folding".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-S".to_string(),
        "pkg".to_string(),
        "-D".to_string(),
        "pkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "same-package mixed operation failed: {:?}",
        result.err()
    );
    assert!(fs::symlink_metadata(target_dir.join("bin/old")).is_err());
    assert!(target_dir.join("bin/tool").exists());
    assert!(target_dir.join("bin/new").exists());
}

#[test]
fn test_cli_mixed_duplicate_stow_groups_are_idempotent() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let old_package_dir = stow_dir.join("oldpkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::create_dir_all(old_package_dir.join("share")).unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();
    fs::write(old_package_dir.join("share/old"), "old").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("share/old").exists());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-S".to_string(),
        "pkg".to_string(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "pkg".to_string(),
    ]);
    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "duplicate stow mixed operation failed: {:?}",
        result.err()
    );
    assert!(target_dir.join("bin/tool").exists());
    assert!(!target_dir.join("share/old").exists());
}

#[test]
fn test_binary_mixed_same_package_delete_and_stow_prunes_stale_symlink() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/old"), "old").unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(&target_dir.join("bin/old")));

    fs::remove_file(package_dir.join("bin/old")).unwrap();
    fs::write(package_dir.join("bin/new"), "new").unwrap();

    let output = run_rustow([
        "--no-folding",
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "-D",
        "pkg",
        "-S",
        "pkg",
    ]);

    assert!(
        output.status.success(),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fs::symlink_metadata(target_dir.join("bin/old")).is_err());
    assert!(target_dir.join("bin/tool").exists());
    assert!(target_dir.join("bin/new").exists());
}

#[test]
fn test_binary_no_folding_keeps_directory_open() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();

    let output = run_rustow([
        "--no-folding",
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "pkg",
    ]);

    assert!(
        output.status.success(),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        target_dir.join("bin").is_dir(),
        "bin should remain a directory"
    );
    assert!(
        !fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "bin should not be a symlink when --no-folding is enabled"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/tool"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "bin/tool should be symlinked"
    );
}

#[test]
fn test_binary_no_folding_split_open_with_existing_folded_tree() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let package1_dir = stow_dir.join("perl");
    let package2_dir = stow_dir.join("emacs");
    fs::create_dir_all(package1_dir.join("bin")).unwrap();
    fs::create_dir_all(package2_dir.join("bin")).unwrap();
    fs::write(package1_dir.join("bin/perl"), "perl").unwrap();
    fs::write(package2_dir.join("bin/emacs"), "emacs").unwrap();

    let first = run_rustow([
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "perl",
    ]);
    assert!(
        first.status.success(),
        "initial rustow failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "first package should fold bin"
    );

    let second = run_rustow([
        "--no-folding",
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "emacs",
    ]);
    assert!(
        second.status.success(),
        "second rustow failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    assert!(
        target_dir.join("bin").is_dir()
            && !fs::symlink_metadata(target_dir.join("bin"))
                .unwrap()
                .file_type()
                .is_symlink(),
        "bin should become a real directory after split-open"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/perl"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "old package entry should be relinked"
    );
    assert!(
        fs::symlink_metadata(target_dir.join("bin/emacs"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "new package entry should be linked"
    );
}

#[test]
fn test_binary_mixed_delete_and_stow_operations() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(new_package_dir.join("bin")).unwrap();
    fs::write(old_package_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_package_dir.join("bin/new_tool"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();

    let output = run_rustow([
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "-D",
        "oldpkg",
        "-S",
        "newpkg",
    ]);

    assert!(
        output.status.success(),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!target_dir.join("bin/old_tool").exists());
    assert!(target_dir.join("bin/new_tool").exists());
}

#[test]
fn test_cli_mixed_stow_then_delete_replaces_overlapping_target() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(new_package_dir.join("bin")).unwrap();
    fs::write(old_package_dir.join("bin/tool"), "old").unwrap();
    fs::write(new_package_dir.join("bin/tool"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("bin/tool").exists());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-S".to_string(),
        "newpkg".to_string(),
        "-D".to_string(),
        "oldpkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "GNU mixed stow/delete replacement failed: {:?}",
        result.err()
    );
    assert!(target_dir.join("bin/tool").exists());
    assert_eq!(
        fs::read_link(target_dir.join("bin")).unwrap(),
        PathBuf::from("../stow_dir/newpkg/bin")
    );
}

#[test]
fn test_cli_mixed_stow_conflict_prevents_prior_delete() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(new_package_dir.join("etc")).unwrap();
    fs::write(old_package_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_package_dir.join("etc/app.conf"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    fs::create_dir_all(target_dir.join("etc")).unwrap();
    fs::write(target_dir.join("etc/app.conf"), "unmanaged").unwrap();
    assert!(target_dir.join("bin/old_tool").exists());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "newpkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_err(),
        "mixed operation should fail before mutation when later stow conflicts"
    );
    assert!(
        target_dir.join("bin/old_tool").exists(),
        "old package should remain stowed when later stow phase conflicts"
    );
    assert_eq!(
        fs::read_to_string(target_dir.join("etc/app.conf")).unwrap(),
        "unmanaged"
    );
}

#[test]
fn test_binary_mixed_simulate_conflict_reports_failure_without_mutation() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(new_package_dir.join("etc")).unwrap();
    fs::write(old_package_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_package_dir.join("etc/app.conf"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    fs::create_dir_all(target_dir.join("etc")).unwrap();
    fs::write(target_dir.join("etc/app.conf"), "unmanaged").unwrap();

    let output = run_rustow([
        "-n",
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "-D",
        "oldpkg",
        "-S",
        "newpkg",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("CONFLICT"));
    assert!(target_dir.join("bin/old_tool").exists());
    assert_eq!(
        fs::read_to_string(target_dir.join("etc/app.conf")).unwrap(),
        "unmanaged"
    );
}

#[test]
fn test_cli_restow_conflict_preserves_existing_symlinks() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "old").unwrap();

    let stow_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    stow_packages(&stow_config).unwrap();
    fs::create_dir_all(package_dir.join("etc")).unwrap();
    fs::write(package_dir.join("etc/app.conf"), "new").unwrap();
    fs::create_dir_all(target_dir.join("etc")).unwrap();
    fs::write(target_dir.join("etc/app.conf"), "unmanaged").unwrap();

    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: true,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["pkg".to_string()],
    });

    assert!(result.is_err());
    assert!(target_dir.join("bin/tool").exists());
    assert_eq!(
        fs::read_to_string(target_dir.join("etc/app.conf")).unwrap(),
        "unmanaged"
    );
}

#[test]
fn test_cli_mixed_delete_open_directory_then_stow_file_replacement() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let old_package_dir = stow_dir.join("oldpkg");
    let new_package_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::create_dir_all(&new_package_dir).unwrap();
    fs::write(old_package_dir.join("bin/tool"), "old").unwrap();
    fs::write(new_package_dir.join("bin"), "new").unwrap();

    let mut old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    old_config.no_folding = true;
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("bin/tool").exists());
    assert!(target_dir.join("bin").is_dir());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "newpkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "mixed replacement failed: {:?}",
        result.err()
    );
    assert!(
        fs::read_link(target_dir.join("bin"))
            .unwrap()
            .ends_with("stow_dir/newpkg/bin")
    );
}

#[test]
fn test_binary_double_dash_allows_dash_prefixed_package_names() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let package_dir = stow_dir.join("-D");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/dash_tool"), "dash").unwrap();

    let output = run_rustow([
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "--",
        "-D",
    ]);

    assert!(
        output.status.success(),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(target_dir.join("bin/dash_tool").exists());
}

#[test]
fn test_binary_double_dash_preserves_verbose_like_package_names() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let package_dir = stow_dir.join("--verbose=0");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/verbose_tool"), "verbose").unwrap();

    let output = run_rustow([
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "--",
        "--verbose=0",
    ]);

    assert!(
        output.status.success(),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(target_dir.join("bin/verbose_tool").exists());
}

#[test]
fn test_binary_verbose_numeric_level_is_accepted() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();

    let output = run_rustow([
        "--verbose=2",
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "pkg",
    ]);

    assert!(
        output.status.success(),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Summary:"));
}

#[test]
fn test_binary_compat_verbosity_cluster_preserves_verbosity() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();

    let output = run_rustow([
        "-d",
        stow_dir.to_str().unwrap(),
        "-t",
        target_dir.to_str().unwrap(),
        "-pv",
        "pkg",
    ]);

    assert!(
        output.status.success(),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("Summary:"));
    assert!(target_dir.join("bin/tool").exists());
}

#[test]
fn test_binary_verbose_parse_errors_and_help_exit_codes() {
    let invalid_verbose = run_rustow(["--verbose=6", "pkg"]);
    assert_eq!(invalid_verbose.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid_verbose.stderr).contains("between 0 and 5"));

    let help_with_invalid_verbose = run_rustow(["--help", "--verbose=6"]);
    assert_eq!(help_with_invalid_verbose.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help_with_invalid_verbose.stdout).contains("Usage:"));

    let version_with_invalid_verbose = run_rustow(["--version", "-vvvvvv"]);
    assert_eq!(version_with_invalid_verbose.status.code(), Some(0));

    let help_after_version = run_rustow(["--version", "--help"]);
    assert_eq!(help_after_version.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help_after_version.stdout).contains("Usage:"));

    let help_in_short_cluster = run_rustow(["-Vh"]);
    assert_eq!(help_in_short_cluster.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help_in_short_cluster.stdout).contains("Usage:"));

    let version_after_help_in_short_cluster = run_rustow(["-hV"]);
    assert_eq!(version_after_help_in_short_cluster.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&version_after_help_in_short_cluster.stdout).contains("Usage:")
    );
}

#[test]
fn test_binary_help_and_version_keep_precedence_after_packages() {
    let help_output = run_rustow(["pkg", "--help"]);
    assert_eq!(help_output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help_output.stdout).contains("Usage:"));

    let version_output = run_rustow(["pkg", "-V"]);
    assert_eq!(version_output.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&version_output.stdout).is_empty());

    let help_with_extra_arg_output = run_rustow(["pkg", "--help", "extra"]);
    assert_eq!(help_with_extra_arg_output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help_with_extra_arg_output.stdout).contains("Usage:"));

    let version_with_extra_arg_output = run_rustow(["pkg", "--version", "extra"]);
    assert_eq!(version_with_extra_arg_output.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&version_with_extra_arg_output.stdout).is_empty());
}

#[test]
fn test_binary_stowrc_options_from_current_and_home_are_prepared() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let stow_local = temp_dir.path().join("local_stow");
    let stow_home = temp_dir.path().join("home_stow");
    let target_local = temp_dir.path().join("target_local");
    let target_home = temp_dir.path().join("target_home");
    let package_local = stow_local.join("pkg");
    let package_home = stow_home.join("pkg");

    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(package_local.join("bin")).unwrap();
    fs::create_dir_all(package_home.join("bin")).unwrap();
    fs::create_dir_all(&target_local).unwrap();
    fs::create_dir_all(&target_home).unwrap();
    fs::write(package_local.join("bin/tool"), "local").unwrap();
    fs::write(package_home.join("bin/tool"), "home").unwrap();

    let envs = vec![(
        "HOME",
        home_dir.to_str().expect("home dir should be valid utf-8"),
    )];

    fs::write(
        cwd.join(".stowrc"),
        format!(
            "--dir={}\n--target={}\n",
            stow_local.to_string_lossy(),
            target_local.to_string_lossy()
        ),
    )
    .unwrap();
    fs::write(
        home_dir.join(".stowrc"),
        format!(
            "--dir={}\n--target={}\n",
            stow_home.to_string_lossy(),
            target_home.to_string_lossy()
        ),
    )
    .unwrap();

    let output = run_rustow_with(["pkg"], &cwd, &envs);
    assert_eq!(
        output.status.code(),
        Some(0),
        "rustow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        target_local.join("bin").exists(),
        "current-dir stowrc should override home stowrc"
    );
    assert!(
        !target_home.join("bin").exists(),
        "home stowrc should not override current-dir stowrc"
    );

    let output_with_cli_override = run_rustow_with(
        [
            "-d",
            stow_home.to_string_lossy().as_ref(),
            "-t",
            target_home.to_string_lossy().as_ref(),
            "pkg",
        ],
        &cwd,
        &envs,
    );
    assert_eq!(
        output_with_cli_override.status.code(),
        Some(0),
        "rustow with cli override should succeed: {}",
        String::from_utf8_lossy(&output_with_cli_override.stderr)
    );
    assert!(
        target_home.join("bin").exists(),
        "cli flags should override resource files"
    );
}

#[test]
fn test_binary_stowrc_rejects_missing_value_without_consuming_cli_package() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join(".stowrc"), "--target\n").unwrap();

    let envs = vec![(
        "HOME",
        home_dir.to_str().expect("home dir should be valid utf-8"),
    )];

    let output = run_rustow_with(["pkg"], &cwd, &envs);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("resource file option '--target' requires a value")
    );
}

#[test]
fn test_binary_help_ignores_malformed_stowrc() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join(".stowrc"), "--target\n").unwrap();

    let envs = vec![(
        "HOME",
        home_dir.to_str().expect("home dir should be valid utf-8"),
    )];

    let output = run_rustow_with(["--help"], &cwd, &envs);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
}

#[test]
fn test_binary_version_ignores_malformed_stowrc() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join(".stowrc"), "--target\n").unwrap();

    let envs = vec![(
        "HOME",
        home_dir.to_str().expect("home dir should be valid utf-8"),
    )];

    let output = run_rustow_with(["--version"], &cwd, &envs);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains("rustow"));
}

#[test]
fn test_binary_stowrc_env_expanded_path_is_redacted_in_config_error() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_SECRET_PATH/missing\n").unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        ("RUSTOW_SECRET_PATH", "secret-value-from-env"),
    ];

    let output = run_rustow_with(["pkg"], &cwd, &envs);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_PATH/missing"));
    assert!(stderr.contains("Invalid stow directory"));
    assert!(!stderr.contains("Operation failed"));

    let stow_dir = temp_dir.path().join("stow");
    fs::create_dir_all(stow_dir.join("pkg")).unwrap();
    fs::write(
        cwd.join(".stowrc"),
        format!(
            "--dir={}\n--target=$RUSTOW_SECRET_PATH/missing\n",
            stow_dir.to_string_lossy()
        ),
    )
    .unwrap();

    let output = run_rustow_with(["pkg"], &cwd, &envs);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_PATH/missing"));
    assert!(stderr.contains("Invalid target directory"));
    assert!(!stderr.contains("Operation failed"));

    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_TILDE_SECRET/missing\n").unwrap();
    let tilde_secret = "~/secret-value-from-env";
    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        ("RUSTOW_TILDE_SECRET", tilde_secret),
    ];
    let output = run_rustow_with(["pkg"], &cwd, &envs);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("secret-value-from-env"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("$RUSTOW_TILDE_SECRET/missing"));

    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_SHORT_SECRET/missing\n").unwrap();
    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        ("RUSTOW_SHORT_SECRET", "abc"),
    ];
    let output = run_rustow_with(["pkg"], &cwd, &envs);
    assert_eq!(output.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("abc/missing"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("$RUSTOW_SHORT_SECRET/missing"));
}

#[test]
fn test_binary_stowrc_undefined_env_errors_before_cli_override() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let cli_stow_dir = temp_dir.path().join("cli-stow");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(cli_stow_dir.join("pkg")).unwrap();
    fs::write(
        cwd.join(".stowrc"),
        "--dir=$RUSTOW_TEST_UNDEFINED_STOWRC_VAR_FOR_COMPAT\n",
    )
    .unwrap();

    let home = home_dir.to_str().expect("home dir should be valid utf-8");
    let output = Command::new(env!("CARGO_BIN_EXE_rustow"))
        .args(["pkg"])
        .current_dir(&cwd)
        .env("HOME", home)
        .env_remove("STOW_DIR")
        .env_remove("RUSTOW_TEST_UNDEFINED_STOWRC_VAR_FOR_COMPAT")
        .output()
        .expect("Failed to run rustow binary");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "stderr: {}", stderr);
    assert!(stderr.contains(
        "--dir option references undefined environment variable $RUSTOW_TEST_UNDEFINED_STOWRC_VAR_FOR_COMPAT; aborting!"
    ));

    let cli_override = Command::new(env!("CARGO_BIN_EXE_rustow"))
        .args([
            "--dir",
            cli_stow_dir
                .to_str()
                .expect("stow dir should be valid utf-8"),
            "pkg",
        ])
        .current_dir(&cwd)
        .env("HOME", home)
        .env_remove("STOW_DIR")
        .env_remove("RUSTOW_TEST_UNDEFINED_STOWRC_VAR_FOR_COMPAT")
        .output()
        .expect("Failed to run rustow binary");
    let stderr = String::from_utf8_lossy(&cli_override.stderr);
    assert_eq!(cli_override.status.code(), Some(2), "stderr: {}", stderr);
    assert!(stderr.contains(
        "--dir option references undefined environment variable $RUSTOW_TEST_UNDEFINED_STOWRC_VAR_FOR_COMPAT; aborting!"
    ));
}

#[test]
fn test_binary_direct_cli_config_error_keeps_literal_path() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();

    let missing_stow = home_dir.join("missing-stow");
    fs::write(
        cwd.join(".stowrc"),
        "--dir=$RUSTOW_SECRET_PATH/missing-stow\n",
    )
    .unwrap();
    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_PATH",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(
        ["-d", missing_stow.to_string_lossy().as_ref(), "pkg"],
        &cwd,
        &envs,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(missing_stow.to_string_lossy().as_ref())
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("$HOME/missing-stow"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("$RUSTOW_SECRET_PATH/missing-stow"));
}

#[test]
fn test_binary_stowrc_env_expanded_package_validation_error_is_redacted() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_stow_dir = temp_dir.path().join("secret-value-from-env");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&secret_stow_dir).unwrap();
    fs::write(secret_stow_dir.join("pkg"), "not a directory").unwrap();
    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_SECRET_STOW_DIR\n").unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_STOW_DIR",
            secret_stow_dir
                .to_str()
                .expect("stow dir should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["pkg"], &cwd, &envs);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1));
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_STOW_DIR/pkg"));
}

#[test]
fn test_binary_stowrc_env_expanded_action_output_is_redacted() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_root = temp_dir.path().join("secret-value-from-env");
    let stow_dir = secret_root.join("stow");
    let target_dir = secret_root.join("target");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(stow_dir.join("pkg/bin")).unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(stow_dir.join("pkg/bin/tool"), "tool").unwrap();
    fs::write(
        cwd.join(".stowrc"),
        "--dir=$RUSTOW_SECRET_ROOT/stow\n--target=$RUSTOW_SECRET_ROOT/target\n",
    )
    .unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_ROOT",
            secret_root
                .to_str()
                .expect("secret root should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["--simulate", "pkg"], &cwd, &envs);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/stow/pkg/bin"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/target/bin"));
}

#[test]
fn test_binary_stowrc_env_expanded_conflict_output_is_redacted() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_root = temp_dir.path().join("secret-value-from-env");
    let stow_dir = secret_root.join("stow");
    let target_dir = secret_root.join("target");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(stow_dir.join("pkg/bin")).unwrap();
    fs::create_dir_all(target_dir.join("bin")).unwrap();
    fs::write(stow_dir.join("pkg/bin/tool"), "tool").unwrap();
    fs::write(target_dir.join("bin/tool"), "existing").unwrap();
    fs::write(
        cwd.join(".stowrc"),
        "--dir=$RUSTOW_SECRET_ROOT/stow\n--target=$RUSTOW_SECRET_ROOT/target\n",
    )
    .unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_ROOT",
            secret_root
                .to_str()
                .expect("secret root should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["pkg"], &cwd, &envs);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/target/bin/tool"));
}

#[test]
fn test_binary_stowrc_env_expanded_failure_output_is_redacted() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_root = temp_dir.path().join("secret-value-from-env");
    let stow_dir = secret_root.join("stow");
    let target_dir = secret_root.join("target");
    let external_dir = temp_dir.path().join("external");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(stow_dir.join("pkg/bin")).unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&external_dir).unwrap();
    fs::write(stow_dir.join("pkg/bin/tool"), "tool").unwrap();
    rustow::fs_utils::create_symlink(&target_dir.join("bin"), &external_dir).unwrap();
    fs::write(
        cwd.join(".stowrc"),
        "--dir=$RUSTOW_SECRET_ROOT/stow\n--target=$RUSTOW_SECRET_ROOT/target\n",
    )
    .unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_ROOT",
            secret_root
                .to_str()
                .expect("secret root should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["--no-folding", "pkg"], &cwd, &envs);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/target/bin"));
}

#[cfg(unix)]
#[test]
fn test_binary_stowrc_env_expanded_planning_error_is_redacted() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }

    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_root = temp_dir.path().join("secret-value-from-env");
    let stow_dir = secret_root.join("stow");
    let blocked_dir = stow_dir.join("pkg/blocked");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&blocked_dir).unwrap();
    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_SECRET_ROOT/stow\n").unwrap();

    let mut blocked_permissions = fs::metadata(&blocked_dir).unwrap().permissions();
    blocked_permissions.set_mode(0o0);
    fs::set_permissions(&blocked_dir, blocked_permissions).unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_ROOT",
            secret_root
                .to_str()
                .expect("secret root should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["pkg"], &cwd, &envs);

    let mut restored_permissions = fs::metadata(&blocked_dir).unwrap().permissions();
    restored_permissions.set_mode(0o700);
    fs::set_permissions(&blocked_dir, restored_permissions).unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/stow/pkg/blocked"));
    assert!(stderr.contains("WalkDir error"));
    assert!(!stderr.contains("Operation failed"));
}

#[cfg(unix)]
#[test]
fn test_binary_stowrc_env_expanded_default_target_planning_error_is_redacted() {
    if unsafe { libc::geteuid() } == 0 {
        return;
    }

    use std::os::unix::fs::PermissionsExt;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_root = temp_dir.path().join("secret-value-from-env");
    let stow_dir = secret_root.join("stow");
    let blocked_target_dir = secret_root.join("blocked");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(stow_dir.join("pkg/blocked")).unwrap();
    fs::write(stow_dir.join("pkg/blocked/tool"), "tool").unwrap();
    fs::create_dir_all(&blocked_target_dir).unwrap();
    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_SECRET_ROOT/stow\n").unwrap();

    let mut blocked_permissions = fs::metadata(&blocked_target_dir).unwrap().permissions();
    blocked_permissions.set_mode(0o0);
    fs::set_permissions(&blocked_target_dir, blocked_permissions).unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_ROOT",
            secret_root
                .to_str()
                .expect("secret root should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["pkg"], &cwd, &envs);

    let mut restored_permissions = fs::metadata(&blocked_target_dir).unwrap().permissions();
    restored_permissions.set_mode(0o700);
    fs::set_permissions(&blocked_target_dir, restored_permissions).unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "stderr: {}", stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/blocked"));
    assert!(stderr.contains("IO error"));
    assert!(!stderr.contains("Operation failed"));
}

#[test]
fn test_binary_stowrc_env_expanded_default_target_output_is_redacted() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_root = temp_dir.path().join("secret-value-from-env");
    let stow_dir = secret_root.join("stow");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(stow_dir.join("pkg/bin")).unwrap();
    fs::write(stow_dir.join("pkg/bin/tool"), "tool").unwrap();
    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_SECRET_ROOT/stow\n").unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_ROOT",
            secret_root
                .to_str()
                .expect("secret root should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["--simulate", "pkg"], &cwd, &envs);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/bin"));
    assert!(stderr.contains("$RUSTOW_SECRET_ROOT/stow/pkg/bin"));
}

#[test]
fn test_binary_stowrc_env_expanded_bare_env_default_target_output_is_redacted() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let secret_root = temp_dir.path().join("secret-value-from-env");
    let stow_dir = secret_root.join("stow");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(stow_dir.join("pkg/bin")).unwrap();
    fs::write(stow_dir.join("pkg/bin/tool"), "tool").unwrap();
    fs::write(cwd.join(".stowrc"), "--dir=$RUSTOW_SECRET_STOW_DIR\n").unwrap();

    let envs = vec![
        (
            "HOME",
            home_dir.to_str().expect("home dir should be valid utf-8"),
        ),
        (
            "RUSTOW_SECRET_STOW_DIR",
            stow_dir.to_str().expect("stow dir should be valid utf-8"),
        ),
    ];
    let output = run_rustow_with(["--simulate", "pkg"], &cwd, &envs);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr);
    assert!(!stderr.contains("secret-value-from-env"));
    assert!(stderr.contains("$RUSTOW_SECRET_STOW_DIR/pkg/bin"));
    assert!(stderr.contains("$RUSTOW_SECRET_STOW_DIR/../bin"));
}

#[test]
fn test_binary_stowrc_expansion_and_quote_handling() {
    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let stow_dir = temp_dir.path().join("stow");
    let target_dir = home_dir.join("target");
    let package_dir = stow_dir.join("quotedpkg");

    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin").join("tool"), "tool").unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        cwd.join(".stowrc"),
        format!(
            "--dir=\"{}\"\n--target=\"$HOME/{}\"\n",
            stow_dir.to_string_lossy(),
            target_dir.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();

    let envs = vec![(
        "HOME",
        home_dir.to_str().expect("home dir should be valid utf-8"),
    )];

    let output = run_rustow_with(["quotedpkg"], &cwd, &envs);
    assert_eq!(
        output.status.code(),
        Some(0),
        "rustow with quoted stowrc expansion should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        target_dir.join("bin").join("tool").exists(),
        "expanded target should be honored"
    );
}

#[test]
fn test_binary_accepts_hyphen_prefixed_separate_option_values() {
    for args in [
        vec!["-d", "-D", "pkg"],
        vec!["--target", "--restow", "pkg"],
        vec!["--ignore", "-S", "pkg"],
        vec!["-Dt", "--help", "pkg"],
        vec!["-Dt", "--verbose", "pkg"],
        vec!["-d", "--simulate", "pkg"],
        vec!["-d", "-n", "pkg"],
        vec!["-Sd", "-n", "pkg"],
        vec!["--target", "--adopt", "pkg"],
        vec!["-d", "--verbose", "pkg"],
        vec!["--target", "--verbose", "pkg"],
        vec!["--ignore", "--verbose", "pkg"],
        vec!["--defer", "--verbose", "pkg"],
        vec!["--override", "--verbose", "pkg"],
        vec!["-Rt", "--simulate", "pkg"],
    ] {
        let output = run_rustow(args);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert_ne!(output.status.code(), Some(2));
        assert!(!stderr.contains("requires a value"));
    }

    let help_as_value = run_rustow(["-Dt", "--help", "pkg"]);
    assert_ne!(help_as_value.status.code(), Some(0));
    assert!(!String::from_utf8_lossy(&help_as_value.stdout).contains("Usage:"));

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let home_dir = temp_dir.path().join("home");
    let cwd = temp_dir.path().join("cwd");
    let stow_dir = temp_dir.path().join("stow");
    let target_dir = cwd.join("--verbose");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(stow_dir.join("pkg/bin")).unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(stow_dir.join("pkg/bin/tool"), "tool").unwrap();

    let envs = vec![(
        "HOME",
        home_dir.to_str().expect("home dir should be valid utf-8"),
    )];
    let output = run_rustow_with(
        [
            "--dir",
            stow_dir.to_str().expect("stow dir should be valid utf-8"),
            "--target",
            "--verbose",
            "pkg",
        ],
        &cwd,
        &envs,
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(target_dir.join("bin/tool").exists());
}

#[test]
fn test_cli_mixed_repeated_mode_switches_are_accepted() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    for package_name in ["oldpkg", "olderpkg", "newpkg"] {
        let package_dir = stow_dir.join(package_name);
        fs::create_dir_all(package_dir.join("bin")).unwrap();
        fs::write(
            package_dir.join("bin").join(format!("{package_name}_tool")),
            package_name,
        )
        .unwrap();
    }

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string(), "olderpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("bin/oldpkg_tool").exists());
    assert!(target_dir.join("bin/olderpkg_tool").exists());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "newpkg".to_string(),
        "-D".to_string(),
        "olderpkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "repeated mixed operation modes should be accepted: {:?}",
        result.err()
    );
    assert!(!target_dir.join("bin/oldpkg_tool").exists());
    assert!(!target_dir.join("bin/olderpkg_tool").exists());
    assert!(target_dir.join("bin/newpkg_tool").exists());
}

#[test]
fn test_cli_mixed_operations_preflight_missing_package_before_delete() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let old_package_dir = stow_dir.join("oldpkg");
    fs::create_dir_all(old_package_dir.join("bin")).unwrap();
    fs::write(old_package_dir.join("bin/old_tool"), "old").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("bin/old_tool").exists());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "missingpkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_err(),
        "mixed operation should fail during preflight"
    );
    assert!(
        target_dir.join("bin/old_tool").exists(),
        "old package should remain stowed when a later package is missing"
    );
}

#[test]
fn test_cli_clustered_target_option_value_is_not_treated_as_package() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let package_dir = stow_dir.join("oldpkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/old_tool"), "old").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("bin/old_tool").exists());

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-Dt".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "oldpkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "clustered -Dt should not schedule the target path as a package: {:?}",
        result.err()
    );
    assert!(
        !target_dir.join("bin/old_tool").exists(),
        "old package should be unstowed"
    );
}

#[test]
fn test_cli_double_dash_allows_dash_prefixed_package_names() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let package_dir = stow_dir.join("-D");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/dash_tool"), "dash").unwrap();

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "--".to_string(),
        "-D".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "dash-prefixed package after -- should be stowed: {:?}",
        result.err()
    );
    assert!(target_dir.join("bin/dash_tool").exists());
}

#[test]
fn test_cli_package_symlink_alias_can_be_stowed_and_deleted() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let package_dir = stow_dir.join("realpkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "real").unwrap();
    rustow::fs_utils::create_symlink(&stow_dir.join("aliaspkg"), Path::new("realpkg")).unwrap();

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
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["aliaspkg".to_string()],
    });
    assert!(
        stow_result.is_ok(),
        "alias stow failed: {:?}",
        stow_result.err()
    );
    assert!(target_dir.join("bin/tool").exists());
    assert_eq!(
        fs::read_link(target_dir.join("bin")).unwrap(),
        PathBuf::from("../stow_dir/aliaspkg/bin")
    );

    let repeat_stow_result = rustow::run(Args {
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
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["aliaspkg".to_string()],
    });
    assert!(
        repeat_stow_result.is_ok(),
        "repeat alias stow failed: {:?}",
        repeat_stow_result.err()
    );

    let restow_result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: true,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["aliaspkg".to_string()],
    });
    assert!(
        restow_result.is_ok(),
        "alias restow failed: {:?}",
        restow_result.err()
    );
    assert_eq!(
        fs::read_link(target_dir.join("bin")).unwrap(),
        PathBuf::from("../stow_dir/aliaspkg/bin")
    );

    let delete_result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: true,
        restow: false,
        adopt: false,
        no_folding: false,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["aliaspkg".to_string()],
    });

    assert!(
        delete_result.is_ok(),
        "alias delete failed: {:?}",
        delete_result.err()
    );
    assert!(!target_dir.join("bin/tool").exists());
}

#[test]
fn test_cli_mixed_delete_real_package_and_stow_alias_package() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();

    let package_dir = stow_dir.join("realpkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "real").unwrap();
    rustow::fs_utils::create_symlink(&stow_dir.join("aliaspkg"), Path::new("realpkg")).unwrap();

    let real_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["realpkg".to_string()],
        false,
        0,
    );
    stow_packages(&real_config).unwrap();
    assert_eq!(
        fs::read_link(target_dir.join("bin")).unwrap(),
        PathBuf::from("../stow_dir/realpkg/bin")
    );

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "realpkg".to_string(),
        "-S".to_string(),
        "aliaspkg".to_string(),
    ]);

    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "mixed alias replacement failed: {:?}",
        result.err()
    );
    assert_eq!(
        fs::read_link(target_dir.join("bin")).unwrap(),
        PathBuf::from("../stow_dir/aliaspkg/bin")
    );
}

#[test]
fn test_restow_preserves_unrelated_empty_directories() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    stow_packages(&config).unwrap();
    let unrelated_empty_dir = target_dir.join(".local/state/app/empty");
    fs::create_dir_all(&unrelated_empty_dir).unwrap();

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(unrelated_empty_dir.is_dir());
}

#[test]
fn test_restow_does_not_refold_directory_with_ignored_descendants() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("config")).unwrap();
    fs::write(package_dir.join("config/visible"), "visible").unwrap();
    fs::write(package_dir.join("config/secret"), "secret").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.ignore_patterns = vec![regex::Regex::new("secret").unwrap()];
    stow_packages(&config).unwrap();
    assert!(target_dir.join("config").is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_dir.join("config")));
    assert!(target_dir.join("config/visible").exists());
    assert!(!target_dir.join("config/secret").exists());

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(target_dir.join("config").is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_dir.join("config")));
    assert!(target_dir.join("config/visible").exists());
    assert!(!target_dir.join("config/secret").exists());
}

#[test]
fn test_mixed_restow_does_not_refold_directory_with_ignored_descendants() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let side_dir = stow_dir.join("side");
    fs::create_dir_all(package_dir.join("config")).unwrap();
    fs::create_dir_all(side_dir.join("share")).unwrap();
    fs::write(package_dir.join("config/visible"), "visible").unwrap();
    fs::write(package_dir.join("config/secret"), "secret").unwrap();
    fs::write(side_dir.join("share/side"), "side").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.ignore_patterns = vec![regex::Regex::new("secret").unwrap()];
    stow_packages(&config).unwrap();

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "--ignore".to_string(),
        "secret".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-R".to_string(),
        "pkg".to_string(),
        "-S".to_string(),
        "side".to_string(),
    ]);
    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(result.is_ok(), "mixed restow failed: {:?}", result.err());
    assert!(target_dir.join("config").is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_dir.join("config")));
    assert!(target_dir.join("config/visible").exists());
    assert!(!target_dir.join("config/secret").exists());
    assert!(target_dir.join("share/side").exists());
}

#[test]
fn test_restow_default_preserves_unrelated_symlink_to_package_path() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("bin")).unwrap();
    fs::write(package_dir.join("bin/tool"), "tool").unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.no_folding = true;
    stow_packages(&config).unwrap();
    let unrelated_dir = target_dir.join("unrelated");
    fs::create_dir_all(&unrelated_dir).unwrap();
    rustow::fs_utils::create_symlink(&unrelated_dir.join("tool"), &stow_dir.join("pkg/bin/tool"))
        .unwrap();

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(rustow::fs_utils::is_symlink(&unrelated_dir.join("tool")));
}

#[test]
fn test_restow_does_not_scan_through_folded_directory_symlink_into_package_source() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("config/nested")).unwrap();
    fs::write(package_dir.join("config/nested/file"), "file").unwrap();
    rustow::fs_utils::create_symlink(
        &package_dir.join("config/nested/source_link"),
        &package_dir.join("config/nested/file"),
    )
    .unwrap();

    let config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    stow_packages(&config).unwrap();
    assert!(rustow::fs_utils::is_symlink(&target_dir.join("config")));
    assert!(rustow::fs_utils::is_symlink(
        &package_dir.join("config/nested/source_link")
    ));

    let mut restow_config = config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(rustow::fs_utils::is_symlink(
        &package_dir.join("config/nested/source_link")
    ));
}

#[test]
fn test_restow_preserves_unrelated_foldable_open_directory() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let other_dir = stow_dir.join("other");
    fs::create_dir_all(other_dir.join("bin")).unwrap();
    fs::write(other_dir.join("bin/other_tool"), "other").unwrap();

    let mut other_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["other".to_string()],
        false,
        0,
    );
    other_config.no_folding = true;
    stow_packages(&other_config).unwrap();
    assert!(target_dir.join("bin").is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_dir.join("bin")));

    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("share")).unwrap();
    fs::write(package_dir.join("share/pkg_tool"), "pkg").unwrap();
    let package_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    stow_packages(&package_config).unwrap();

    let mut restow_config = package_config.clone();
    restow_config.mode = StowMode::Restow;
    let result = restow_packages(&restow_config);

    assert!(result.is_ok(), "restow failed: {:?}", result.err());
    assert!(target_dir.join("bin").is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_dir.join("bin")));
    assert!(rustow::fs_utils::is_symlink(
        &target_dir.join("bin/other_tool")
    ));
}

#[test]
fn test_mixed_operations_preserve_unrelated_foldable_open_directory() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let other_dir = stow_dir.join("other");
    fs::create_dir_all(other_dir.join("bin")).unwrap();
    fs::write(other_dir.join("bin/other_tool"), "other").unwrap();

    let mut other_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["other".to_string()],
        false,
        0,
    );
    other_config.no_folding = true;
    stow_packages(&other_config).unwrap();

    let old_dir = stow_dir.join("oldpkg");
    let new_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_dir.join("share")).unwrap();
    fs::create_dir_all(new_dir.join("share")).unwrap();
    fs::write(old_dir.join("share/old_tool"), "old").unwrap();
    fs::write(new_dir.join("share/new_tool"), "new").unwrap();

    let old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    stow_packages(&old_config).unwrap();

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "newpkg".to_string(),
    ]);
    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(result.is_ok(), "mixed operation failed: {:?}", result.err());
    assert!(target_dir.join("bin").is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_dir.join("bin")));
    assert!(target_dir.join("share/new_tool").exists());
}

#[test]
fn test_mixed_refold_preserves_alias_package_path() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let real_dir = stow_dir.join("realpkg");
    let old_dir = stow_dir.join("oldpkg");
    let new_dir = stow_dir.join("newpkg");
    fs::create_dir_all(real_dir.join("bin")).unwrap();
    fs::create_dir_all(old_dir.join("bin")).unwrap();
    fs::create_dir_all(new_dir.join("share")).unwrap();
    fs::write(real_dir.join("bin/tool"), "real").unwrap();
    fs::write(old_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_dir.join("share/new_tool"), "new").unwrap();
    rustow::fs_utils::create_symlink(&stow_dir.join("aliaspkg"), Path::new("realpkg")).unwrap();

    let mut alias_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["aliaspkg".to_string()],
        false,
        0,
    );
    alias_config.no_folding = true;
    stow_packages(&alias_config).unwrap();

    let mut old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    old_config.no_folding = true;
    stow_packages(&old_config).unwrap();
    assert!(target_dir.join("bin").is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_dir.join("bin")));

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "newpkg".to_string(),
    ]);
    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(
        result.is_ok(),
        "mixed alias refold failed: {:?}",
        result.err()
    );
    assert_eq!(
        fs::read_link(target_dir.join("bin")).unwrap(),
        PathBuf::from("../stow_dir/aliaspkg/bin")
    );
    assert!(target_dir.join("bin/tool").exists());
}

#[test]
fn test_mixed_refold_rejects_package_alias_resolving_outside_stow_dir() {
    let temp_dir = tempdir().unwrap();
    let stow_dir = temp_dir.path().join("stow_dir");
    let target_dir = temp_dir.path().join("target_dir");
    let external_dir = temp_dir.path().join("external");
    fs::create_dir_all(&stow_dir).unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(external_dir.join("bin")).unwrap();
    fs::write(external_dir.join("bin/tool"), "external").unwrap();
    rustow::fs_utils::create_symlink(&stow_dir.join("evil"), &external_dir).unwrap();

    let old_dir = stow_dir.join("oldpkg");
    let new_dir = stow_dir.join("newpkg");
    fs::create_dir_all(old_dir.join("bin")).unwrap();
    fs::create_dir_all(new_dir.join("share")).unwrap();
    fs::write(old_dir.join("bin/old_tool"), "old").unwrap();
    fs::write(new_dir.join("share/new_tool"), "new").unwrap();

    let mut old_config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["oldpkg".to_string()],
        false,
        0,
    );
    old_config.no_folding = true;
    stow_packages(&old_config).unwrap();

    let target_bin = target_dir.join("bin");
    rustow::fs_utils::create_symlink(&target_bin.join("tool"), &stow_dir.join("evil/bin/tool"))
        .unwrap();
    assert!(target_bin.is_dir());
    assert!(rustow::fs_utils::is_symlink(&target_bin.join("tool")));
    assert!(rustow::fs_utils::is_symlink(&target_bin.join("old_tool")));

    let parsed_args = Args::parse_from_with_operation_groups(vec![
        "rustow".to_string(),
        "-d".to_string(),
        stow_dir.to_string_lossy().into_owned(),
        "-t".to_string(),
        target_dir.to_string_lossy().into_owned(),
        "-D".to_string(),
        "oldpkg".to_string(),
        "-S".to_string(),
        "newpkg".to_string(),
    ]);
    let result = rustow::run_with_operation_groups(parsed_args.args, parsed_args.operation_groups);

    assert!(result.is_ok(), "mixed operation failed: {:?}", result.err());
    assert!(target_bin.is_dir());
    assert!(!rustow::fs_utils::is_symlink(&target_bin));
    assert!(rustow::fs_utils::is_symlink(&target_bin.join("tool")));
    assert!(target_dir.join("share/new_tool").exists());
}

#[test]
fn test_stow_rejects_symlinked_target_ancestor_without_external_mutation() {
    let (temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let external_dir = temp_dir.path().join("external");
    fs::create_dir_all(package_dir.join("config")).unwrap();
    fs::create_dir_all(&external_dir).unwrap();
    fs::write(package_dir.join("config/tool"), "tool").unwrap();
    rustow::fs_utils::create_symlink(&target_dir.join("config"), &external_dir).unwrap();

    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: false,
        no_folding: true,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["pkg".to_string()],
    });

    assert!(result.is_err());
    assert!(!external_dir.join("tool").exists());
    assert!(rustow::fs_utils::is_symlink(&target_dir.join("config")));
}

#[test]
fn test_adopt_rejects_symlinked_target_ancestor_without_external_mutation() {
    let (temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let external_dir = temp_dir.path().join("external");
    fs::create_dir_all(package_dir.join("config")).unwrap();
    fs::create_dir_all(&external_dir).unwrap();
    fs::write(package_dir.join("config/secret"), "package").unwrap();
    fs::write(external_dir.join("secret"), "external").unwrap();
    rustow::fs_utils::create_symlink(&target_dir.join("config"), &external_dir).unwrap();

    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: true,
        no_folding: true,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["pkg".to_string()],
    });

    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(external_dir.join("secret")).unwrap(),
        "external"
    );
    assert_eq!(
        fs::read_to_string(package_dir.join("config/secret")).unwrap(),
        "package"
    );
}

#[test]
fn test_delete_rejects_symlinked_target_ancestor_without_external_symlink_deletion() {
    let (temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let external_dir = temp_dir.path().join("external");
    fs::create_dir_all(package_dir.join("link_parent")).unwrap();
    fs::create_dir_all(&external_dir).unwrap();
    fs::write(package_dir.join("link_parent/victim"), "package").unwrap();
    rustow::fs_utils::create_symlink(&target_dir.join("link_parent"), &external_dir).unwrap();
    rustow::fs_utils::create_symlink(
        &external_dir.join("victim"),
        &package_dir.join("link_parent/victim"),
    )
    .unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.mode = StowMode::Delete;
    let result = delete_packages(&config);

    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap()[0].status,
        TargetActionReportStatus::ConflictPrevented
    ));
    assert!(rustow::fs_utils::is_symlink(&external_dir.join("victim")));
}

#[test]
fn test_delete_rejects_symlinked_target_ancestor_without_external_directory_deletion() {
    let (temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    let external_dir = temp_dir.path().join("external");
    fs::create_dir_all(package_dir.join("link_parent/emptydir")).unwrap();
    fs::create_dir_all(external_dir.join("emptydir")).unwrap();
    rustow::fs_utils::create_symlink(&target_dir.join("link_parent"), &external_dir).unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.mode = StowMode::Delete;
    let result = delete_packages(&config);

    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap()[0].status,
        TargetActionReportStatus::ConflictPrevented
    ));
    assert!(external_dir.join("emptydir").exists());
}

#[test]
fn test_adopt_package_symlink_alias_uses_canonical_destination_and_preserves_alias_link() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let real_package_dir = stow_dir.join("realpkg");
    fs::create_dir_all(&real_package_dir).unwrap();
    fs::write(real_package_dir.join("tool"), "package").unwrap();
    rustow::fs_utils::create_symlink(&stow_dir.join("aliaspkg"), Path::new("realpkg")).unwrap();

    fs::write(target_dir.join("tool"), "target").unwrap();

    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: true,
        no_folding: true,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["aliaspkg".to_string()],
    });

    assert!(result.is_ok(), "alias adopt failed: {:?}", result.err());
    assert_eq!(
        fs::read_to_string(real_package_dir.join("tool")).unwrap(),
        "target"
    );
    assert_eq!(
        fs::read_link(target_dir.join("tool")).unwrap(),
        PathBuf::from("../stow_dir/aliaspkg/tool")
    );
}

#[test]
fn test_adopt_directory_does_not_traverse_symlinked_child_directory() {
    let (_temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("config")).unwrap();

    let external_dir = target_dir.join("../external");
    fs::create_dir_all(&external_dir).unwrap();
    fs::write(external_dir.join("secret.txt"), "secret").unwrap();
    let target_config_dir = target_dir.join("config");
    fs::create_dir_all(&target_config_dir).unwrap();
    rustow::fs_utils::create_symlink(&target_config_dir.join("external"), &external_dir).unwrap();

    let mut config = create_test_config(
        stow_dir.clone(),
        target_dir.clone(),
        vec!["pkg".to_string()],
        false,
        0,
    );
    config.adopt = true;
    config.no_folding = true;

    let result = stow_packages(&config);

    assert!(result.is_ok(), "adopt failed: {:?}", result.err());
    assert!(external_dir.join("secret.txt").exists());
    assert!(rustow::fs_utils::is_symlink(
        &package_dir.join("config/external")
    ));
}

#[test]
fn test_adopt_directory_refuses_symlinked_destination_child_directory() {
    let (temp_dir, stow_dir, target_dir): (TempDir, PathBuf, PathBuf) = setup_test_environment();
    let package_dir = stow_dir.join("pkg");
    fs::create_dir_all(package_dir.join("config")).unwrap();

    let external_dir = temp_dir.path().join("external_destination");
    fs::create_dir_all(&external_dir).unwrap();
    rustow::fs_utils::create_symlink(&package_dir.join("config/cache"), &external_dir).unwrap();

    let target_cache_dir = target_dir.join("config/cache");
    fs::create_dir_all(&target_cache_dir).unwrap();
    fs::write(target_cache_dir.join("file"), "target").unwrap();

    let result = rustow::run(Args {
        target: Some(target_dir.clone()),
        dir: Some(stow_dir.clone()),
        stow: false,
        delete: false,
        restow: false,
        adopt: true,
        no_folding: true,
        dotfiles: false,
        override_conflicts: Vec::new(),
        defer_conflicts: Vec::new(),
        ignore_patterns: Vec::new(),
        compat: false,
        simulate: false,
        verbose: 0,
        packages: vec!["pkg".to_string()],
    });

    assert!(result.is_err());
    assert!(!external_dir.join("file").exists());
    assert!(target_cache_dir.join("file").exists());
    assert!(rustow::fs_utils::is_symlink(
        &package_dir.join("config/cache")
    ));
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
                .is_some_and(|name| name == "conflicting_file.txt")
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
                .is_some_and(|name| name == "conflicting_file.txt")
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
                .is_some_and(|name| name == "deferred_file.txt")
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

    // Check override_me.txt - should be Conflict (override does not apply to non-stow targets)
    let override_report = reports
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .is_some_and(|name| name == "override_me.txt")
        })
        .expect("Should find report for override_me.txt");
    assert_eq!(
        override_report.original_action.action_type,
        ActionType::Conflict,
        "override_me.txt should be Conflict when target is not stow-managed"
    );

    // Check defer_me.txt - should be Conflict (defer does not apply to non-stow targets)
    let defer_report = reports
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .is_some_and(|name| name == "defer_me.txt")
        })
        .expect("Should find report for defer_me.txt");
    assert_eq!(
        defer_report.original_action.action_type,
        ActionType::Conflict,
        "defer_me.txt should be Conflict when target is not stow-managed"
    );

    // Check normal_file.txt - should be Conflict (no pattern matches)
    let normal_report = reports
        .iter()
        .find(|r| {
            r.original_action
                .target_path
                .file_name()
                .is_some_and(|name| name == "normal_file.txt")
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
        compat: false,
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
        compat: false,
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
        compat: false,
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
