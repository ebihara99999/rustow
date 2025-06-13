use crate::cli::Args;
use crate::error::{ConfigError, Result as RustowResult, RustowError};
use crate::fs_utils; // Import fs_utils
use regex::Regex;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StowMode {
    Stow,
    Delete,
    Restow,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub target_dir: PathBuf,
    pub stow_dir: PathBuf,
    pub packages: Vec<String>,
    pub mode: StowMode,
    pub stow: bool,
    pub adopt: bool,
    pub no_folding: bool,
    pub dotfiles: bool,
    pub overrides: Vec<Regex>,
    pub defers: Vec<Regex>,
    pub ignore_patterns: Vec<Regex>,
    pub simulate: bool,
    pub verbosity: u8,
    pub home_dir: PathBuf,
}

impl Config {
    pub fn from_args(args: Args) -> RustowResult<Self> {
        // 1. Determine StowMode
        let mode: StowMode = if args.delete {
            StowMode::Delete
        } else if args.restow {
            StowMode::Restow
        } else {
            StowMode::Stow
        };

        // 2. Resolve stow_dir
        let stow_dir_path_unresolved: PathBuf = match args.dir {
            Some(path) => path,
            None => match env::var("STOW_DIR") {
                Ok(val) => PathBuf::from(val),
                Err(_) => env::current_dir().map_err(|e| {
                    RustowError::Config(ConfigError::InvalidStowDir(format!(
                        "Failed to get current directory for stow_dir: {}",
                        e
                    )))
                })?,
            },
        };
        let stow_dir: PathBuf =
            fs_utils::canonicalize_path(&stow_dir_path_unresolved).map_err(|e| match e {
                RustowError::Fs(fs_error) => {
                    RustowError::Config(ConfigError::InvalidStowDir(format!(
                        "Failed to canonicalize stow directory '{}': {}",
                        stow_dir_path_unresolved.display(),
                        fs_error
                    )))
                },
                _ => RustowError::Config(ConfigError::InvalidStowDir(format!(
                    "An unexpected error occurred while canonicalizing stow directory '{}': {}",
                    stow_dir_path_unresolved.display(),
                    e
                ))),
            })?;

        // 3. Resolve target_dir
        let target_dir_path_unresolved: PathBuf = match args.target {
            Some(path) => path,
            None => stow_dir.parent().ok_or_else(|| {
                RustowError::Config(ConfigError::InvalidTargetDir(
                    format!("Stow directory '{}' has no parent, cannot determine default target directory", stow_dir.display())
                ))
            })?.to_path_buf(),
        };
        let target_dir: PathBuf = fs_utils::canonicalize_path(&target_dir_path_unresolved)
            .map_err(|e| match e {
                RustowError::Fs(fs_error) => {
                    RustowError::Config(ConfigError::InvalidTargetDir(format!(
                        "Failed to canonicalize target directory '{}': {}",
                        target_dir_path_unresolved.display(),
                        fs_error
                    )))
                },
                _ => RustowError::Config(ConfigError::InvalidTargetDir(format!(
                    "An unexpected error occurred while canonicalizing target directory '{}': {}",
                    target_dir_path_unresolved.display(),
                    e
                ))),
            })?;

        let home_dir: PathBuf = dirs::home_dir().ok_or_else(|| {
            RustowError::Config(ConfigError::InvalidStowDir(
                "Failed to determine home directory for loading global ignore file".to_string(),
            ))
        })?;

        // Compile override and defer patterns
        let mut overrides_compiled: Vec<Regex> = Vec::new();
        for pattern_str in &args.override_conflicts {
            match Regex::new(pattern_str) {
                Ok(re) => overrides_compiled.push(re),
                Err(e) => {
                    return Err(RustowError::Config(ConfigError::InvalidRegexPattern(
                        format!("Invalid --override pattern '{}': {}", pattern_str, e),
                    )));
                },
            }
        }

        let mut defers_compiled: Vec<Regex> = Vec::new();
        for pattern_str in &args.defer_conflicts {
            match Regex::new(pattern_str) {
                Ok(re) => defers_compiled.push(re),
                Err(e) => {
                    return Err(RustowError::Config(ConfigError::InvalidRegexPattern(
                        format!("Invalid --defer pattern '{}': {}", pattern_str, e),
                    )));
                },
            }
        }

        // Compile ignore patterns
        let mut ignore_patterns_compiled: Vec<Regex> = Vec::new();
        for pattern_str in &args.ignore_patterns {
            match Regex::new(pattern_str) {
                Ok(re) => ignore_patterns_compiled.push(re),
                Err(e) => {
                    return Err(RustowError::Config(ConfigError::InvalidRegexPattern(
                        format!("Invalid --ignore pattern '{}': {}", pattern_str, e),
                    )));
                },
            }
        }

        Ok(Self {
            target_dir,
            stow_dir,
            packages: args.packages.clone(),
            mode,
            stow: args.stow,
            adopt: args.adopt,
            no_folding: args.no_folding,
            dotfiles: args.dotfiles,
            overrides: overrides_compiled,
            defers: defers_compiled,
            ignore_patterns: ignore_patterns_compiled,
            simulate: args.simulate,
            verbosity: args.verbose,
            home_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Args;

    use clap::Parser;
    use std::fs;
    use tempfile::tempdir;

    fn basic_args_for_config_test(package_name: &str) -> Args {
        Args::parse_from(&["rustow", package_name])
    }

    #[test]
    fn test_config_from_basic_args_defaults() {
        let temp_stow_parent = tempdir().unwrap();
        let current_dir_original = env::current_dir().unwrap();
        env::set_current_dir(temp_stow_parent.path()).unwrap();

        let temp_stow_dir_name = "actual_stow_dir_for_test";
        let temp_stow_dir = temp_stow_parent.path().join(temp_stow_dir_name);
        fs::create_dir_all(&temp_stow_dir).unwrap();
        env::set_current_dir(&temp_stow_dir).unwrap();

        unsafe {
            env::remove_var("STOW_DIR");
        }
        let args = basic_args_for_config_test("testpkg");

        let config_result = Config::from_args(args);
        assert!(
            config_result.is_ok(),
            "Config::from_args failed: {:?}",
            config_result.err()
        );
        let config = config_result.unwrap();

        assert_eq!(config.packages, vec!["testpkg"]);
        assert_eq!(config.mode, StowMode::Stow);

        let expected_stow_dir = fs_utils::canonicalize_path(&temp_stow_dir).unwrap();
        let expected_target_dir = fs_utils::canonicalize_path(temp_stow_parent.path()).unwrap();

        assert_eq!(config.stow_dir, expected_stow_dir);
        assert_eq!(config.target_dir, expected_target_dir);

        env::set_current_dir(current_dir_original).unwrap();
    }

    #[test]
    fn test_stow_dir_from_option() {
        let temp_base = tempdir().unwrap();
        let specified_stow_dir = temp_base.path().join("my_stow");
        fs::create_dir_all(&specified_stow_dir).unwrap();

        unsafe {
            env::remove_var("STOW_DIR");
        }
        let args = Args::parse_from(&["rustow", "-d", specified_stow_dir.to_str().unwrap(), "pkg"]);
        let config = Config::from_args(args).unwrap();

        assert_eq!(
            config.stow_dir,
            fs_utils::canonicalize_path(&specified_stow_dir).unwrap()
        );
    }

    #[test]
    fn test_stow_dir_from_env_var() {
        let temp_base = tempdir().unwrap();
        let env_stow_dir_name = "env_stow_val";
        let env_stow_dir = temp_base.path().join(env_stow_dir_name);
        fs::create_dir_all(&env_stow_dir).unwrap();

        // Save original environment and current directory
        let original_stow_dir = env::var("STOW_DIR").ok();
        let current_dir_original = env::current_dir().unwrap();

        unsafe {
            env::set_var("STOW_DIR", env_stow_dir.to_str().unwrap());
        }

        // Need to be in a directory that is not the env_stow_dir for default target to make sense
        let another_dir = temp_base.path().join("another_place");
        fs::create_dir_all(&another_dir).unwrap();
        env::set_current_dir(&another_dir).unwrap();

        // Create args that will use STOW_DIR environment variable
        let args = Args::parse_from(&["rustow", "pkg_env"]);
        let config = Config::from_args(args).unwrap();

        // Restore environment and directory
        unsafe {
            match original_stow_dir {
                Some(val) => env::set_var("STOW_DIR", val),
                None => env::remove_var("STOW_DIR"),
            }
        }
        env::set_current_dir(current_dir_original).unwrap();

        assert_eq!(
            config.stow_dir,
            fs_utils::canonicalize_path(&env_stow_dir).unwrap()
        );
    }

    #[test]
    fn test_target_dir_from_option() {
        let temp_base = tempdir().unwrap();
        let specified_target_dir = temp_base.path().join("my_target");
        fs::create_dir_all(&specified_target_dir).unwrap();
        let dummy_stow_dir = temp_base.path().join("dummy_stow_for_target_test");
        fs::create_dir_all(&dummy_stow_dir).unwrap();

        let args = Args::parse_from(&[
            "rustow",
            "-t",
            specified_target_dir.to_str().unwrap(),
            "-d",
            dummy_stow_dir.to_str().unwrap(),
            "pkg",
        ]);
        let config = Config::from_args(args).unwrap();
        assert_eq!(
            config.target_dir,
            fs_utils::canonicalize_path(&specified_target_dir).unwrap()
        );
    }

    #[test]
    fn test_stow_dir_canonicalization_failure() {
        let non_existent_stow_dir = PathBuf::from("/path/that/definitely/does/not/exist/stow");
        let args = Args::parse_from(&[
            "rustow",
            "-d",
            non_existent_stow_dir.to_str().unwrap(),
            "pkg",
        ]);
        let config_result = Config::from_args(args);
        assert!(config_result.is_err());
        match config_result.err().unwrap() {
            RustowError::Config(ConfigError::InvalidStowDir(msg)) => {
                assert!(msg.contains("Failed to canonicalize stow directory"));
            },
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_target_dir_canonicalization_failure() {
        let temp_base = tempdir().unwrap();
        let valid_stow_dir = temp_base.path().join("valid_stow_target_fail");
        fs::create_dir_all(&valid_stow_dir).unwrap();
        let non_existent_target_dir = PathBuf::from("/path/that/equally/does/not/exist/target");

        let args = Args::parse_from(&[
            "rustow",
            "-d",
            valid_stow_dir.to_str().unwrap(),
            "-t",
            non_existent_target_dir.to_str().unwrap(),
            "pkg",
        ]);
        let config_result = Config::from_args(args);
        assert!(config_result.is_err());
        match config_result.err().unwrap() {
            RustowError::Config(ConfigError::InvalidTargetDir(msg)) => {
                assert!(msg.contains("Failed to canonicalize target directory"));
            },
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_stow_mode_delete() {
        let temp_base = tempdir().unwrap();
        let dummy_stow = temp_base.path().join("s");
        fs::create_dir_all(&dummy_stow).unwrap();
        let dummy_target = temp_base.path().join("t");
        fs::create_dir_all(&dummy_target).unwrap();
        let args = Args::parse_from(&[
            "rustow",
            "-D",
            "-d",
            dummy_stow.to_str().unwrap(),
            "-t",
            dummy_target.to_str().unwrap(),
            "pkg_del",
        ]);
        let config = Config::from_args(args).unwrap();
        assert_eq!(config.mode, StowMode::Delete);
    }

    #[test]
    fn test_stow_mode_restow() {
        let temp_base = tempdir().unwrap();
        let dummy_stow = temp_base.path().join("s_res");
        fs::create_dir_all(&dummy_stow).unwrap();
        let dummy_target = temp_base.path().join("t_res");
        fs::create_dir_all(&dummy_target).unwrap();
        let args = Args::parse_from(&[
            "rustow",
            "-R",
            "-d",
            dummy_stow.to_str().unwrap(),
            "-t",
            dummy_target.to_str().unwrap(),
            "pkg_res",
        ]);
        let config = Config::from_args(args).unwrap();
        assert_eq!(config.mode, StowMode::Restow);
    }

    #[test]
    fn test_override_defer_regex_compilation_success() {
        let temp_base = tempdir().unwrap();
        let stow_dir = temp_base.path().join("s_regex");
        fs::create_dir_all(&stow_dir).unwrap();
        let target_dir = temp_base.path().join("t_regex");
        fs::create_dir_all(&target_dir).unwrap();

        let args = Args::parse_from(&[
            "rustow",
            "-d",
            stow_dir.to_str().unwrap(),
            "-t",
            target_dir.to_str().unwrap(),
            "--override=^foo.*",
            "--override=bar$",
            "--defer=baz",
            "pkg_regex",
        ]);
        let config_result = Config::from_args(args);
        assert!(
            config_result.is_ok(),
            "Regex compilation failed: {:?}",
            config_result.err()
        );
        let config = config_result.unwrap();

        assert_eq!(config.overrides.len(), 2);
        assert_eq!(config.overrides[0].as_str(), "^foo.*");
        assert_eq!(config.overrides[1].as_str(), "bar$");
        assert_eq!(config.defers.len(), 1);
        assert_eq!(config.defers[0].as_str(), "baz");
    }

    #[test]
    fn test_override_regex_compilation_failure() {
        let temp_base = tempdir().unwrap();
        let stow_dir = temp_base.path().join("s_regex_fail_ov");
        fs::create_dir_all(&stow_dir).unwrap();
        let target_dir = temp_base.path().join("t_regex_fail_ov");
        fs::create_dir_all(&target_dir).unwrap();

        let invalid_pattern = "*invalid[";
        let args = Args::parse_from(&[
            "rustow",
            "-d",
            stow_dir.to_str().unwrap(),
            "-t",
            target_dir.to_str().unwrap(),
            &format!("--override={}", invalid_pattern),
            "pkg_regex_fail",
        ]);
        let config_result = Config::from_args(args);
        assert!(config_result.is_err());
        match config_result.err().unwrap() {
            RustowError::Config(ConfigError::InvalidRegexPattern(msg)) => {
                assert!(msg.contains("Invalid --override pattern"));
                assert!(msg.contains(invalid_pattern));
            },
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_defer_regex_compilation_failure() {
        let temp_base = tempdir().unwrap();
        let stow_dir = temp_base.path().join("s_regex_fail_def");
        fs::create_dir_all(&stow_dir).unwrap();
        let target_dir = temp_base.path().join("t_regex_fail_def");
        fs::create_dir_all(&target_dir).unwrap();

        let invalid_pattern = "(unclosed";
        let args = Args::parse_from(&[
            "rustow",
            "-d",
            stow_dir.to_str().unwrap(),
            "-t",
            target_dir.to_str().unwrap(),
            &format!("--defer={}", invalid_pattern),
            "pkg_regex_fail_defer",
        ]);
        let config_result = Config::from_args(args);
        assert!(config_result.is_err());
        match config_result.err().unwrap() {
            RustowError::Config(ConfigError::InvalidRegexPattern(msg)) => {
                assert!(msg.contains("Invalid --defer pattern"));
                assert!(msg.contains(invalid_pattern));
            },
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_stow_mode_explicit_stow() {
        let temp_base = tempdir().unwrap();
        let dummy_stow = temp_base.path().join("s_explicit");
        fs::create_dir_all(&dummy_stow).unwrap();
        let dummy_target = temp_base.path().join("t_explicit");
        fs::create_dir_all(&dummy_target).unwrap();

        let args = Args::parse_from(&[
            "rustow",
            "-S",
            "-d",
            dummy_stow.to_str().unwrap(),
            "-t",
            dummy_target.to_str().unwrap(),
            "pkg_stow",
        ]);
        let config = Config::from_args(args).unwrap();
        assert_eq!(config.mode, StowMode::Stow);
        assert!(config.stow);
    }

    #[test]
    fn test_ignore_patterns_compilation_success() {
        let temp_base = tempdir().unwrap();
        let stow_dir = temp_base.path().join("s_ignore");
        fs::create_dir_all(&stow_dir).unwrap();
        let target_dir = temp_base.path().join("t_ignore");
        fs::create_dir_all(&target_dir).unwrap();

        let args = Args::parse_from(&[
            "rustow",
            "-d",
            stow_dir.to_str().unwrap(),
            "-t",
            target_dir.to_str().unwrap(),
            "--ignore=\\.git",
            "--ignore=.*~$",
            "--ignore=node_modules",
            "pkg_ignore",
        ]);
        let config_result = Config::from_args(args);
        assert!(
            config_result.is_ok(),
            "Ignore patterns compilation failed: {:?}",
            config_result.err()
        );
        let config = config_result.unwrap();

        assert_eq!(config.ignore_patterns.len(), 3);
        assert_eq!(config.ignore_patterns[0].as_str(), "\\.git");
        assert_eq!(config.ignore_patterns[1].as_str(), ".*~$");
        assert_eq!(config.ignore_patterns[2].as_str(), "node_modules");
    }

    #[test]
    fn test_ignore_patterns_compilation_failure() {
        let temp_base = tempdir().unwrap();
        let stow_dir = temp_base.path().join("s_ignore_fail");
        fs::create_dir_all(&stow_dir).unwrap();
        let target_dir = temp_base.path().join("t_ignore_fail");
        fs::create_dir_all(&target_dir).unwrap();

        let invalid_pattern = "*invalid_ignore[";
        let args = Args::parse_from(&[
            "rustow",
            "-d",
            stow_dir.to_str().unwrap(),
            "-t",
            target_dir.to_str().unwrap(),
            &format!("--ignore={}", invalid_pattern),
            "pkg_ignore_fail",
        ]);
        let config_result = Config::from_args(args);
        assert!(config_result.is_err());
        match config_result.err().unwrap() {
            RustowError::Config(ConfigError::InvalidRegexPattern(msg)) => {
                assert!(msg.contains("Invalid --ignore pattern"));
                assert!(msg.contains(invalid_pattern));
            },
            e => panic!("Unexpected error type: {:?}", e),
        }
    }

    #[test]
    fn test_stow_with_ignore_and_other_options() {
        let temp_base = tempdir().unwrap();
        let stow_dir = temp_base.path().join("s_combined");
        fs::create_dir_all(&stow_dir).unwrap();
        let target_dir = temp_base.path().join("t_combined");
        fs::create_dir_all(&target_dir).unwrap();

        let args = Args::parse_from(&[
            "rustow",
            "-S",
            "--ignore=\\.git",
            "--ignore=temp",
            "--override=foo",
            "--defer=bar",
            "--dotfiles",
            "--adopt",
            "-v",
            "-d",
            stow_dir.to_str().unwrap(),
            "-t",
            target_dir.to_str().unwrap(),
            "pkg_combined",
        ]);
        let config = Config::from_args(args).unwrap();

        assert!(config.stow);
        assert_eq!(config.mode, StowMode::Stow);
        assert_eq!(config.ignore_patterns.len(), 2);
        assert_eq!(config.overrides.len(), 1);
        assert_eq!(config.defers.len(), 1);
        assert!(config.dotfiles);
        assert!(config.adopt);
        assert_eq!(config.verbosity, 1);
    }
}
