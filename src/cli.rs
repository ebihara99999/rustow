use clap::Parser;
use std::path::PathBuf;

/// Rustow: A Rust implementation of GNU Stow
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    // Ensure this is pub
    /// Target directory for symlinks
    #[clap(short, long, value_parser)]
    pub target: Option<PathBuf>,

    /// Directory containing stow packages
    #[clap(short, long, value_parser, env = "STOW_DIR")]
    pub dir: Option<PathBuf>,

    /// Stow the specified packages (default action)
    #[clap(short = 'S', long)]
    pub stow: bool,

    /// Delete specified packages from the target
    #[clap(short = 'D', long)]
    pub delete: bool,

    /// Restow specified packages (delete then stow)
    #[clap(short = 'R', long)]
    pub restow: bool,

    /// Adopt existing files in target into the stow package
    #[clap(long)]
    pub adopt: bool,

    /// Disable folding of directories
    #[clap(long)]
    pub no_folding: bool,

    /// Enable special handling for dotfiles (prefix files with 'dot-')
    #[clap(long)]
    pub dotfiles: bool,

    /// Override existing conflicting symlinks from other packages that match the regex
    #[clap(long = "override", value_parser)]
    pub override_conflicts: Vec<String>,

    /// Defer stowing files that would conflict with existing symlinks from other packages that match the regex
    #[clap(long = "defer", value_parser)]
    pub defer_conflicts: Vec<String>,

    /// Ignore files matching the specified regex pattern
    #[clap(long = "ignore", value_parser)]
    pub ignore_patterns: Vec<String>,

    /// Simulate execution, do not make any changes
    #[clap(short = 'n', long, alias = "no")]
    pub simulate: bool,

    /// Set verbosity level (e.g., -v, -vv, -vvv)
    #[clap(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Packages to process
    #[clap(value_parser, required = true, num_args = 1..)]
    pub packages: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to ensure STOW_DIR is cleared before and after tests that use it.
    // This is to prevent interference between tests when run in parallel.
    struct StowDirEnvGuard {
        original_value: Option<String>,
    }

    impl StowDirEnvGuard {
        fn new() -> Self {
            // Save original value if it exists
            let original_value = std::env::var("STOW_DIR").ok();
            // Clear the environment variable
            unsafe {
                std::env::remove_var("STOW_DIR");
            }
            StowDirEnvGuard { original_value }
        }
    }

    impl Drop for StowDirEnvGuard {
        fn drop(&mut self) {
            // Restore original value if it existed, otherwise ensure it's cleared
            unsafe {
                match &self.original_value {
                    Some(value) => std::env::set_var("STOW_DIR", value),
                    None => std::env::remove_var("STOW_DIR"),
                }
            }
        }
    }

    #[test]
    fn test_basic_stow_command() {
        let _guard = StowDirEnvGuard::new(); // Ensure STOW_DIR is clear
        let args = Args::parse_from(["rustow", "mypackage"]);
        assert_eq!(args.packages, vec!["mypackage"]);
        assert!(!args.stow); // explicitly set to false by default
        assert!(!args.delete);
        assert!(!args.restow);
        assert_eq!(args.verbose, 0);
        assert!(!args.simulate);
        assert!(args.target.is_none());
        assert!(args.dir.is_none());
        assert!(args.ignore_patterns.is_empty());
    }

    #[test]
    fn test_delete_option() {
        let args = Args::parse_from(["rustow", "-D", "mypackage"]);
        assert!(args.delete);
        assert!(!args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_restow_option() {
        let args = Args::parse_from(["rustow", "-R", "mypackage"]);
        assert!(args.restow);
        assert!(!args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_target_and_dir_options() {
        let args = Args::parse_from([
            "rustow",
            "-t",
            "/target/dir",
            "-d",
            "/stow/dir",
            "mypackage",
        ]);
        assert_eq!(args.target, Some(PathBuf::from("/target/dir")));
        assert_eq!(args.dir, Some(PathBuf::from("/stow/dir")));
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_verbose_option() {
        let args = Args::parse_from(["rustow", "-vvv", "mypackage"]);
        assert_eq!(args.verbose, 3);
        let args_single_v = Args::parse_from(["rustow", "-v", "mypackage"]);
        assert_eq!(args_single_v.verbose, 1);
    }

    #[test]
    fn test_multiple_packages() {
        let args = Args::parse_from(["rustow", "pkg1", "pkg2", "pkg3"]);
        assert_eq!(args.packages, vec!["pkg1", "pkg2", "pkg3"]);
    }

    #[test]
    fn test_simulate_option() {
        let args = Args::parse_from(["rustow", "-n", "mypackage"]);
        assert!(args.simulate);
        let args_long = Args::parse_from(["rustow", "--simulate", "mypackage"]);
        assert!(args_long.simulate);
        let args_alias = Args::parse_from(["rustow", "--no", "mypackage"]);
        assert!(args_alias.simulate);
    }

    #[test]
    fn test_override_defer_options() {
        let args = Args::parse_from([
            "rustow",
            "--override=foo",
            "--override=bar",
            "--defer=baz",
            "mypackage",
        ]);
        assert_eq!(args.override_conflicts, vec!["foo", "bar"]);
        assert_eq!(args.defer_conflicts, vec!["baz"]);
    }

    #[test]
    fn test_all_boolean_flags() {
        let args = Args::parse_from([
            "rustow",
            "--adopt",
            "--no-folding",
            "--dotfiles",
            "mypackage",
        ]);
        assert!(args.adopt);
        assert!(args.no_folding);
        assert!(args.dotfiles);
    }

    #[test]
    fn test_stow_dir_from_env() {
        // This test verifies that the Args struct is configured to read STOW_DIR from environment
        let _guard = StowDirEnvGuard::new(); // Ensure STOW_DIR is clear initially

        // Set STOW_DIR environment variable
        unsafe {
            std::env::set_var("STOW_DIR", "/env/stow/path");
        }

        // Test that when no -d option is provided, dir is read from STOW_DIR env var
        let args = Args::parse_from(["rustow", "mypackage"]);
        assert_eq!(args.dir, Some(PathBuf::from("/env/stow/path")));
    }

    #[test]
    fn test_stow_dir_no_env_no_option() {
        let _guard = StowDirEnvGuard::new(); // Ensure STOW_DIR is clear initially

        // Double-check that STOW_DIR is actually cleared
        assert!(
            std::env::var("STOW_DIR").is_err(),
            "STOW_DIR should be cleared"
        );

        // Test that when no -d option is provided and no STOW_DIR env var, dir is None
        let args = Args::parse_from(["rustow", "mypackage"]);
        assert!(
            args.dir.is_none(),
            "dir should be None when no STOW_DIR env var and no -d option"
        );
    }

    #[test]
    fn test_stow_dir_from_option_overrides_env() {
        let _guard = StowDirEnvGuard::new(); // Ensure STOW_DIR is clear initially
        unsafe {
            std::env::set_var("STOW_DIR", "/env/stow/path");
        }
        let args = Args::parse_from(["rustow", "-d", "/cmd/stow/path", "mypackage"]);
        assert_eq!(args.dir, Some(PathBuf::from("/cmd/stow/path")));
    }

    #[test]
    fn test_stow_option_short() {
        let args = Args::parse_from(["rustow", "-S", "mypackage"]);
        assert!(args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_stow_option_long() {
        let args = Args::parse_from(["rustow", "--stow", "mypackage"]);
        assert!(args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_ignore_option_single() {
        let args = Args::parse_from(["rustow", "--ignore=\\.git", "mypackage"]);
        assert_eq!(args.ignore_patterns, vec!["\\.git"]);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_ignore_option_multiple() {
        let args = Args::parse_from([
            "rustow",
            "--ignore=\\.git",
            "--ignore=.*~",
            "--ignore=node_modules",
            "mypackage",
        ]);
        assert_eq!(args.ignore_patterns, vec!["\\.git", ".*~", "node_modules"]);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_stow_with_ignore_combination() {
        let args = Args::parse_from([
            "rustow",
            "-S",
            "--ignore=\\.git",
            "--ignore=temp",
            "mypackage",
        ]);
        assert!(args.stow);
        assert_eq!(args.ignore_patterns, vec!["\\.git", "temp"]);
        assert_eq!(args.packages, vec!["mypackage"]);
    }
}
