use clap::Parser;
use std::ffi::OsString;
use std::path::PathBuf;
use std::{env, fs, io::ErrorKind, path::Path};

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
    pub fn try_parse_from<I, T>(itr: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        let merged = merge_stowrc_args(&argv)?;
        parse_args_from_argv(&merged)
    }

    pub fn parse_from<I, T>(itr: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        Self::try_parse_from(argv).unwrap_or_else(|err| err.exit())
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
        let merged = merge_stowrc_args(&argv)?;
        let args = parse_args_from_argv(&merged)?;
        let operation_groups = parse_operation_groups(&argv);

        Ok(ParsedArgs {
            args,
            operation_groups,
        })
    }
}

fn parse_args_from_argv(argv: &[OsString]) -> Result<Args, clap::Error> {
    if let Some(help_or_version) = help_or_version_arg(argv) {
        let program = argv
            .first()
            .cloned()
            .unwrap_or_else(|| OsString::from("rustow"));
        return <Args as Parser>::try_parse_from([program, help_or_version]);
    }

    validate_separate_option_values(argv)?;
    let verbose = parse_verbose_level(argv)?;
    let mut args = <Args as Parser>::try_parse_from(normalize_verbose_args(argv))?;
    args.verbose = verbose;
    Ok(args)
}

fn merge_stowrc_args(argv: &[OsString]) -> Result<Vec<OsString>, clap::Error> {
    let mut merged_argv = Vec::new();
    if argv.is_empty() {
        return Ok(merged_argv);
    }

    let mut stowrc_args = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        let local_path = current_dir.join(".stowrc");
        let local_entries = read_stowrc_file(&local_path)?;
        stowrc_args.extend(local_entries);
    }

    if let Some(home_dir) = dirs::home_dir() {
        let home_path = home_dir.join(".stowrc");
        let home_entries = read_stowrc_file(&home_path)?;
        stowrc_args.extend(home_entries);
    }

    let merged_resource_args = normalize_stowrc_tokens(stowrc_args)?;

    merged_argv.push(argv[0].clone());
    merged_argv.extend(merged_resource_args);
    merged_argv.extend(argv.iter().skip(1).cloned());
    Ok(merged_argv)
}

fn read_stowrc_file(path: &Path) -> Result<Vec<String>, clap::Error> {
    let contents = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::Io,
                format!("failed to read resource file '{}': {}", path.display(), err),
            ));
        },
    };

    let mut tokens = Vec::new();
    for line in contents.lines() {
        tokens.extend(tokenize_stowrc_line(line));
    }

    Ok(tokens)
}

#[derive(Debug, Clone, Copy)]
enum ResourceValueOption {
    Dir,
    Target,
    Ignore,
    Defer,
    Override,
}

impl ResourceValueOption {
    fn is_path_option(self) -> bool {
        matches!(self, Self::Dir | Self::Target)
    }
}

fn normalize_stowrc_tokens(raw_tokens: Vec<String>) -> Result<Vec<OsString>, clap::Error> {
    let mut normalized = Vec::new();
    let mut expecting_value: Option<ResourceValueOption> = None;

    for token in raw_tokens {
        if let Some(option_name) = expecting_value {
            if option_name.is_path_option() {
                normalized.push(expand_path_value(&token).into());
            } else {
                normalized.push(OsString::from(token));
            }
            expecting_value = None;
            continue;
        }

        if token == "--" {
            // Packages are ignored in resource files.
            continue;
        }

        if token.is_empty() || (!token.starts_with('-')) {
            // Treated as package names in resource files.
            continue;
        }

        if token.starts_with("--") {
            if let Some((emit_token, expecting)) = parse_long_stowrc_option(&token) {
                if let Some(option_name) = expecting {
                    normalized.push(emit_token);
                    expecting_value = Some(option_name);
                } else {
                    normalized.push(emit_token);
                }
            }

            continue;
        }

        if token.len() > 1 {
            if let Some((emit_tokens, expecting)) = parse_short_stowrc_option(&token) {
                normalized.extend(emit_tokens);
                expecting_value = expecting;
            }
        }
    }

    Ok(normalized)
}

fn parse_long_stowrc_option(token: &str) -> Option<(OsString, Option<ResourceValueOption>)> {
    let token = token.trim();
    let long_token = token.trim_start_matches("--");
    if long_token.is_empty() {
        return None;
    }

    if long_token == "stow" || long_token == "delete" || long_token == "restow" {
        return None;
    }

    if let Some((key, value)) = long_token.split_once('=') {
        if let Some(option_name) = map_long_value_option(key) {
            if option_name.is_path_option() {
                return Some((
                    OsString::from(format!("--{}={}", key, expand_path_value(value))),
                    None,
                ));
            }
            return Some((OsString::from(token), None));
        }

        return Some((OsString::from(token), None));
    }

    if let Some(option_name) = map_long_value_option(long_token) {
        return Some((OsString::from(token), Some(option_name)));
    }

    if map_long_bool_option(long_token) {
        return Some((OsString::from(token), None));
    }

    Some((OsString::from(token), None))
}

fn map_long_bool_option(token: &str) -> bool {
    matches!(
        token,
        "compat"
            | "adopt"
            | "no-folding"
            | "dotfiles"
            | "simulate"
            | "verbose"
            | "help"
            | "version"
            | "no"
    )
}

fn map_long_value_option(token: &str) -> Option<ResourceValueOption> {
    match token {
        "target" => Some(ResourceValueOption::Target),
        "dir" => Some(ResourceValueOption::Dir),
        "ignore" => Some(ResourceValueOption::Ignore),
        "defer" => Some(ResourceValueOption::Defer),
        "override" => Some(ResourceValueOption::Override),
        _ => None,
    }
}

fn parse_short_stowrc_option(token: &str) -> Option<(Vec<OsString>, Option<ResourceValueOption>)> {
    let chars: Vec<char> = token.chars().skip(1).collect();
    if chars.is_empty() {
        return None;
    }

    let mut bool_flags: Vec<char> = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            'S' | 'D' | 'R' => {
                i += 1;
                continue;
            },
            't' | 'd' => {
                let option_name = match chars[i] {
                    't' => ResourceValueOption::Target,
                    'd' => ResourceValueOption::Dir,
                    _ => unreachable!(),
                };
                let token_remainder = chars.iter().skip(i + 1).collect::<String>();
                if token_remainder.is_empty() {
                    if bool_flags.is_empty() {
                        return Some((vec![format!("-{}", chars[i]).into()], Some(option_name)));
                    }
                    return Some((
                        vec![
                            format!("-{}{}", bool_flags.iter().collect::<String>(), chars[i])
                                .into(),
                        ],
                        Some(option_name),
                    ));
                }

                let prefix = if bool_flags.is_empty() {
                    format!("-{}", chars[i])
                } else {
                    format!("-{}{}", bool_flags.iter().collect::<String>(), chars[i])
                };
                return Some((
                    vec![OsString::from(format!(
                        "{}{}",
                        prefix,
                        expand_path_value(&token_remainder)
                    ))],
                    None,
                ));
            },
            'p' | 'v' | 'h' | 'V' | 'n' => {
                bool_flags.push(chars[i]);
                i += 1;
            },
            other if other.is_ascii_alphabetic() => {
                return Some((vec![OsString::from(token)], None));
            },
            _ => return Some((vec![OsString::from(token)], None)),
        }
    }

    if bool_flags.is_empty() {
        None
    } else {
        Some((
            vec![format!("-{}", bool_flags.iter().collect::<String>()).into()],
            None,
        ))
    }
}

fn tokenize_stowrc_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '#' && !in_single_quote && !in_double_quote {
            break;
        }

        if ch == '\\' && !in_single_quote {
            if let Some(next) = chars.next() {
                if next.is_whitespace() {
                    token.push(next);
                    continue;
                }

                if next == '$' || next == '~' {
                    token.push('\\');
                    token.push(next);
                    continue;
                }

                token.push(next);
                continue;
            }
            token.push('\\');
            continue;
        }

        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            continue;
        }

        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            continue;
        }

        if ch.is_whitespace() && !in_single_quote && !in_double_quote {
            if !token.is_empty() {
                tokens.push(token.clone());
                token.clear();
            }
            continue;
        }

        token.push(ch);
    }

    if !token.is_empty() {
        tokens.push(token);
    }

    tokens
}

fn expand_path_value(raw_value: &str) -> String {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from(""));
    let mut output = String::new();
    let mut chars = raw_value.chars().peekable();
    let mut is_start = true;

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                if next == '$' || next == '~' {
                    output.push(next);
                } else {
                    output.push('\\');
                    output.push(next);
                }
            } else {
                output.push('\\');
            }
            is_start = false;
            continue;
        }

        if ch == '~' && is_start && !home_dir.as_os_str().is_empty() {
            output.push_str(&home_dir.to_string_lossy());
            is_start = false;
            continue;
        }

        if ch == '$' {
            if let Some('{') = chars.peek().copied() {
                chars.next();
                let mut variable = String::new();
                for next in chars.by_ref() {
                    if next == '}' {
                        break;
                    }
                    variable.push(next);
                }
                if let Ok(value) = env::var(&variable) {
                    output.push_str(&value);
                }
                is_start = false;
                continue;
            }

            if let Some(next) = chars.peek().copied() {
                if is_valid_var_start(next) {
                    let mut variable = String::new();
                    variable.push(chars.next().expect("peeked variable start"));
                    while let Some(next_char) = chars.peek() {
                        if is_valid_var_char(*next_char) {
                            variable.push(*next_char);
                            chars.next();
                        } else {
                            break;
                        }
                    }

                    if let Ok(value) = env::var(&variable) {
                        output.push_str(&value);
                    }
                    is_start = false;
                    continue;
                }
            }

            output.push('$');
            is_start = false;
            continue;
        }

        output.push(ch);
        is_start = false;
    }

    output
}

fn is_valid_var_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_valid_var_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
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
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;

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

    struct EnvVarGuard {
        key: &'static str,
        original_value: Option<String>,
    }

    impl EnvVarGuard {
        fn new(key: &'static str, value: &str) -> Self {
            let original_value = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                original_value,
            }
        }
    }

    struct CurrentDirGuard {
        original: std::path::PathBuf,
    }

    impl CurrentDirGuard {
        fn set(path: &std::path::Path) -> Self {
            let original = std::env::current_dir().expect("current dir should be obtainable");
            std::env::set_current_dir(path).expect("failed to switch current directory");
            Self { original }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.original).expect("failed to restore current directory");
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original_value {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    fn write_file(path: &std::path::Path, content: &str) -> std::path::PathBuf {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path.to_path_buf()
    }

    impl Drop for StowDirEnvGuard {
        fn drop(&mut self) {
            // Restore original value if it existed, otherwise ensure it's cleared
            match &self.original_value {
                Some(value) => unsafe { std::env::set_var("STOW_DIR", value) },
                None => unsafe { std::env::remove_var("STOW_DIR") },
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

        let error = Args::try_parse_from(["rustow", "--defer", "--help", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("requires a value"));

        let error = Args::try_parse_from(["rustow", "--override", "-V", "mypackage"]).unwrap_err();
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
    fn test_hyphen_prefixed_option_values_for_all_pattern_lists() {
        let args = Args::parse_from([
            "rustow",
            "--ignore=--help",
            "--override=--verbose=0",
            "--defer=--version",
            "--target=--verbose",
            "mypackage",
        ]);

        assert_eq!(args.ignore_patterns, vec!["--help"]);
        assert_eq!(args.override_conflicts, vec!["--verbose=0"]);
        assert_eq!(args.defer_conflicts, vec!["--version"]);
        assert_eq!(args.target, Some(PathBuf::from("--verbose")));
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
    fn test_short_cluster_mode_before_dir_value() {
        let parsed_args = Args::parse_from_with_operation_groups(["rustow", "-Sd/tmp/stow", "pkg"]);

        assert_eq!(
            parsed_args.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Stow,
                packages: vec!["pkg".to_string()],
            }]
        );
        assert_eq!(parsed_args.args.dir, Some(PathBuf::from("/tmp/stow")));
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

    #[test]
    fn test_stowrc_options_from_current_and_home_are_prepared() {
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&home_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();

        let home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            "--dir=/local\n--ignore=local\n--target=/local_target\n",
        );
        write_file(
            &home_dir.join(".stowrc"),
            "--dir=~/.stowrc_home_dir\n--ignore=home\n--target=~/home_target\n",
        );

        let args = Args::parse_from(["rustow", "my-package"]);
        assert_eq!(
            args.dir,
            Some(PathBuf::from(format!(
                "{}/.stowrc_home_dir",
                home_dir.to_string_lossy()
            )))
        );
        assert_eq!(
            args.target,
            Some(PathBuf::from(format!(
                "{}/home_target",
                home_dir.to_string_lossy()
            )))
        );
        assert_eq!(args.ignore_patterns, vec!["local", "home"]);
        assert_eq!(args.packages, vec!["my-package"]);

        let args_override = Args::parse_from(["rustow", "--dir", "/cli-dir", "my-package"]);
        assert_eq!(args_override.dir, Some(PathBuf::from("/cli-dir")));
        drop(home_guard);
    }

    #[test]
    fn test_stowrc_ignores_mode_flags_and_package_names() {
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "-D\npkg-from-rc\n--ignore=from-rc\n");

        let args = Args::parse_from(["rustow", "cli-pkg"]);
        assert!(!args.delete);
        assert_eq!(args.ignore_patterns, vec!["from-rc"]);
        assert_eq!(args.packages, vec!["cli-pkg"]);

        let grouped = Args::parse_from_with_operation_groups(["rustow", "cli-pkg"]);
        assert_eq!(
            grouped.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Stow,
                packages: vec!["cli-pkg".to_string()],
            }]
        );
    }

    #[test]
    fn test_stowrc_expands_path_tokens_in_env_and_tilde() {
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        fs::create_dir_all(&home_dir).unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();

        let home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            "--dir=\"$HOME/.stowrc dir\"\n--target=~/.stowrc_target\n",
        );

        let args = Args::parse_from(["rustow", "pkg"]);
        assert_eq!(
            args.dir,
            Some(PathBuf::from(format!(
                "{}/.stowrc dir",
                home_dir.to_string_lossy()
            )))
        );
        assert_eq!(
            args.target,
            Some(PathBuf::from(format!(
                "{}/.stowrc_target",
                home_dir.to_string_lossy()
            )))
        );

        write_file(&cwd.join(".stowrc"), "--dir=\\$HOME/.stowrc_noparse\n");
        let with_escaped_home = Args::parse_from(["rustow", "pkg"]);
        assert_eq!(
            with_escaped_home.dir,
            Some(PathBuf::from("$HOME/.stowrc_noparse"))
        );
        drop(home_guard);
    }

    #[test]
    fn test_stowrc_tokenization_handles_quotes_and_comments() {
        assert_eq!(
            tokenize_stowrc_line(r#"--ignore='one # two' "three four" # comment"#),
            vec!["--ignore=one # two".to_string(), "three four".to_string()]
        );
    }

    #[test]
    fn test_stowrc_tokenization_supports_escaped_hash_character() {
        assert_eq!(
            tokenize_stowrc_line(r#"--ignore=a\#b"#),
            vec!["--ignore=a#b".to_string()]
        );
    }

    #[test]
    fn test_expand_path_value_supports_braced_and_unbraced_env_vars() {
        let _home_guard = EnvVarGuard::new("HOME_STOW", "/home/example");

        assert_eq!(
            expand_path_value(r"$HOME_STOW/.stowrc"),
            "/home/example/.stowrc".to_string()
        );
        assert_eq!(
            expand_path_value(r"${HOME_STOW}/nested/${HOME_STOW}"),
            "/home/example/nested//home/example".to_string()
        );
    }

    #[test]
    fn test_expand_path_value_preserves_escaped_markers() {
        let _home_guard = EnvVarGuard::new("HOME_STOW", "/home/example");

        assert_eq!(
            expand_path_value(r"\$HOME_STOW/keep-this"),
            "$HOME_STOW/keep-this".to_string()
        );
        assert_eq!(
            expand_path_value(r"\~/.keep-this"),
            "~/.keep-this".to_string()
        );
        assert_eq!(
            expand_path_value(r"\${HOME_STOW}/path"),
            "${HOME_STOW}/path".to_string()
        );
    }

    #[test]
    fn test_stowrc_short_option_cluster_parsing_expands_attached_path_values() {
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        fs::create_dir_all(&home_dir).unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();

        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "-d$HOME/d\n-t${HOME}/t\n");

        let args = Args::parse_from(["rustow", "pkg"]);
        assert_eq!(
            args.dir,
            Some(PathBuf::from(format!("{}/d", home_dir.to_string_lossy())))
        );
        assert_eq!(
            args.target,
            Some(PathBuf::from(format!("{}/t", home_dir.to_string_lossy())))
        );
    }
}
