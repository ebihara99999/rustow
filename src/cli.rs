use clap::Parser;
use std::ffi::OsString;
use std::path::PathBuf;

const MAX_VERBOSITY: u8 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationMode {
    Stow,
    Delete,
    Restow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationGroup {
    pub mode: OperationMode,
    pub packages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedArgs {
    pub args: Args,
    pub operation_groups: Vec<OperationGroup>,
}

/// Rustow: A Rust implementation of GNU Stow
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None, args_override_self = true)]
pub struct Args {
    // Ensure this is pub
    /// Target directory for symlinks
    #[clap(short, long, value_parser, allow_hyphen_values = true)]
    pub target: Option<PathBuf>,

    /// Directory containing stow packages
    #[clap(short, long, value_parser, allow_hyphen_values = true)]
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

    /// Search package symlinks using GNU Stow --compat mode
    #[clap(short = 'p', long)]
    pub compat: bool,

    /// Override existing conflicting symlinks from other packages that match the regex
    #[clap(long = "override", value_parser, allow_hyphen_values = true)]
    pub override_conflicts: Vec<String>,

    /// Defer stowing files that would conflict with existing symlinks from other packages that match the regex
    #[clap(long = "defer", value_parser, allow_hyphen_values = true)]
    pub defer_conflicts: Vec<String>,

    /// Ignore files matching the specified regex pattern
    #[clap(long = "ignore", value_parser, allow_hyphen_values = true)]
    pub ignore_patterns: Vec<String>,

    /// Simulate execution, do not make any changes
    #[clap(short = 'n', long, alias = "no")]
    pub simulate: bool,

    /// Set verbosity level (repeat -v or use --verbose=LEVEL, 0-5)
    #[clap(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Packages to process
    #[clap(value_parser, required = true, num_args = 1..)]
    pub packages: Vec<String>,
}

impl Args {
    pub fn parse_from<I, T>(itr: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        Self::try_parse_from(argv).unwrap_or_else(|err| err.exit())
    }

    pub fn try_parse_from<I, T>(itr: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        if let Some(help_or_version) = help_or_version_arg(&argv) {
            let program = argv
                .first()
                .cloned()
                .unwrap_or_else(|| OsString::from("rustow"));
            return <Self as Parser>::try_parse_from([program, help_or_version]);
        }

        validate_separate_option_values(&argv)?;
        let verbose = parse_verbose_level(&argv)?;
        let mut args = <Self as Parser>::try_parse_from(normalize_verbose_args(&argv))?;
        args.verbose = verbose;
        Ok(args)
    }

    pub fn parse_with_operation_groups() -> ParsedArgs {
        Self::parse_from_with_operation_groups(std::env::args_os())
    }

    pub fn parse_from_with_operation_groups<I, T>(itr: I) -> ParsedArgs
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Self::try_parse_from_with_operation_groups(itr).unwrap_or_else(|err| err.exit())
    }

    pub fn try_parse_from_with_operation_groups<I, T>(itr: I) -> Result<ParsedArgs, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        let args = Self::try_parse_from(argv.clone())?;

        Ok(ParsedArgs {
            args,
            operation_groups: parse_operation_groups(&argv),
        })
    }
}

fn normalize_verbose_args(argv: &[OsString]) -> Vec<OsString> {
    let mut normalized_args = Vec::with_capacity(argv.len());
    let mut expecting_option_value = false;
    let mut after_double_dash = false;

    for (index, arg) in argv.iter().enumerate() {
        if index == 0 {
            normalized_args.push(arg.clone());
            continue;
        }

        if after_double_dash {
            normalized_args.push(arg.clone());
            continue;
        }

        if expecting_option_value {
            expecting_option_value = false;
            normalized_args.push(arg.clone());
            continue;
        }

        let arg_string = arg.to_string_lossy();
        if arg_string == "--" {
            after_double_dash = true;
            normalized_args.push(arg.clone());
            continue;
        }

        if is_option_requiring_separate_value(&arg_string) {
            expecting_option_value = true;
            normalized_args.push(arg.clone());
            continue;
        }

        if arg_string.starts_with('-')
            && !arg_string.starts_with("--")
            && short_option_cluster_consumes_value(&arg_string, &mut OperationMode::Stow)
        {
            expecting_option_value = short_option_cluster_needs_next_value(&arg_string);
        }

        normalized_args.extend(normalize_verbose_arg(arg).unwrap_or_else(|| vec![arg.clone()]));
    }

    normalized_args
}

fn normalize_verbose_arg(arg: &OsString) -> Option<Vec<OsString>> {
    let arg = arg.to_str()?;
    let level = arg.strip_prefix("--verbose=")?;
    let level = parse_numeric_verbose_level(level)?;

    if level == 0 {
        return Some(Vec::new());
    }

    Some(vec![OsString::from("-v")])
}

fn parse_numeric_verbose_level(level: &str) -> Option<u8> {
    level
        .parse::<u8>()
        .ok()
        .filter(|level| *level <= MAX_VERBOSITY)
}

fn parse_verbose_level(argv: &[OsString]) -> Result<u8, clap::Error> {
    let mut verbosity = 0;
    let mut expecting_option_value = false;
    let mut after_double_dash = false;

    for arg in argv.iter().skip(1) {
        let arg = arg.to_string_lossy();

        if after_double_dash {
            continue;
        }

        if expecting_option_value {
            expecting_option_value = false;
            continue;
        }

        if arg == "--" {
            after_double_dash = true;
            continue;
        }

        if is_option_requiring_separate_value(&arg) {
            expecting_option_value = true;
            continue;
        }

        if arg.starts_with("--verbose=") {
            let level = arg
                .strip_prefix("--verbose=")
                .and_then(parse_numeric_verbose_level)
                .ok_or_else(|| verbose_level_error(&arg))?;
            verbosity = level;
            continue;
        }

        if arg == "--verbose" {
            increment_verbose_level(&mut verbosity)?;
            continue;
        }

        if arg.starts_with("--") {
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            parse_short_verbose_cluster(&arg, &mut verbosity)?;
            if short_option_cluster_consumes_value(&arg, &mut OperationMode::Stow) {
                expecting_option_value = short_option_cluster_needs_next_value(&arg);
            }
        }
    }

    Ok(verbosity)
}

fn validate_separate_option_values(argv: &[OsString]) -> Result<(), clap::Error> {
    let mut option_waiting_for_value: Option<String> = None;
    let mut after_double_dash = false;
    let mut current_mode = OperationMode::Stow;

    for arg in argv.iter().skip(1) {
        let arg = arg.to_string_lossy();

        if after_double_dash {
            break;
        }

        if let Some(option_name) = option_waiting_for_value.take() {
            if is_reserved_flag_value(&arg) {
                return Err(missing_value_before_flag_error(&option_name, &arg));
            }
            continue;
        }

        if arg == "--" {
            after_double_dash = true;
            continue;
        }

        if is_option_requiring_separate_value(&arg) {
            option_waiting_for_value = Some(arg.into_owned());
            continue;
        }

        if arg.starts_with('-')
            && !arg.starts_with("--")
            && arg.len() > 1
            && short_option_cluster_consumes_value(&arg, &mut current_mode)
            && short_option_cluster_needs_next_value(&arg)
        {
            option_waiting_for_value = Some(arg.into_owned());
        }
    }

    Ok(())
}

fn is_reserved_flag_value(value: &str) -> bool {
    matches!(
        value,
        "-S" | "-D"
            | "-R"
            | "-p"
            | "-h"
            | "-V"
            | "-n"
            | "-v"
            | "--stow"
            | "--delete"
            | "--restow"
            | "--help"
            | "--version"
            | "--compat"
            | "--simulate"
            | "--no"
            | "--adopt"
            | "--no-folding"
            | "--dotfiles"
            | "--verbose"
    ) || value.starts_with("--verbose=")
        || is_short_flag_cluster(value)
}

fn is_short_flag_cluster(value: &str) -> bool {
    if !value.starts_with('-') || value.starts_with("--") || value.len() <= 1 {
        return false;
    }

    value[1..]
        .chars()
        .all(|flag| matches!(flag, 'S' | 'D' | 'R' | 'p' | 'h' | 'V' | 'n' | 'v'))
}

fn missing_value_before_flag_error(option_name: &str, flag: &str) -> clap::Error {
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!(
            "option '{option_name}' requires a value; '{flag}' is an operation/help/version flag. Use '{option_name}={flag}' to pass it as a literal value."
        ),
    )
}

fn help_or_version_arg(argv: &[OsString]) -> Option<OsString> {
    let mut expecting_option_value = false;
    let mut after_double_dash = false;
    let mut current_mode = OperationMode::Stow;

    for arg in argv.iter().skip(1) {
        let arg = arg.to_string_lossy();

        if after_double_dash {
            continue;
        }

        if expecting_option_value {
            expecting_option_value = false;
            continue;
        }

        if arg == "--" {
            after_double_dash = true;
            continue;
        }

        if is_option_requiring_separate_value(&arg) {
            expecting_option_value = true;
            continue;
        }

        if matches!(arg.as_ref(), "--help" | "--version") {
            return Some(OsString::from(arg.as_ref()));
        }

        if arg.starts_with("--") {
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            if short_option_cluster_consumes_value(&arg, &mut current_mode) {
                expecting_option_value = short_option_cluster_needs_next_value(&arg);
            }

            for flag in arg[1..].chars() {
                if matches!(flag, 'h' | 'V') {
                    return Some(OsString::from(format!("-{flag}")));
                }

                if short_cluster_stops_value_parsing(flag) {
                    break;
                }
            }
        }
    }

    None
}

fn parse_short_verbose_cluster(arg: &str, verbosity: &mut u8) -> Result<(), clap::Error> {
    for flag in arg[1..].chars() {
        if short_cluster_stops_value_parsing(flag) {
            break;
        }

        if flag == 'v' {
            increment_verbose_level(verbosity)?;
        }
    }

    Ok(())
}

fn short_cluster_stops_value_parsing(flag: char) -> bool {
    matches!(flag, 't' | 'd')
}

fn increment_verbose_level(verbosity: &mut u8) -> Result<(), clap::Error> {
    if *verbosity >= MAX_VERBOSITY {
        return Err(verbose_level_error("--verbose"));
    }

    *verbosity += 1;
    Ok(())
}

fn verbose_level_error(value: &str) -> clap::Error {
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!("verbosity level must be between 0 and {MAX_VERBOSITY}: {value}"),
    )
}

fn parse_operation_groups(argv: &[OsString]) -> Vec<OperationGroup> {
    let mut groups: Vec<OperationGroup> = Vec::new();
    let mut current_mode = OperationMode::Stow;
    let mut expecting_option_value = false;
    let mut after_double_dash = false;

    for arg in argv.iter().skip(1) {
        let arg = arg.to_string_lossy();

        if expecting_option_value {
            expecting_option_value = false;
            continue;
        }

        if after_double_dash {
            push_package_operation(&mut groups, current_mode.clone(), arg.into_owned());
            continue;
        }

        if arg == "--" {
            after_double_dash = true;
            continue;
        }

        if is_option_requiring_separate_value(&arg) {
            expecting_option_value = true;
            continue;
        }

        if arg.starts_with("--") {
            if let Some(mode) = long_operation_mode(&arg) {
                current_mode = mode;
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            if short_option_cluster_consumes_value(&arg, &mut current_mode) {
                expecting_option_value = short_option_cluster_needs_next_value(&arg);
            }
            continue;
        }

        push_package_operation(&mut groups, current_mode.clone(), arg.into_owned());
    }

    groups
}

fn push_package_operation(groups: &mut Vec<OperationGroup>, mode: OperationMode, package: String) {
    if let Some(last_group) = groups.last_mut() {
        if last_group.mode == mode {
            last_group.packages.push(package);
            return;
        }
    }

    groups.push(OperationGroup {
        mode,
        packages: vec![package],
    });
}

fn long_operation_mode(arg: &str) -> Option<OperationMode> {
    match arg {
        "--stow" => Some(OperationMode::Stow),
        "--delete" => Some(OperationMode::Delete),
        "--restow" => Some(OperationMode::Restow),
        _ => None,
    }
}

fn short_operation_mode(flag: char) -> Option<OperationMode> {
    match flag {
        'S' => Some(OperationMode::Stow),
        'D' => Some(OperationMode::Delete),
        'R' => Some(OperationMode::Restow),
        _ => None,
    }
}

fn is_option_requiring_separate_value(arg: &str) -> bool {
    matches!(
        arg,
        "-t" | "--target" | "-d" | "--dir" | "--override" | "--defer" | "--ignore"
    )
}

fn short_option_cluster_consumes_value(arg: &str, current_mode: &mut OperationMode) -> bool {
    for flag in arg[1..].chars() {
        if matches!(flag, 't' | 'd') {
            return true;
        }

        if let Some(mode) = short_operation_mode(flag) {
            *current_mode = mode;
        }
    }

    false
}

fn short_option_cluster_needs_next_value(arg: &str) -> bool {
    let mut flags = arg[1..].chars().peekable();

    while let Some(flag) = flags.next() {
        if matches!(flag, 't' | 'd') {
            return flags.peek().is_none();
        }
    }

    false
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
        let args_long = Args::parse_from(["rustow", "--verbose", "mypackage"]);
        assert_eq!(args_long.verbose, 1);
        let args_long_level = Args::parse_from(["rustow", "--verbose=2", "mypackage"]);
        assert_eq!(args_long_level.verbose, 2);
        let args_long_zero = Args::parse_from(["rustow", "--verbose=0", "mypackage"]);
        assert_eq!(args_long_zero.verbose, 0);
        assert!(Args::try_parse_from(["rustow", "--verbose=invalid", "mypackage"]).is_err());
        assert!(Args::try_parse_from(["rustow", "--verbose=6", "mypackage"]).is_err());
    }

    #[test]
    fn test_verbose_option_cluster_with_compat_before() {
        let args = Args::parse_from(["rustow", "-pv", "mypackage"]);

        assert!(args.compat);
        assert_eq!(args.verbose, 1);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_verbose_option_cluster_with_compat_after() {
        let args = Args::parse_from(["rustow", "-vp", "mypackage"]);

        assert!(args.compat);
        assert_eq!(args.verbose, 1);
    }

    #[test]
    fn test_verbose_numeric_out_of_range_reports_range() {
        let error = Args::try_parse_from(["rustow", "--verbose=6", "mypackage"]).unwrap_err();

        assert!(error.to_string().contains("between 0 and 5"));
    }

    #[test]
    fn test_hyphen_prefixed_option_values_are_preserved() {
        let args = Args::parse_from([
            "rustow",
            "--dir=--verbose=0",
            "--ignore=--verbose=1",
            "--target=--verbose",
            "mypackage",
        ]);

        assert_eq!(args.dir, Some(PathBuf::from("--verbose=0")));
        assert_eq!(args.ignore_patterns, vec!["--verbose=1"]);
        assert_eq!(args.target, Some(PathBuf::from("--verbose")));
        assert_eq!(args.verbose, 0);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_operation_flags_are_rejected_as_missing_separate_option_values() {
        let error = Args::try_parse_from(["rustow", "-d", "-D", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error =
            Args::try_parse_from(["rustow", "--target", "--restow", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "--ignore", "-S", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "-d", "--simulate", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "-d", "-n", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error =
            Args::try_parse_from(["rustow", "--target", "--adopt", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "-d", "--verbose", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error =
            Args::try_parse_from(["rustow", "--target", "--verbose", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error =
            Args::try_parse_from(["rustow", "--defer", "--verbose", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "-Dt", "--verbose", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "-Dt", "--help", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "-Sd", "-n", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "-Rt", "--simulate", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));
    }

    #[test]
    fn test_reserved_flags_can_be_passed_as_explicit_hyphen_values() {
        let args = Args::parse_from([
            "rustow",
            "--dir=-D",
            "--ignore=-S",
            "--defer=--simulate",
            "mypackage",
        ]);

        assert_eq!(args.dir, Some(PathBuf::from("-D")));
        assert_eq!(args.ignore_patterns, vec!["-S"]);
        assert_eq!(args.defer_conflicts, vec!["--simulate"]);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_help_takes_precedence_over_invalid_verbose() {
        let error = Args::try_parse_from(["rustow", "--help", "--verbose=6"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn test_help_takes_precedence_after_packages() {
        let error = Args::try_parse_from(["rustow", "mypackage", "--help"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);

        let error = Args::try_parse_from(["rustow", "mypackage", "-h"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);

        let error = Args::try_parse_from(["rustow", "mypackage", "-V"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn test_try_parse_from_with_operation_groups_returns_parse_errors() {
        let error =
            Args::try_parse_from_with_operation_groups(["rustow", "--verbose=6", "mypackage"])
                .unwrap_err();

        assert!(error.to_string().contains("between 0 and 5"));
    }

    #[test]
    fn test_verbose_numeric_option_sets_level_in_argument_order() {
        let args = Args::parse_from(["rustow", "-v", "--verbose=0", "mypackage"]);
        assert_eq!(args.verbose, 0);

        let args = Args::parse_from(["rustow", "--verbose=2", "-v", "mypackage"]);
        assert_eq!(args.verbose, 3);

        let args = Args::parse_from(["rustow", "--verbose=2", "--verbose=1", "mypackage"]);
        assert_eq!(args.verbose, 1);
    }

    #[test]
    fn test_verbose_numeric_option_after_double_dash_is_package() {
        let args = Args::parse_from(["rustow", "--", "--verbose=0"]);
        assert_eq!(args.verbose, 0);
        assert_eq!(args.packages, vec!["--verbose=0"]);
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
            "--compat",
            "mypackage",
        ]);
        assert!(args.adopt);
        assert!(args.no_folding);
        assert!(args.dotfiles);
        assert!(args.compat);
    }

    #[test]
    fn test_compat_option_is_parsed() {
        let args = Args::parse_from(["rustow", "--compat", "mypackage"]);
        assert!(args.compat);

        let args = Args::parse_from(["rustow", "-p", "mypackage"]);
        assert!(args.compat);
    }

    #[test]
    fn test_stow_dir_from_env() {
        // STOW_DIR is resolved by Config, not by the CLI parser.
        let _guard = StowDirEnvGuard::new(); // Ensure STOW_DIR is clear initially

        // Set STOW_DIR environment variable
        unsafe {
            std::env::set_var("STOW_DIR", "/env/stow/path");
        }

        let args = Args::parse_from(["rustow", "mypackage"]);
        assert!(args.dir.is_none());
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
    fn test_parse_operation_groups_mixed_modes() {
        let parsed_args = Args::parse_from_with_operation_groups([
            "rustow", "-D", "old", "-S", "new", "--restow", "refresh",
        ]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![
                OperationGroup {
                    mode: OperationMode::Delete,
                    packages: vec!["old".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Stow,
                    packages: vec!["new".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Restow,
                    packages: vec!["refresh".to_string()],
                },
            ]
        );
    }

    #[test]
    fn test_parse_operation_groups_defaults_to_stow_until_mode_changes() {
        let parsed_args = Args::parse_from_with_operation_groups([
            "rustow", "base", "-D", "old1", "old2", "-S", "new1", "new2",
        ]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![
                OperationGroup {
                    mode: OperationMode::Stow,
                    packages: vec!["base".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Delete,
                    packages: vec!["old1".to_string(), "old2".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Stow,
                    packages: vec!["new1".to_string(), "new2".to_string()],
                },
            ]
        );
    }

    #[test]
    fn test_parse_operation_groups_treats_args_after_double_dash_as_packages() {
        let parsed_args = Args::parse_from_with_operation_groups(["rustow", "--", "-D", "old"]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Stow,
                packages: vec!["-D".to_string(), "old".to_string()],
            }]
        );
    }

    #[test]
    fn test_parse_operation_groups_keeps_current_mode_after_double_dash() {
        let parsed_args =
            Args::parse_from_with_operation_groups(["rustow", "-D", "old", "--", "-S", "new"]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Delete,
                packages: vec!["old".to_string(), "-S".to_string(), "new".to_string()],
            }]
        );
    }

    #[test]
    fn test_parse_operation_groups_skips_clustered_short_option_values() {
        let parsed_args =
            Args::parse_from_with_operation_groups(["rustow", "-Dt", "/tmp/target", "old"]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Delete,
                packages: vec!["old".to_string()],
            }]
        );
    }

    #[test]
    fn test_short_cluster_with_value_flag_stops_verbosity_counting() {
        let args = Args::parse_from(["rustow", "-tv", "mypackage"]);
        assert_eq!(args.target, Some(PathBuf::from("v")));
        assert_eq!(args.verbose, 0);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_short_cluster_mode_value_attached_to_target() {
        let parsed_args = Args::parse_from_with_operation_groups(["rustow", "-Dt/tmp/stow", "pkg"]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Delete,
                packages: vec!["pkg".to_string()],
            }]
        );
        assert_eq!(parsed_args.args.target, Some(PathBuf::from("/tmp/stow")));
    }

    #[test]
    fn test_parse_operation_groups_skips_attached_short_option_values() {
        let parsed_args = Args::parse_from_with_operation_groups([
            "rustow",
            "-Dtdir",
            "old",
            "-Sd/tmp/stow",
            "new",
        ]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![
                OperationGroup {
                    mode: OperationMode::Delete,
                    packages: vec!["old".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Stow,
                    packages: vec!["new".to_string()],
                },
            ]
        );
    }

    #[test]
    fn test_parse_operation_groups_allows_repeated_modes_after_switching() {
        let parsed_args = Args::parse_from_with_operation_groups([
            "rustow", "-D", "old", "-S", "new", "-D", "older",
        ]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![
                OperationGroup {
                    mode: OperationMode::Delete,
                    packages: vec!["old".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Stow,
                    packages: vec!["new".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Delete,
                    packages: vec!["older".to_string()],
                },
            ]
        );
    }

    #[test]
    fn test_parse_operation_groups_allows_repeated_long_modes_after_switching() {
        let parsed_args = Args::parse_from_with_operation_groups([
            "rustow", "--delete", "old", "--stow", "new", "--delete", "older",
        ]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![
                OperationGroup {
                    mode: OperationMode::Delete,
                    packages: vec!["old".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Stow,
                    packages: vec!["new".to_string()],
                },
                OperationGroup {
                    mode: OperationMode::Delete,
                    packages: vec!["older".to_string()],
                },
            ]
        );
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
