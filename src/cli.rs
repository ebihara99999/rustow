use clap::{Parser, builder::TypedValueParser};
use std::ffi::OsString;
#[cfg(unix)]
use std::ffi::{CStr, CString};
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::{env, fs, io::ErrorKind, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Clone, Default)]
#[doc(hidden)]
pub(crate) struct PathDisplayOverride {
    pub(crate) path: PathBuf,
    pub(crate) display: String,
}

impl PathDisplayOverride {
    pub(crate) fn new(path: PathBuf, display: String) -> Self {
        Self { path, display }
    }
}

impl std::fmt::Debug for PathDisplayOverride {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathDisplayOverride")
            .field("display", &self.display)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
#[doc(hidden)]
pub struct RuntimeParsedArgs {
    parsed_args: ParsedArgs,
    path_displays: Vec<PathDisplayOverride>,
}

impl RuntimeParsedArgs {
    #[allow(dead_code)]
    pub(crate) fn into_parts(self) -> (ParsedArgs, Vec<PathDisplayOverride>) {
        (self.parsed_args, self.path_displays)
    }
}

impl std::fmt::Debug for RuntimeParsedArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeParsedArgs")
            .field("operation_groups", &self.parsed_args.operation_groups)
            .field("path_displays", &self.path_displays)
            .finish()
    }
}

/// Rustow: A Rust implementation of GNU Stow
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None, args_override_self = true)]
pub struct Args {
    // Ensure this is pub
    /// Target directory for symlinks
    #[clap(
        short,
        long,
        value_parser = clap::builder::OsStringValueParser::new().map(PathBuf::from),
        allow_hyphen_values = true
    )]
    pub target: Option<PathBuf>,

    /// Directory containing stow packages
    #[clap(
        short,
        long,
        value_parser = clap::builder::OsStringValueParser::new().map(PathBuf::from),
        allow_hyphen_values = true
    )]
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

    /// Set verbosity level (repeat -v or use --verbose=LEVEL)
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
        parse_args_from_argv(&argv)
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

    #[doc(hidden)]
    pub fn parse_runtime_with_operation_groups() -> RuntimeParsedArgs {
        Self::parse_runtime_from_with_operation_groups(std::env::args_os())
    }

    /// Parses only the supplied argv and reconstructs operation groups.
    ///
    /// This public parser intentionally does not read `.stowrc`; the binary
    /// uses the hidden runtime parser so resource-file diagnostics can carry
    /// redacted display metadata.
    pub fn parse_from_with_operation_groups<I, T>(itr: I) -> ParsedArgs
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Self::try_parse_from_with_operation_groups(itr).unwrap_or_else(|err| err.exit())
    }

    #[doc(hidden)]
    pub fn parse_runtime_from_with_operation_groups<I, T>(itr: I) -> RuntimeParsedArgs
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Self::try_parse_runtime_from_with_operation_groups(itr).unwrap_or_else(|err| err.exit())
    }

    pub fn try_parse_from_with_operation_groups<I, T>(itr: I) -> Result<ParsedArgs, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        let args = parse_args_from_argv(&argv)?;
        let operation_groups = parse_operation_groups(&argv);

        Ok(ParsedArgs {
            args,
            operation_groups,
        })
    }

    #[doc(hidden)]
    pub fn try_parse_runtime_from_with_operation_groups<I, T>(
        itr: I,
    ) -> Result<RuntimeParsedArgs, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        if help_or_version_arg(&argv).is_some() {
            // Clap returns DisplayHelp/DisplayVersion errors here; keeping this
            // branch before .stowrc loading preserves GNU-style precedence.
            let args = parse_args_from_argv(&argv)?;
            return Ok(RuntimeParsedArgs {
                parsed_args: ParsedArgs {
                    args,
                    operation_groups: Vec::new(),
                },
                path_displays: Vec::new(),
            });
        }
        validate_cli_args_before_resource_files(&argv)?;
        let merged = merge_stowrc_args(&argv)?;
        let args = parse_args_from_argv(&merged.argv)?;
        let path_displays = effective_path_displays(merged.path_displays, &argv);
        let operation_groups = parse_operation_groups(&argv);

        Ok(RuntimeParsedArgs {
            parsed_args: ParsedArgs {
                args,
                operation_groups,
            },
            path_displays,
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

    validate_cli_option_tokens(argv)?;
    let verbose = parse_verbose_level(argv)?;
    let mut args = <Args as Parser>::try_parse_from(normalize_verbose_args(argv)?)?;
    args.verbose = verbose;
    Ok(args)
}

fn validate_cli_args_before_resource_files(argv: &[OsString]) -> Result<(), clap::Error> {
    match parse_args_from_argv(argv) {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::MissingRequiredArgument
            ) =>
        {
            Ok(())
        },
        Err(error) => Err(error),
    }
}

fn validate_cli_option_tokens(argv: &[OsString]) -> Result<(), clap::Error> {
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

        if let Some(long_token) = arg.strip_prefix("--") {
            let (key, has_attached_value) = long_token
                .split_once('=')
                .map_or((long_token, false), |(key, _)| (key, true));
            let spec = match resolve_long_option(key) {
                Ok(spec) => spec,
                Err(LongOptionResolveError::Ambiguous) => {
                    return Err(ambiguous_long_option_error(key));
                },
                Err(LongOptionResolveError::Unknown) => return Err(unknown_long_option_error(key)),
            };
            match spec.kind {
                LongOptionKind::Value(_) => {
                    expecting_option_value = !has_attached_value;
                },
                LongOptionKind::Verbose => {},
                LongOptionKind::Bool
                | LongOptionKind::Mode(_)
                | LongOptionKind::Help
                | LongOptionKind::Version => {
                    if has_attached_value {
                        return Err(long_option_value_error(key));
                    }
                },
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            let cluster = &arg[1..];
            for (index, flag) in cluster.char_indices() {
                if !is_known_short_option(flag) {
                    return Err(unknown_option_error(&format!("-{flag}")));
                }
                if flag == 'v' {
                    let rest = &cluster[index + flag.len_utf8()..];
                    if starts_with_verbose_numeric_value(rest) {
                        break;
                    }
                }
                if matches!(flag, 't' | 'd') {
                    expecting_option_value = index + flag.len_utf8() == cluster.len();
                    break;
                }
            }
        }
    }

    Ok(())
}

fn unknown_option_error(option: &str) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    clap::Error::raw(
        clap::error::ErrorKind::UnknownArgument,
        format!("unexpected argument '{option}'\n\n{usage}"),
    )
}

fn unknown_long_option_error(option: &str) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    clap::Error::raw(
        clap::error::ErrorKind::UnknownArgument,
        format!("Unknown option: {option}\n\n{usage}"),
    )
}

fn long_option_value_error(option: &str) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!("Option {option} does not take a value\n\n{usage}"),
    )
}

fn ambiguous_long_option_error(option: &str) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    let candidates = long_option_abbreviation_candidates(option).join(", ");
    clap::Error::raw(
        clap::error::ErrorKind::UnknownArgument,
        format!("Option {option} is ambiguous ({candidates})\n\n{usage}"),
    )
}

fn unknown_stowrc_long_option_error(option: &str, origin: &StowrcTokenOrigin) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    clap::Error::raw(
        clap::error::ErrorKind::UnknownArgument,
        format!(
            "Unknown option: {option} in '{}:{}'\n\n{usage}",
            origin.display_path, origin.line
        ),
    )
}

fn ambiguous_stowrc_long_option_error(option: &str, origin: &StowrcTokenOrigin) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    let candidates = long_option_abbreviation_candidates(option).join(", ");
    clap::Error::raw(
        clap::error::ErrorKind::UnknownArgument,
        format!(
            "Option {option} is ambiguous ({candidates}) in '{}:{}'\n\n{usage}",
            origin.display_path, origin.line
        ),
    )
}

fn stowrc_long_option_value_error(option: &str, origin: &StowrcTokenOrigin) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!(
            "Option {option} does not take a value in '{}:{}'\n\n{usage}",
            origin.display_path, origin.line
        ),
    )
}

fn unknown_stowrc_short_option_error(option: char, origin: &StowrcTokenOrigin) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    clap::Error::raw(
        clap::error::ErrorKind::UnknownArgument,
        format!(
            "Unknown option: {option} in '{}:{}'\n\n{usage}",
            origin.display_path, origin.line
        ),
    )
}

fn stowrc_verbose_level_error(value: &str, origin: &StowrcTokenOrigin) -> clap::Error {
    let mut command = <Args as clap::CommandFactory>::command();
    let usage = command.render_usage();
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!(
            "verbosity level must be a non-negative integer: {value} in '{}:{}'\n\n{usage}",
            origin.display_path, origin.line
        ),
    )
}

#[cfg(test)]
fn is_known_long_option(option: &str) -> bool {
    resolve_long_option(option).is_ok()
}

#[derive(Debug)]
struct MergedArgs {
    argv: Vec<OsString>,
    path_displays: Vec<ResourcePathDisplay>,
}

fn merge_stowrc_args(argv: &[OsString]) -> Result<MergedArgs, clap::Error> {
    let mut merged_argv = Vec::new();
    if argv.is_empty() {
        return Ok(MergedArgs {
            argv: merged_argv,
            path_displays: Vec::new(),
        });
    }

    let mut stowrc_args = StowrcNormalizer::new();
    if let Some(home_dir) = env::var_os("HOME") {
        let home_path = home_stowrc_path(home_dir);
        read_stowrc_file(&home_path, "~/.stowrc", &mut stowrc_args)?;
    }

    if let Ok(current_dir) = env::current_dir() {
        let local_path = current_dir.join(".stowrc");
        read_stowrc_file(&local_path, "./.stowrc", &mut stowrc_args)?;
    }

    let merged_resource_args = stowrc_args.finish()?;

    merged_argv.push(argv[0].clone());
    merged_argv.extend(merged_resource_args.argv);
    merged_argv.extend(argv.iter().skip(1).cloned());
    Ok(MergedArgs {
        argv: merged_argv,
        path_displays: merged_resource_args.path_displays,
    })
}

pub(crate) fn path_display(path: &Path, overrides: &[PathDisplayOverride]) -> String {
    overrides
        .iter()
        .rev()
        .find(|override_path| override_path.path == path)
        .map(|override_path| override_path.display.clone())
        .unwrap_or_else(|| path.display().to_string())
}

#[allow(dead_code)]
pub(crate) fn path_display_with_prefix(path: &Path, overrides: &[PathDisplayOverride]) -> String {
    overrides
        .iter()
        .filter_map(|override_path| {
            if override_path.path == path {
                return Some((
                    override_path.path.as_os_str().len(),
                    override_path.display.clone(),
                ));
            }
            let suffix = path.strip_prefix(&override_path.path).ok()?;
            Some((
                override_path.path.as_os_str().len(),
                join_display_path(&override_path.display, suffix),
            ))
        })
        .max_by_key(|(prefix_len, _)| *prefix_len)
        .map(|(_, display)| display)
        .unwrap_or_else(|| path.display().to_string())
}

#[allow(dead_code)]
fn join_display_path(prefix: &str, suffix: &Path) -> String {
    if suffix.as_os_str().is_empty() {
        return prefix.to_string();
    }

    let suffix = suffix.display();
    if prefix.ends_with(std::path::MAIN_SEPARATOR) {
        format!("{prefix}{suffix}")
    } else {
        format!("{prefix}{}{suffix}", std::path::MAIN_SEPARATOR)
    }
}

fn effective_path_displays(
    resource_displays: Vec<ResourcePathDisplay>,
    cli_argv: &[OsString],
) -> Vec<PathDisplayOverride> {
    let cli_overrides = cli_path_option_overrides(cli_argv);
    resource_displays
        .into_iter()
        .filter(|display| !cli_overrides.overrides(display.option_name))
        .map(|display| PathDisplayOverride {
            path: display.path,
            display: display.display,
        })
        .collect()
}

#[derive(Debug, Default)]
struct CliPathOptionOverrides {
    dir: bool,
    target: bool,
}

impl CliPathOptionOverrides {
    fn overrides(&self, option_name: ResourceValueOption) -> bool {
        match option_name {
            ResourceValueOption::Dir => self.dir,
            ResourceValueOption::Target => self.target,
            _ => false,
        }
    }
}

fn cli_path_option_overrides(argv: &[OsString]) -> CliPathOptionOverrides {
    let mut overrides = CliPathOptionOverrides::default();
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

        if let Some(long_token) = arg.strip_prefix("--") {
            let (key, has_attached_value) = long_token
                .split_once('=')
                .map_or((long_token, false), |(key, _)| (key, true));
            match map_long_value_option(key) {
                Some(ResourceValueOption::Dir) => {
                    overrides.dir = true;
                    expecting_option_value = !has_attached_value;
                },
                Some(ResourceValueOption::Target) => {
                    overrides.target = true;
                    expecting_option_value = !has_attached_value;
                },
                Some(_) => {
                    expecting_option_value = !has_attached_value;
                },
                None => {},
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            let mut flags = arg[1..].chars().peekable();
            while let Some(flag) = flags.next() {
                match flag {
                    'd' => {
                        overrides.dir = true;
                        expecting_option_value = flags.peek().is_none();
                        break;
                    },
                    't' => {
                        overrides.target = true;
                        expecting_option_value = flags.peek().is_none();
                        break;
                    },
                    _ => {},
                }
            }
        }
    }

    overrides
}

#[derive(Debug, Clone)]
struct StowrcToken {
    value: OsString,
    origin: StowrcTokenOrigin,
}

#[derive(Debug, Clone)]
struct StowrcTokenOrigin {
    display_path: Arc<String>,
    line: usize,
}

#[derive(Debug, Clone)]
struct PendingResourceValue {
    option_name: ResourceValueOption,
    origin: StowrcTokenOrigin,
    value_prefix: String,
}

#[derive(Debug, Clone)]
struct ResourceValueExpectation {
    option_name: ResourceValueOption,
    value_prefix: String,
}

fn home_stowrc_path(home_dir: OsString) -> PathBuf {
    if home_dir.as_os_str().is_empty() {
        PathBuf::from("/.stowrc")
    } else {
        PathBuf::from(home_dir).join(".stowrc")
    }
}

fn read_stowrc_file(
    path: &Path,
    display_path: &str,
    normalizer: &mut StowrcNormalizer,
) -> Result<(), clap::Error> {
    let file = match open_stowrc_file(path, display_path)? {
        Some(file) => file,
        None => return Ok(()),
    };

    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::PermissionDenied => return Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::Io,
                format!(
                    "failed to inspect resource file '{}': {}",
                    display_path, err
                ),
            ));
        },
    };

    if !metadata.is_file() {
        return Ok(());
    }

    stowrc_tokens_from_reader(display_path, file, normalizer)
}

#[cfg(unix)]
fn open_stowrc_file(path: &Path, display_path: &str) -> Result<Option<fs::File>, clap::Error> {
    match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) if err.kind() == ErrorKind::PermissionDenied => Ok(None),
        Err(err) => Err(clap::Error::raw(
            clap::error::ErrorKind::Io,
            format!("failed to open resource file '{}': {}", display_path, err),
        )),
    }
}

#[cfg(not(unix))]
fn open_stowrc_file(path: &Path, display_path: &str) -> Result<Option<fs::File>, clap::Error> {
    match fs::File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) if err.kind() == ErrorKind::PermissionDenied => Ok(None),
        Err(err) => Err(clap::Error::raw(
            clap::error::ErrorKind::Io,
            format!("failed to open resource file '{}': {}", display_path, err),
        )),
    }
}

#[cfg(unix)]
fn stowrc_tokens_from_reader(
    display_path: &str,
    file: fs::File,
    normalizer: &mut StowrcNormalizer,
) -> Result<(), clap::Error> {
    let checkpoint = normalizer.checkpoint();
    let display_path = Arc::new(display_path.to_string());
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut line_number = 0;

    loop {
        line.clear();
        let bytes_read = match reader.read_until(b'\n', &mut line) {
            Ok(bytes_read) => bytes_read,
            Err(err) if err.kind() == ErrorKind::PermissionDenied => {
                normalizer.rewind(checkpoint);
                return Ok(());
            },
            Err(err) if err.kind() == ErrorKind::NotFound => {
                normalizer.rewind(checkpoint);
                return Ok(());
            },
            Err(err) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::Io,
                    format!("failed to read resource file '{}': {}", display_path, err),
                ));
            },
        };
        if bytes_read == 0 {
            break;
        }
        line_number += 1;

        if line.ends_with(b"\n") {
            line.pop();
            if line.ends_with(b"\r") {
                line.pop();
            }
        }

        let origin = StowrcTokenOrigin {
            display_path: display_path.clone(),
            line: line_number,
        };
        let line_checkpoint = normalizer.checkpoint();
        let mut push_error = None;
        let tokenize_result = emit_stowrc_line_tokens_bytes(&line, |value| {
            if push_error.is_some() {
                return;
            }
            if let Err(error) = normalizer.push_token(StowrcToken {
                value: OsString::from_vec(value),
                origin: origin.clone(),
            }) {
                push_error = Some(error);
            }
        });
        match tokenize_result {
            Ok(true) => {
                if let Some(error) = push_error {
                    return Err(error);
                }
            },
            Ok(false) => {
                normalizer.rewind(line_checkpoint);
            },
            Err(err) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::InvalidValue,
                    format!("{} in '{}:{}'", err, display_path, line_number),
                ));
            },
        }
    }

    normalizer.finish_file()
}

#[cfg(not(unix))]
fn stowrc_tokens_from_reader(
    display_path: &str,
    file: fs::File,
    normalizer: &mut StowrcNormalizer,
) -> Result<(), clap::Error> {
    let checkpoint = normalizer.checkpoint();
    let display_path = Arc::new(display_path.to_string());
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) if err.kind() == ErrorKind::PermissionDenied => {
                normalizer.rewind(checkpoint);
                return Ok(());
            },
            Err(err) if err.kind() == ErrorKind::NotFound => {
                normalizer.rewind(checkpoint);
                return Ok(());
            },
            Err(err) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::Io,
                    format!("failed to read resource file '{}': {}", display_path, err),
                ));
            },
        };
        let origin = StowrcTokenOrigin {
            display_path: display_path.clone(),
            line: index + 1,
        };
        let line_checkpoint = normalizer.checkpoint();
        let mut push_error = None;
        let tokenize_result = emit_stowrc_line_tokens(&line, |value| {
            if push_error.is_some() {
                return;
            }
            if let Err(error) = normalizer.push_token(StowrcToken {
                value: OsString::from(value),
                origin: origin.clone(),
            }) {
                push_error = Some(error);
            }
        });
        match tokenize_result {
            Ok(true) => {
                if let Some(error) = push_error {
                    return Err(error);
                }
            },
            Ok(false) => {
                normalizer.rewind(line_checkpoint);
            },
            Err(err) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::InvalidValue,
                    format!("{} in '{}:{}'", err, display_path, index + 1),
                ));
            },
        }
    }

    normalizer.finish_file()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    fn option_name(self) -> &'static str {
        match self {
            Self::Dir => "--dir",
            Self::Target => "--target",
            Self::Ignore => "--ignore",
            Self::Defer => "--defer",
            Self::Override => "--override",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LongOptionKind {
    Bool,
    Value(ResourceValueOption),
    Mode(OperationMode),
    Verbose,
    Help,
    Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LongOptionSpec {
    name: &'static str,
    canonical: &'static str,
    kind: LongOptionKind,
}

const LONG_OPTION_SPECS: &[LongOptionSpec] = &[
    LongOptionSpec {
        name: "target",
        canonical: "target",
        kind: LongOptionKind::Value(ResourceValueOption::Target),
    },
    LongOptionSpec {
        name: "dir",
        canonical: "dir",
        kind: LongOptionKind::Value(ResourceValueOption::Dir),
    },
    LongOptionSpec {
        name: "stow",
        canonical: "stow",
        kind: LongOptionKind::Mode(OperationMode::Stow),
    },
    LongOptionSpec {
        name: "delete",
        canonical: "delete",
        kind: LongOptionKind::Mode(OperationMode::Delete),
    },
    LongOptionSpec {
        name: "restow",
        canonical: "restow",
        kind: LongOptionKind::Mode(OperationMode::Restow),
    },
    LongOptionSpec {
        name: "adopt",
        canonical: "adopt",
        kind: LongOptionKind::Bool,
    },
    LongOptionSpec {
        name: "no-folding",
        canonical: "no-folding",
        kind: LongOptionKind::Bool,
    },
    LongOptionSpec {
        name: "dotfiles",
        canonical: "dotfiles",
        kind: LongOptionKind::Bool,
    },
    LongOptionSpec {
        name: "compat",
        canonical: "compat",
        kind: LongOptionKind::Bool,
    },
    LongOptionSpec {
        name: "override",
        canonical: "override",
        kind: LongOptionKind::Value(ResourceValueOption::Override),
    },
    LongOptionSpec {
        name: "defer",
        canonical: "defer",
        kind: LongOptionKind::Value(ResourceValueOption::Defer),
    },
    LongOptionSpec {
        name: "ignore",
        canonical: "ignore",
        kind: LongOptionKind::Value(ResourceValueOption::Ignore),
    },
    LongOptionSpec {
        name: "simulate",
        canonical: "simulate",
        kind: LongOptionKind::Bool,
    },
    LongOptionSpec {
        name: "no",
        canonical: "simulate",
        kind: LongOptionKind::Bool,
    },
    LongOptionSpec {
        name: "verbose",
        canonical: "verbose",
        kind: LongOptionKind::Verbose,
    },
    LongOptionSpec {
        name: "help",
        canonical: "help",
        kind: LongOptionKind::Help,
    },
    LongOptionSpec {
        name: "version",
        canonical: "version",
        kind: LongOptionKind::Version,
    },
];

const SHORT_OPTION_SPECS: &[char] = &['h', 'V', 'S', 'D', 'R', 'p', 'n', 'v', 't', 'd'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LongOptionResolveError {
    Unknown,
    Ambiguous,
}

fn resolve_long_option(option: &str) -> Result<LongOptionSpec, LongOptionResolveError> {
    if option.is_empty() {
        return Err(LongOptionResolveError::Unknown);
    }

    if let Some(spec) = LONG_OPTION_SPECS.iter().find(|spec| spec.name == option) {
        return Ok(*spec);
    }

    let mut matched = None;
    for spec in LONG_OPTION_SPECS
        .iter()
        .filter(|spec| spec.name.starts_with(option))
    {
        if let Some(previous) = matched {
            let previous: LongOptionSpec = previous;
            if previous.canonical != spec.canonical {
                return Err(LongOptionResolveError::Ambiguous);
            }
        } else {
            matched = Some(*spec);
        }
    }

    matched.ok_or(LongOptionResolveError::Unknown)
}

fn long_option_abbreviation_candidates(option: &str) -> Vec<&'static str> {
    let mut candidates = Vec::new();
    for spec in LONG_OPTION_SPECS
        .iter()
        .filter(|spec| spec.name.starts_with(option))
    {
        if !candidates.contains(&spec.name) {
            candidates.push(spec.name);
        }
    }
    candidates
}

#[derive(Debug, Clone, Copy)]
struct ResourcePathValue {
    index: usize,
    option_name: ResourceValueOption,
    value_start: usize,
}

#[derive(Debug, Clone)]
struct ResourcePathDisplay {
    option_name: ResourceValueOption,
    path: PathBuf,
    display: String,
}

#[derive(Debug)]
struct ParsedResourceOption {
    tokens: Vec<OsString>,
    expecting_value: Option<ResourceValueExpectation>,
    path_value: Option<ResourcePathValue>,
    verbose: ResourceVerboseAction,
}

#[derive(Debug, Clone, Copy, Default)]
struct ResourceVerboseAction {
    increments: u8,
    value: Option<u8>,
    expects_value: bool,
}

#[derive(Debug)]
struct NormalizedStowrcArgs {
    argv: Vec<OsString>,
    path_displays: Vec<ResourcePathDisplay>,
}

#[derive(Debug, Clone)]
struct StowrcNormalizerCheckpoint {
    normalized_len: usize,
    path_values_len: usize,
    expecting_value: Option<PendingResourceValue>,
    expecting_verbose_value: bool,
    after_double_dash: bool,
    resource_verbosity: u8,
    saw_resource_verbose: bool,
}

#[derive(Debug)]
struct StowrcNormalizer {
    normalized: Vec<OsString>,
    expecting_value: Option<PendingResourceValue>,
    expecting_verbose_value: bool,
    path_values: Vec<ResourcePathValue>,
    after_double_dash: bool,
    resource_verbosity: u8,
    saw_resource_verbose: bool,
}

impl StowrcNormalizer {
    fn new() -> Self {
        Self {
            normalized: Vec::new(),
            expecting_value: None,
            expecting_verbose_value: false,
            path_values: Vec::new(),
            after_double_dash: false,
            resource_verbosity: 0,
            saw_resource_verbose: false,
        }
    }

    fn checkpoint(&self) -> StowrcNormalizerCheckpoint {
        StowrcNormalizerCheckpoint {
            normalized_len: self.normalized.len(),
            path_values_len: self.path_values.len(),
            expecting_value: self.expecting_value.clone(),
            expecting_verbose_value: self.expecting_verbose_value,
            after_double_dash: self.after_double_dash,
            resource_verbosity: self.resource_verbosity,
            saw_resource_verbose: self.saw_resource_verbose,
        }
    }

    fn rewind(&mut self, checkpoint: StowrcNormalizerCheckpoint) {
        self.normalized.truncate(checkpoint.normalized_len);
        self.path_values.truncate(checkpoint.path_values_len);
        self.expecting_value = checkpoint.expecting_value;
        self.expecting_verbose_value = checkpoint.expecting_verbose_value;
        self.after_double_dash = checkpoint.after_double_dash;
        self.resource_verbosity = checkpoint.resource_verbosity;
        self.saw_resource_verbose = checkpoint.saw_resource_verbose;
    }

    fn push_token(&mut self, token: StowrcToken) -> Result<(), clap::Error> {
        let StowrcToken {
            value: token_value,
            origin,
        } = token;
        let token_text = token_value.to_string_lossy();

        if let Some(pending) = self.expecting_value.take() {
            let mut normalized_value = OsString::from(&pending.value_prefix);
            normalized_value.push(&token_value);
            let value_start = pending.value_prefix.len();
            if pending.option_name.is_path_option() {
                self.path_values.push(ResourcePathValue {
                    index: self.normalized.len(),
                    option_name: pending.option_name,
                    value_start,
                });
            }
            self.normalized.push(normalized_value);
            return Ok(());
        }

        if self.expecting_verbose_value {
            self.expecting_verbose_value = false;
            if is_verbose_numeric_token(&token_text) {
                self.set_resource_verbosity(parse_stowrc_verbose_numeric_value(
                    &token_text,
                    &origin,
                )?);
                return Ok(());
            }
            self.increment_resource_verbosity();
        }

        if self.after_double_dash {
            return Ok(());
        }

        if token_text == "--" {
            // GNU Stow ignores package names in resource files; after `--`,
            // every following token is necessarily a package-like argument.
            self.after_double_dash = true;
            return Ok(());
        }

        if !token_text.starts_with('-') {
            // GNU Stow does not allow package names in resource files, so
            // package-like tokens are ignored instead of becoming CLI args.
            return Ok(());
        }

        if token_text.starts_with("--") {
            if let Some(parsed) = parse_long_stowrc_option(&token_value, &origin)? {
                self.apply_parsed_option(parsed, origin);
            }

            return Ok(());
        }

        if token_text.len() > 1 {
            if let Some(parsed) = parse_short_stowrc_option(&token_value, &origin)? {
                self.apply_parsed_option(parsed, origin);
            }
        }

        Ok(())
    }

    fn apply_parsed_option(&mut self, parsed: ParsedResourceOption, origin: StowrcTokenOrigin) {
        let base_index = self.normalized.len();
        self.normalized.extend(parsed.tokens);
        if let Some(path_value) = parsed.path_value {
            self.path_values.push(ResourcePathValue {
                index: base_index + path_value.index,
                option_name: path_value.option_name,
                value_start: path_value.value_start,
            });
        }
        if let Some(option_name) = parsed.expecting_value {
            self.expecting_value = Some(PendingResourceValue {
                option_name: option_name.option_name,
                origin,
                value_prefix: option_name.value_prefix,
            });
        }
        self.apply_verbose_action(parsed.verbose);
    }

    fn apply_verbose_action(&mut self, verbose: ResourceVerboseAction) {
        if let Some(value) = verbose.value {
            self.set_resource_verbosity(value);
        }
        for _ in 0..verbose.increments {
            self.increment_resource_verbosity();
        }
        self.expecting_verbose_value = verbose.expects_value;
    }

    fn set_resource_verbosity(&mut self, value: u8) {
        self.resource_verbosity = value;
        self.saw_resource_verbose = true;
    }

    fn increment_resource_verbosity(&mut self) {
        self.resource_verbosity = self.resource_verbosity.saturating_add(1);
        self.saw_resource_verbose = true;
    }

    fn finish_file(&mut self) -> Result<(), clap::Error> {
        if let Some(pending) = self.expecting_value.take() {
            return Err(stowrc_missing_value_error(&pending));
        }
        if self.expecting_verbose_value {
            self.expecting_verbose_value = false;
            self.increment_resource_verbosity();
        }
        Ok(())
    }

    fn finish(mut self) -> Result<NormalizedStowrcArgs, clap::Error> {
        self.finish_file()?;
        if self.saw_resource_verbose {
            self.normalized.push(OsString::from(format!(
                "--verbose={}",
                self.resource_verbosity
            )));
        }
        let path_displays =
            expand_final_resource_path_values(&mut self.normalized, &self.path_values)?;

        Ok(NormalizedStowrcArgs {
            argv: self.normalized,
            path_displays,
        })
    }
}

fn parse_long_stowrc_option(
    token: &OsString,
    origin: &StowrcTokenOrigin,
) -> Result<Option<ParsedResourceOption>, clap::Error> {
    let token_text = token.to_string_lossy();
    let token_text = token_text.trim();
    let long_token = token_text.trim_start_matches("--");
    if long_token.is_empty() {
        return Ok(None);
    }

    if let Some((key, _value)) = long_token.split_once('=') {
        match resolve_long_option(key) {
            Ok(spec) => match spec.kind {
                LongOptionKind::Value(option_name) if option_name.is_path_option() => {
                    return Ok(Some(ParsedResourceOption {
                        tokens: vec![token.clone()],
                        expecting_value: None,
                        path_value: Some(ResourcePathValue {
                            index: 0,
                            option_name,
                            value_start: key.len() + 3,
                        }),
                        verbose: ResourceVerboseAction::default(),
                    }));
                },
                LongOptionKind::Value(_) => {
                    return Ok(Some(ParsedResourceOption {
                        tokens: vec![token.clone()],
                        expecting_value: None,
                        path_value: None,
                        verbose: ResourceVerboseAction::default(),
                    }));
                },
                LongOptionKind::Verbose => {
                    return Ok(Some(ParsedResourceOption {
                        tokens: Vec::new(),
                        expecting_value: None,
                        path_value: None,
                        verbose: ResourceVerboseAction {
                            value: Some(parse_stowrc_verbose_numeric_value(_value, origin)?),
                            ..ResourceVerboseAction::default()
                        },
                    }));
                },
                LongOptionKind::Bool
                | LongOptionKind::Mode(_)
                | LongOptionKind::Help
                | LongOptionKind::Version => {
                    return Err(stowrc_long_option_value_error(key, origin));
                },
            },
            Err(LongOptionResolveError::Ambiguous) => {
                return Err(ambiguous_stowrc_long_option_error(key, origin));
            },
            Err(LongOptionResolveError::Unknown) => {
                return Err(unknown_stowrc_long_option_error(key, origin));
            },
        }
    }

    match resolve_long_option(long_token) {
        Ok(spec) => match spec.kind {
            // GNU Stow ignores -D, -R, and -S in resource files.
            LongOptionKind::Mode(_) => Ok(None),
            LongOptionKind::Value(option_name) => Ok(Some(ParsedResourceOption {
                tokens: Vec::new(),
                expecting_value: Some(ResourceValueExpectation {
                    option_name,
                    value_prefix: format!("--{}=", long_token),
                }),
                path_value: None,
                verbose: ResourceVerboseAction::default(),
            })),
            LongOptionKind::Verbose => Ok(Some(ParsedResourceOption {
                tokens: Vec::new(),
                expecting_value: None,
                path_value: None,
                verbose: ResourceVerboseAction {
                    expects_value: true,
                    ..ResourceVerboseAction::default()
                },
            })),
            LongOptionKind::Bool | LongOptionKind::Help | LongOptionKind::Version => {
                Ok(Some(ParsedResourceOption {
                    tokens: vec![OsString::from(token_text)],
                    expecting_value: None,
                    path_value: None,
                    verbose: ResourceVerboseAction::default(),
                }))
            },
        },
        Err(LongOptionResolveError::Ambiguous) => {
            Err(ambiguous_stowrc_long_option_error(long_token, origin))
        },
        Err(LongOptionResolveError::Unknown) => {
            Err(unknown_stowrc_long_option_error(long_token, origin))
        },
    }
}

fn map_long_value_option(token: &str) -> Option<ResourceValueOption> {
    match resolve_long_option(token).map(|spec| spec.kind) {
        Ok(LongOptionKind::Value(option_name)) => Some(option_name),
        _ => None,
    }
}

#[cfg(unix)]
fn parse_short_stowrc_option(
    token: &OsString,
    origin: &StowrcTokenOrigin,
) -> Result<Option<ParsedResourceOption>, clap::Error> {
    let bytes = token.as_os_str().as_bytes();
    let cluster = &bytes[1..];
    if cluster.is_empty() {
        return Ok(None);
    }

    let mut bool_flags = Vec::new();
    let mut verbose = ResourceVerboseAction::default();
    let mut index = 0;

    while index < cluster.len() {
        match cluster[index] {
            b'S' | b'D' | b'R' => {
                // GNU Stow ignores -D, -R, and -S in resource files.
            },
            b't' | b'd' => {
                let flag = cluster[index];
                let option_name = match flag {
                    b't' => ResourceValueOption::Target,
                    b'd' => ResourceValueOption::Dir,
                    _ => unreachable!(),
                };
                let rest = &cluster[index + 1..];
                let mut value_prefix = Vec::with_capacity(1 + bool_flags.len() + 1);
                value_prefix.push(b'-');
                value_prefix.extend_from_slice(&bool_flags);
                value_prefix.push(flag);

                if rest.is_empty() {
                    let value_prefix = String::from_utf8(value_prefix)
                        .expect("resource short option prefix is ASCII");
                    return Ok(Some(ParsedResourceOption {
                        tokens: Vec::new(),
                        expecting_value: Some(ResourceValueExpectation {
                            option_name,
                            value_prefix,
                        }),
                        path_value: None,
                        verbose,
                    }));
                }

                let mut normalized = value_prefix.clone();
                normalized.extend_from_slice(rest);
                return Ok(Some(ParsedResourceOption {
                    tokens: vec![OsString::from_vec(normalized)],
                    expecting_value: None,
                    path_value: Some(ResourcePathValue {
                        index: 0,
                        option_name,
                        value_start: value_prefix.len(),
                    }),
                    verbose,
                }));
            },
            b'v' => {
                let rest = &cluster[index + 1..];
                if rest.is_empty() {
                    verbose.expects_value = true;
                    return Ok(Some(ParsedResourceOption {
                        tokens: normalized_short_tokens_from_bytes(&bool_flags, &[]),
                        expecting_value: None,
                        path_value: None,
                        verbose,
                    }));
                }
                if rest.first().is_some_and(u8::is_ascii_digit) {
                    let rest = std::str::from_utf8(rest).map_err(|_| {
                        stowrc_verbose_level_error(&String::from_utf8_lossy(rest), origin)
                    })?;
                    verbose.value = Some(parse_stowrc_verbose_numeric_value(rest, origin)?);
                    return Ok(Some(ParsedResourceOption {
                        tokens: normalized_short_tokens_from_bytes(&bool_flags, &[]),
                        expecting_value: None,
                        path_value: None,
                        verbose,
                    }));
                }
                verbose.increments = verbose.increments.saturating_add(1);
            },
            b'p' | b'h' | b'V' | b'n' => {
                bool_flags.push(cluster[index]);
            },
            other => {
                return Err(unknown_stowrc_short_option_error(
                    char::from_u32(u32::from(other)).unwrap_or('\u{FFFD}'),
                    origin,
                ));
            },
        }

        index += 1;
    }

    if bool_flags.is_empty() && verbose.increments == 0 && verbose.value.is_none() {
        Ok(None)
    } else {
        Ok(Some(ParsedResourceOption {
            tokens: normalized_short_tokens_from_bytes(&bool_flags, &[]),
            expecting_value: None,
            path_value: None,
            verbose,
        }))
    }
}

#[cfg(not(unix))]
fn parse_short_stowrc_option(
    token: &OsString,
    origin: &StowrcTokenOrigin,
) -> Result<Option<ParsedResourceOption>, clap::Error> {
    let token_text = token.to_string_lossy();
    let token = token_text.as_ref();
    let cluster = &token[1..];
    if cluster.is_empty() {
        return Ok(None);
    }

    let mut bool_flags = String::new();
    let mut verbose = ResourceVerboseAction::default();

    for (index, flag) in cluster.char_indices() {
        match flag {
            'S' | 'D' | 'R' => {
                continue;
            },
            't' | 'd' => {
                let option_name = match flag {
                    't' => ResourceValueOption::Target,
                    'd' => ResourceValueOption::Dir,
                    _ => unreachable!(),
                };
                let token_remainder = &cluster[index + flag.len_utf8()..];
                if token_remainder.is_empty() {
                    if bool_flags.is_empty() {
                        let value_prefix = format!("-{flag}");
                        return Ok(Some(ParsedResourceOption {
                            tokens: Vec::new(),
                            expecting_value: Some(ResourceValueExpectation {
                                option_name,
                                value_prefix,
                            }),
                            path_value: None,
                            verbose,
                        }));
                    }
                    let value_prefix = format!("-{}{}", bool_flags, flag);
                    return Ok(Some(ParsedResourceOption {
                        tokens: Vec::new(),
                        expecting_value: Some(ResourceValueExpectation {
                            option_name,
                            value_prefix,
                        }),
                        path_value: None,
                        verbose,
                    }));
                }

                let prefix = if bool_flags.is_empty() {
                    format!("-{flag}")
                } else {
                    format!("-{}{}", bool_flags, flag)
                };
                return Ok(Some(ParsedResourceOption {
                    tokens: vec![OsString::from(format!("{prefix}{token_remainder}"))],
                    expecting_value: None,
                    path_value: Some(ResourcePathValue {
                        index: 0,
                        option_name,
                        value_start: prefix.len(),
                    }),
                    verbose,
                }));
            },
            'v' => {
                let rest = &cluster[index + flag.len_utf8()..];
                if rest.is_empty() {
                    verbose.expects_value = true;
                    return Ok(Some(ParsedResourceOption {
                        tokens: normalized_short_tokens(bool_flags),
                        expecting_value: None,
                        path_value: None,
                        verbose,
                    }));
                }
                if starts_with_verbose_numeric_value(rest) {
                    verbose.value = Some(parse_stowrc_verbose_numeric_value(rest, origin)?);
                    return Ok(Some(ParsedResourceOption {
                        tokens: normalized_short_tokens(bool_flags),
                        expecting_value: None,
                        path_value: None,
                        verbose,
                    }));
                }
                verbose.increments = verbose.increments.saturating_add(1);
            },
            'p' | 'h' | 'V' | 'n' => {
                bool_flags.push(flag);
            },
            other => {
                return Err(unknown_stowrc_short_option_error(other, origin));
            },
        }
    }

    if bool_flags.is_empty() && verbose.increments == 0 && verbose.value.is_none() {
        Ok(None)
    } else {
        Ok(Some(ParsedResourceOption {
            tokens: normalized_short_tokens(bool_flags),
            expecting_value: None,
            path_value: None,
            verbose,
        }))
    }
}

#[cfg(any(test, not(unix)))]
fn emit_stowrc_line_tokens<F>(line: &str, mut emit: F) -> Result<bool, String>
where
    F: FnMut(String),
{
    let mut token = String::new();
    let mut token_started = false;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' && !in_single_quote {
            if let Some(next) = chars.next() {
                token.push(next);
                token_started = true;
                continue;
            }
            return Ok(false);
        }

        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            token_started = true;
            continue;
        }

        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            token_started = true;
            continue;
        }

        if ch.is_whitespace() && !in_single_quote && !in_double_quote {
            if token_started {
                emit(std::mem::take(&mut token));
                token_started = false;
            }
            continue;
        }

        token.push(ch);
        token_started = true;
    }

    if in_single_quote || in_double_quote {
        return Ok(false);
    }

    if token_started {
        emit(token);
    }

    Ok(true)
}

#[cfg(unix)]
fn emit_stowrc_line_tokens_bytes<F>(line: &[u8], mut emit: F) -> Result<bool, String>
where
    F: FnMut(Vec<u8>),
{
    let mut token = Vec::new();
    let mut token_started = false;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut index = 0;

    while index < line.len() {
        let byte = line[index];
        if byte == b'\\' && !in_single_quote {
            if let Some(next) = line.get(index + 1) {
                token.push(*next);
                token_started = true;
                index += 2;
                continue;
            }
            return Ok(false);
        }

        if byte == b'\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            token_started = true;
            index += 1;
            continue;
        }

        if byte == b'"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            token_started = true;
            index += 1;
            continue;
        }

        if byte.is_ascii_whitespace() && !in_single_quote && !in_double_quote {
            if token_started {
                emit(std::mem::take(&mut token));
                token_started = false;
            }
            index += 1;
            continue;
        }

        token.push(byte);
        token_started = true;
        index += 1;
    }

    if in_single_quote || in_double_quote {
        return Ok(false);
    }

    if token_started {
        emit(token);
    }

    Ok(true)
}

#[cfg(test)]
fn tokenize_stowrc_line(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    if !emit_stowrc_line_tokens(line, |token| tokens.push(token))? {
        tokens.clear();
    }
    Ok(tokens)
}

#[cfg(test)]
fn expand_path_value(
    raw_value: &str,
    option_name: ResourceValueOption,
) -> Result<String, clap::Error> {
    let expanded = expand_path_value_with_display(raw_value, option_name)?;
    Ok(expanded
        .value
        .into_string()
        .expect("test path value should be valid UTF-8"))
}

struct ExpandedPathValue {
    value: OsString,
    display: Option<String>,
}

fn expand_path_value_with_display(
    raw_value: &str,
    option_name: ResourceValueOption,
) -> Result<ExpandedPathValue, clap::Error> {
    let env_expanded = expand_environment_value(raw_value, option_name)?;
    let display = env_expanded
        .display
        .map(|display| unescape_tilde_markers(&display))
        .or_else(|| {
            let tilde_expanded = expand_tilde_value_with_display(env_expanded.value.clone());
            tilde_expanded.display
        });
    let tilde_expanded = expand_tilde_value_with_display(env_expanded.value);

    Ok(ExpandedPathValue {
        value: tilde_expanded.value,
        display,
    })
}

#[cfg(unix)]
fn expand_path_value_os_with_display(
    raw_value: OsString,
    option_name: ResourceValueOption,
) -> Result<ExpandedPathValue, clap::Error> {
    if let Some(raw_value) = raw_value.to_str() {
        return expand_path_value_with_display(raw_value, option_name);
    }

    let env_expanded = expand_environment_value_os(raw_value, option_name)?;
    let display = env_expanded
        .display
        .map(|display| unescape_tilde_markers(&display))
        .or_else(|| {
            let tilde_expanded = expand_tilde_value_with_display(env_expanded.value.clone());
            tilde_expanded.display
        });
    let tilde_expanded = expand_tilde_value_with_display(env_expanded.value);

    Ok(ExpandedPathValue {
        value: tilde_expanded.value,
        display,
    })
}

#[cfg(not(unix))]
fn expand_path_value_os_with_display(
    raw_value: OsString,
    option_name: ResourceValueOption,
) -> Result<ExpandedPathValue, clap::Error> {
    match raw_value.into_string() {
        Ok(raw_value) => expand_path_value_with_display(&raw_value, option_name),
        Err(raw_value) => {
            let tilde_expanded = expand_tilde_value_with_display(raw_value);
            Ok(ExpandedPathValue {
                value: tilde_expanded.value,
                display: tilde_expanded.display,
            })
        },
    }
}

struct ExpandedEnvironmentValue {
    value: OsString,
    display: Option<String>,
}

struct ExpandedTildeValue {
    value: OsString,
    display: Option<String>,
}

fn expand_environment_value(
    raw_value: &str,
    option_name: ResourceValueOption,
) -> Result<ExpandedEnvironmentValue, clap::Error> {
    let mut output = OsString::new();
    let mut display = String::new();
    let mut changed_display = false;
    let mut chars = raw_value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                if next == '$' {
                    push_char_os(&mut output, next);
                    display.push(next);
                } else {
                    output.push("\\");
                    push_char_os(&mut output, next);
                    display.push('\\');
                    display.push(next);
                }
            } else {
                output.push("\\");
                display.push('\\');
            }
            continue;
        }

        if ch == '$' {
            if let Some('{') = chars.peek().copied() {
                chars.next();
                let mut variable = String::new();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == '}' {
                        closed = true;
                        break;
                    }
                    variable.push(next);
                }
                if !closed || !is_gnu_braced_env_name(&variable) {
                    output.push("${");
                    output.push(&variable);
                    display.push_str("${");
                    display.push_str(&variable);
                    if closed {
                        output.push("}");
                        display.push('}');
                    }
                    continue;
                }
                let value = env_resource_value(&variable, option_name)?;
                output.push(value);
                display.push_str(&format!("${{{}}}", variable));
                changed_display = true;
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

                    let value = env_resource_value(&variable, option_name)?;
                    output.push(value);
                    display.push_str(&format!("${}", variable));
                    changed_display = true;
                    continue;
                }
            }

            output.push("$");
            display.push('$');
            continue;
        }

        push_char_os(&mut output, ch);
        display.push(ch);
    }

    Ok(ExpandedEnvironmentValue {
        value: output,
        display: changed_display.then_some(display),
    })
}

#[cfg(unix)]
fn expand_environment_value_os(
    raw_value: OsString,
    option_name: ResourceValueOption,
) -> Result<ExpandedEnvironmentValue, clap::Error> {
    let bytes = raw_value.as_os_str().as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut display = Vec::with_capacity(bytes.len());
    let mut changed_display = false;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            if let Some(next) = bytes.get(index + 1) {
                if *next == b'$' {
                    output.push(*next);
                    display.push(*next);
                } else {
                    output.push(b'\\');
                    output.push(*next);
                    display.push(b'\\');
                    display.push(*next);
                }
                index += 2;
            } else {
                output.push(b'\\');
                display.push(b'\\');
                index += 1;
            }
            continue;
        }

        if byte == b'$' {
            if bytes.get(index + 1) == Some(&b'{') {
                let variable_start = index + 2;
                let mut variable_end = variable_start;
                let mut closed = false;
                while variable_end < bytes.len() {
                    if bytes[variable_end] == b'}' {
                        closed = true;
                        break;
                    }
                    variable_end += 1;
                }
                let variable = &bytes[variable_start..variable_end];
                if closed && is_gnu_braced_env_name_bytes(variable) {
                    let variable = std::str::from_utf8(variable)
                        .expect("validated braced environment name is ASCII");
                    let value = env_resource_value(variable, option_name)?;
                    output.extend_from_slice(value.as_os_str().as_bytes());
                    display.extend_from_slice(b"${");
                    display.extend_from_slice(variable.as_bytes());
                    display.push(b'}');
                    changed_display = true;
                    index = variable_end + 1;
                    continue;
                }

                output.extend_from_slice(b"${");
                output.extend_from_slice(variable);
                display.extend_from_slice(b"${");
                display.extend_from_slice(variable);
                if closed {
                    output.push(b'}');
                    display.push(b'}');
                    index = variable_end + 1;
                } else {
                    index = variable_end;
                }
                continue;
            }

            if let Some(next) = bytes.get(index + 1) {
                if is_valid_var_start_byte(*next) {
                    let variable_start = index + 1;
                    let mut variable_end = variable_start + 1;
                    while variable_end < bytes.len() && is_valid_var_char_byte(bytes[variable_end])
                    {
                        variable_end += 1;
                    }
                    let variable = std::str::from_utf8(&bytes[variable_start..variable_end])
                        .expect("validated environment name is ASCII");
                    let value = env_resource_value(variable, option_name)?;
                    output.extend_from_slice(value.as_os_str().as_bytes());
                    display.push(b'$');
                    display.extend_from_slice(variable.as_bytes());
                    changed_display = true;
                    index = variable_end;
                    continue;
                }
            }

            output.push(b'$');
            display.push(b'$');
            index += 1;
            continue;
        }

        output.push(byte);
        display.push(byte);
        index += 1;
    }

    Ok(ExpandedEnvironmentValue {
        value: OsString::from_vec(output),
        display: changed_display.then(|| String::from_utf8_lossy(&display).into_owned()),
    })
}

fn env_resource_value(
    variable: &str,
    option_name: ResourceValueOption,
) -> Result<OsString, clap::Error> {
    // GNU Stow 2.4.1 aborts here via _safe_expand_env_var despite a nearby
    // source comment mentioning Perl's empty-string fallback.
    env::var_os(variable).ok_or_else(|| undefined_env_var_error(variable, option_name))
}

fn undefined_env_var_error(variable: &str, option_name: ResourceValueOption) -> clap::Error {
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!(
            "{} option references undefined environment variable ${}; aborting!",
            option_name.option_name(),
            variable
        ),
    )
}

fn expand_tilde_value_with_display(value: OsString) -> ExpandedTildeValue {
    expand_tilde_value_with_display_impl(value)
}

#[cfg(not(unix))]
fn expand_tilde_value_with_display_impl(value: OsString) -> ExpandedTildeValue {
    let value_string = value.to_string_lossy().into_owned();
    let tilde_expanded = if let Some(rest) = value_string.strip_prefix('~') {
        let user_end = rest.find('/').unwrap_or(rest.len());
        let user = &rest[..user_end];
        let suffix = &rest[user_end..];
        let mut output = OsString::new();

        if user.is_empty() {
            if let Some(home_dir) = current_user_home_dir() {
                output.push(home_dir);
            } else {
                output.push("~");
            }
        } else if let Some(user_home) = home_dir_for_user(user) {
            output.push(user_home);
        } else {
            // GNU Stow's Perl tilde expansion substitutes undef here, which
            // effectively removes the unknown user part of the path.
        }

        output.push(unescape_tilde_markers(suffix));

        output
    } else {
        unescape_tilde_markers_os(value)
    };

    let display = value_string
        .starts_with('~')
        .then(|| unescape_tilde_markers(&value_string))
        .filter(|display| display.as_str() != tilde_expanded.to_string_lossy());

    ExpandedTildeValue {
        value: tilde_expanded,
        display,
    }
}

#[cfg(unix)]
fn expand_tilde_value_with_display_impl(value: OsString) -> ExpandedTildeValue {
    let display_source = value.as_os_str().to_str().map(str::to_owned);
    let value_bytes = value.as_os_str().as_bytes();
    let tilde_expanded = if let Some(rest) = value_bytes.strip_prefix(b"~") {
        let user_end = rest
            .iter()
            .position(|byte| *byte == b'/')
            .unwrap_or(rest.len());
        let user = &rest[..user_end];
        let suffix = &rest[user_end..];
        let mut output = Vec::new();

        if user.is_empty() {
            if let Some(home_dir) = current_user_home_dir() {
                output.extend_from_slice(home_dir.as_os_str().as_bytes());
            } else {
                output.push(b'~');
            }
        } else if let Ok(user) = std::str::from_utf8(user) {
            if let Some(user_home) = home_dir_for_user(user) {
                output.extend_from_slice(user_home.as_os_str().as_bytes());
            }
        } else {
            // GNU Stow's Perl tilde expansion substitutes undef here, which
            // effectively removes the unknown user part of the path.
        }

        output.extend_from_slice(&unescape_tilde_bytes(suffix));
        OsString::from_vec(output)
    } else {
        unescape_tilde_markers_os(value)
    };

    let display = display_source
        .as_deref()
        .filter(|value| value.starts_with('~'))
        .map(unescape_tilde_markers)
        .filter(|display| display.as_str() != tilde_expanded.to_string_lossy());

    ExpandedTildeValue {
        value: tilde_expanded,
        display,
    }
}

fn unescape_tilde_markers(value: &str) -> String {
    value.replace("\\~", "~")
}

fn push_char_os(output: &mut OsString, ch: char) {
    let mut buffer = [0; 4];
    output.push(ch.encode_utf8(&mut buffer));
}

#[cfg(not(unix))]
fn unescape_tilde_markers_os(value: OsString) -> OsString {
    OsString::from(unescape_tilde_markers(&value.to_string_lossy()))
}

#[cfg(unix)]
fn unescape_tilde_markers_os(value: OsString) -> OsString {
    OsString::from_vec(unescape_tilde_bytes(&value.into_vec()))
}

#[cfg(unix)]
fn unescape_tilde_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && bytes.get(index + 1) == Some(&b'~') {
            output.push(b'~');
            index += 2;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    output
}

fn expand_final_resource_path_values(
    normalized: &mut [OsString],
    path_values: &[ResourcePathValue],
) -> Result<Vec<ResourcePathDisplay>, clap::Error> {
    let final_dir_index = final_resource_path_value_index(path_values, ResourceValueOption::Dir);
    let final_target_index =
        final_resource_path_value_index(path_values, ResourceValueOption::Target);
    let mut path_displays = Vec::new();

    for path_value in path_values {
        let should_expand = match path_value.option_name {
            ResourceValueOption::Dir => Some(path_value.index) == final_dir_index,
            ResourceValueOption::Target => Some(path_value.index) == final_target_index,
            _ => false,
        };

        if should_expand {
            let (prefix, raw_value) =
                split_resource_path_token(&normalized[path_value.index], path_value.value_start);
            let expanded = expand_path_value_os_with_display(raw_value, path_value.option_name)?;
            let mut expanded_token = prefix;
            expanded_token.push(&expanded.value);
            let redacted_path_value = expanded.value;
            let redacted_display = expanded.display;
            if let Some(display) = redacted_display {
                path_displays.push(ResourcePathDisplay {
                    option_name: path_value.option_name,
                    path: PathBuf::from(redacted_path_value),
                    display,
                });
            }
            normalized[path_value.index] = expanded_token;
        }
    }

    Ok(path_displays)
}

#[cfg(unix)]
fn split_resource_path_token(token: &OsString, value_start: usize) -> (OsString, OsString) {
    let bytes = token.as_os_str().as_bytes();
    (
        OsString::from_vec(bytes[..value_start].to_vec()),
        OsString::from_vec(bytes[value_start..].to_vec()),
    )
}

#[cfg(not(unix))]
fn split_resource_path_token(token: &OsString, value_start: usize) -> (OsString, OsString) {
    let token = token.to_string_lossy();
    let (prefix, value) = token.split_at(value_start);
    (OsString::from(prefix), OsString::from(value))
}

fn final_resource_path_value_index(
    path_values: &[ResourcePathValue],
    option_name: ResourceValueOption,
) -> Option<usize> {
    path_values
        .iter()
        .rev()
        .find(|value| value.option_name == option_name)
        .map(|value| value.index)
}

fn stowrc_missing_value_error(pending: &PendingResourceValue) -> clap::Error {
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!(
            "resource file option '{}' requires a value in '{}:{}'",
            pending.option_name.option_name(),
            pending.origin.display_path,
            pending.origin.line
        ),
    )
}

fn current_user_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|home| !home.as_os_str().is_empty())
        .or_else(|| env::var_os("LOGDIR").filter(|home| !home.as_os_str().is_empty()))
        .map(PathBuf::from)
        .or_else(current_user_home_dir_from_system)
}

#[cfg(unix)]
fn current_user_home_dir_from_system() -> Option<PathBuf> {
    user_home_dir_by_uid(unsafe { libc::geteuid() })
}

#[cfg(not(unix))]
fn current_user_home_dir_from_system() -> Option<PathBuf> {
    dirs::home_dir()
}

#[cfg(unix)]
fn home_dir_for_user(user: &str) -> Option<PathBuf> {
    let user = CString::new(user).ok()?;
    passwd_home_dir(|pwd, buffer, result| unsafe {
        libc::getpwnam_r(
            user.as_ptr(),
            pwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            result,
        )
    })
}

#[cfg(not(unix))]
fn home_dir_for_user(_user: &str) -> Option<PathBuf> {
    None
}

#[cfg(unix)]
fn user_home_dir_by_uid(uid: libc::uid_t) -> Option<PathBuf> {
    passwd_home_dir(|pwd, buffer, result| unsafe {
        libc::getpwuid_r(uid, pwd, buffer.as_mut_ptr().cast(), buffer.len(), result)
    })
}

#[cfg(unix)]
fn passwd_home_dir<F>(mut lookup: F) -> Option<PathBuf>
where
    F: FnMut(&mut libc::passwd, &mut [u8], &mut *mut libc::passwd) -> libc::c_int,
{
    let suggested_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let mut buffer_size = if suggested_size > 0 {
        suggested_size as usize
    } else {
        16 * 1024
    };

    while buffer_size <= 1024 * 1024 {
        let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        let mut buffer = vec![0; buffer_size];
        let status = lookup(unsafe { &mut *pwd.as_mut_ptr() }, &mut buffer, &mut result);

        if status == 0 {
            if result.is_null() {
                return None;
            }
            let pwd = unsafe { pwd.assume_init() };
            if pwd.pw_dir.is_null() {
                return None;
            }
            let home = unsafe { CStr::from_ptr(pwd.pw_dir) };
            return Some(PathBuf::from(OsString::from_vec(home.to_bytes().to_vec())));
        }

        if status == libc::ERANGE {
            buffer_size *= 2;
            continue;
        }

        return None;
    }

    None
}

fn is_valid_var_start(ch: char) -> bool {
    is_valid_var_char(ch)
}

fn is_valid_var_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_gnu_braced_env_name(variable: &str) -> bool {
    !variable.is_empty()
        && variable
            .chars()
            .all(|ch| is_valid_var_char(ch) || ch.is_ascii_whitespace())
}

#[cfg(unix)]
fn is_valid_var_start_byte(byte: u8) -> bool {
    is_valid_var_char_byte(byte)
}

#[cfg(unix)]
fn is_valid_var_char_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

#[cfg(unix)]
fn is_gnu_braced_env_name_bytes(variable: &[u8]) -> bool {
    !variable.is_empty()
        && variable
            .iter()
            .all(|byte| is_valid_var_char_byte(*byte) || byte.is_ascii_whitespace())
}

fn normalize_verbose_args(argv: &[OsString]) -> Result<Vec<OsString>, clap::Error> {
    let mut normalized_args = Vec::with_capacity(argv.len());
    let mut expecting_option_value = false;
    let mut expecting_verbose_value = false;
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
        if expecting_verbose_value {
            expecting_verbose_value = false;
            if is_verbose_numeric_token(&arg_string) {
                continue;
            }
        }

        if arg_string == "--" {
            after_double_dash = true;
            normalized_args.push(arg.clone());
            continue;
        }

        if let Some(long_token) = arg_string.strip_prefix("--") {
            let (key, attached_value) = long_token
                .split_once('=')
                .map_or((long_token, None), |(key, value)| (key, Some(value)));
            match resolve_long_option(key) {
                Ok(spec) => {
                    let canonical = format!("--{}", spec.canonical);
                    match spec.kind {
                        LongOptionKind::Value(_) => {
                            if attached_value.is_some() {
                                normalized_args
                                    .push(canonicalize_attached_long_arg(arg, spec.canonical));
                            } else {
                                expecting_option_value = true;
                                normalized_args.push(OsString::from(canonical));
                            }
                        },
                        LongOptionKind::Verbose => {
                            if let Some(value) = attached_value {
                                parse_verbose_numeric_value(value)?;
                            } else {
                                expecting_verbose_value = true;
                            }
                        },
                        LongOptionKind::Bool
                        | LongOptionKind::Mode(_)
                        | LongOptionKind::Help
                        | LongOptionKind::Version => {
                            if attached_value.is_some() {
                                normalized_args
                                    .push(canonicalize_attached_long_arg(arg, spec.canonical));
                            } else {
                                normalized_args.push(OsString::from(canonical));
                            }
                        },
                    }
                },
                Err(LongOptionResolveError::Ambiguous) => {
                    return Err(ambiguous_long_option_error(key));
                },
                Err(LongOptionResolveError::Unknown) => {
                    return Err(unknown_long_option_error(key));
                },
            }
            continue;
        }

        if arg_string.starts_with('-') && arg_string.len() > 1 {
            let normalized_short = normalize_short_verbose_arg(arg)?;
            expecting_option_value = normalized_short.expects_option_value;
            expecting_verbose_value = normalized_short.expects_verbose_value;
            normalized_args.extend(normalized_short.tokens);
            continue;
        }

        normalized_args.push(arg.clone());
    }

    Ok(normalized_args)
}

#[derive(Debug)]
struct NormalizedShortArg {
    tokens: Vec<OsString>,
    expects_option_value: bool,
    expects_verbose_value: bool,
}

#[cfg(unix)]
fn normalize_short_verbose_arg(arg: &OsString) -> Result<NormalizedShortArg, clap::Error> {
    let bytes = arg.as_os_str().as_bytes();
    let mut kept_flags = Vec::new();
    let mut index = 1;

    while index < bytes.len() {
        match bytes[index] {
            b't' | b'd' => {
                kept_flags.push(bytes[index]);
                let rest = &bytes[index + 1..];
                return Ok(NormalizedShortArg {
                    tokens: normalized_short_tokens_from_bytes(&kept_flags, rest),
                    expects_option_value: rest.is_empty(),
                    expects_verbose_value: false,
                });
            },
            b'v' => {
                let rest = &bytes[index + 1..];
                if rest.is_empty() {
                    return Ok(NormalizedShortArg {
                        tokens: normalized_short_tokens_from_bytes(&kept_flags, &[]),
                        expects_option_value: false,
                        expects_verbose_value: true,
                    });
                }
                if rest.first().is_some_and(u8::is_ascii_digit) {
                    let rest = std::str::from_utf8(rest)
                        .map_err(|_| verbose_level_error(&String::from_utf8_lossy(rest)))?;
                    parse_verbose_numeric_value(rest)?;
                    return Ok(NormalizedShortArg {
                        tokens: normalized_short_tokens_from_bytes(&kept_flags, &[]),
                        expects_option_value: false,
                        expects_verbose_value: false,
                    });
                }
            },
            byte => kept_flags.push(byte),
        }

        index += 1;
    }

    Ok(NormalizedShortArg {
        tokens: normalized_short_tokens_from_bytes(&kept_flags, &[]),
        expects_option_value: false,
        expects_verbose_value: false,
    })
}

#[cfg(unix)]
fn normalized_short_tokens_from_bytes(flags: &[u8], rest: &[u8]) -> Vec<OsString> {
    if flags.is_empty() {
        Vec::new()
    } else {
        let mut token = Vec::with_capacity(1 + flags.len() + rest.len());
        token.push(b'-');
        token.extend_from_slice(flags);
        token.extend_from_slice(rest);
        vec![OsString::from_vec(token)]
    }
}

#[cfg(not(unix))]
fn normalize_short_verbose_arg(arg: &OsString) -> Result<NormalizedShortArg, clap::Error> {
    let arg = arg.to_string_lossy();
    let cluster = &arg[1..];
    let mut kept_flags = String::new();

    for (index, flag) in cluster.char_indices() {
        if matches!(flag, 't' | 'd') {
            kept_flags.push(flag);
            let rest = &cluster[index + flag.len_utf8()..];
            kept_flags.push_str(rest);
            return Ok(NormalizedShortArg {
                tokens: normalized_short_tokens(kept_flags),
                expects_option_value: rest.is_empty(),
                expects_verbose_value: false,
            });
        }

        if flag == 'v' {
            let rest = &cluster[index + flag.len_utf8()..];
            if rest.is_empty() {
                return Ok(NormalizedShortArg {
                    tokens: normalized_short_tokens(kept_flags),
                    expects_option_value: false,
                    expects_verbose_value: true,
                });
            }
            if starts_with_verbose_numeric_value(rest) {
                parse_verbose_numeric_value(rest)?;
                return Ok(NormalizedShortArg {
                    tokens: normalized_short_tokens(kept_flags),
                    expects_option_value: false,
                    expects_verbose_value: false,
                });
            }
            continue;
        }

        kept_flags.push(flag);
    }

    Ok(NormalizedShortArg {
        tokens: normalized_short_tokens(kept_flags),
        expects_option_value: false,
        expects_verbose_value: false,
    })
}

#[cfg(not(unix))]
fn normalized_short_tokens(flags: String) -> Vec<OsString> {
    if flags.is_empty() {
        Vec::new()
    } else {
        vec![OsString::from(format!("-{flags}"))]
    }
}

#[cfg(unix)]
fn canonicalize_attached_long_arg(arg: &OsString, canonical: &str) -> OsString {
    let mut normalized = OsString::from(format!("--{canonical}="));
    if let Some(value_start) = arg
        .as_os_str()
        .as_bytes()
        .iter()
        .position(|byte| *byte == b'=')
    {
        normalized.push(std::ffi::OsStr::from_bytes(
            &arg.as_os_str().as_bytes()[value_start + 1..],
        ));
    }
    normalized
}

#[cfg(not(unix))]
fn canonicalize_attached_long_arg(arg: &OsString, canonical: &str) -> OsString {
    let arg = arg.to_string_lossy();
    let value = arg.split_once('=').map_or("", |(_, value)| value);
    OsString::from(format!("--{canonical}={value}"))
}

fn parse_verbose_numeric_value(level: &str) -> Result<u8, clap::Error> {
    if !is_verbose_numeric_token(level) {
        return Err(verbose_level_error(level));
    }

    Ok(level.parse::<u8>().unwrap_or(u8::MAX))
}

fn parse_stowrc_verbose_numeric_value(
    level: &str,
    origin: &StowrcTokenOrigin,
) -> Result<u8, clap::Error> {
    if !is_verbose_numeric_token(level) {
        return Err(stowrc_verbose_level_error(level, origin));
    }

    Ok(level.parse::<u8>().unwrap_or(u8::MAX))
}

fn is_verbose_numeric_token(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
}

fn starts_with_verbose_numeric_value(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_digit)
}

fn parse_verbose_level(argv: &[OsString]) -> Result<u8, clap::Error> {
    let mut verbosity = 0;
    let mut expecting_option_value = false;
    let mut after_double_dash = false;
    let mut args = argv.iter().skip(1).peekable();

    while let Some(arg) = args.next() {
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

        if let Some(long_token) = arg.strip_prefix("--") {
            let (key, attached_value) = long_token
                .split_once('=')
                .map_or((long_token, None), |(key, value)| (key, Some(value)));
            match resolve_long_option(key) {
                Ok(spec) => match spec.kind {
                    LongOptionKind::Value(_) => {
                        expecting_option_value = attached_value.is_none();
                    },
                    LongOptionKind::Verbose => {
                        if let Some(value) = attached_value {
                            verbosity = parse_verbose_numeric_value(value)?;
                        } else if let Some(next) = args.peek() {
                            let next = next.to_string_lossy();
                            if is_verbose_numeric_token(&next) {
                                verbosity = parse_verbose_numeric_value(&next)?;
                                args.next();
                            } else {
                                increment_verbose_level(&mut verbosity)?;
                            }
                        } else {
                            increment_verbose_level(&mut verbosity)?;
                        }
                    },
                    LongOptionKind::Bool
                    | LongOptionKind::Mode(_)
                    | LongOptionKind::Help
                    | LongOptionKind::Version => {},
                },
                Err(LongOptionResolveError::Ambiguous) => {
                    return Err(ambiguous_long_option_error(key));
                },
                Err(LongOptionResolveError::Unknown) => {},
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            if parse_short_verbose_cluster(&arg, &mut verbosity)? {
                if let Some(next) = args.peek() {
                    let next = next.to_string_lossy();
                    if is_verbose_numeric_token(&next) {
                        verbosity = parse_verbose_numeric_value(&next)?;
                        args.next();
                    }
                }
            }
            if short_option_cluster_consumes_value(&arg, &mut OperationMode::Stow) {
                expecting_option_value = short_option_cluster_needs_next_value(&arg);
            }
        }
    }

    Ok(verbosity)
}

fn help_or_version_arg(argv: &[OsString]) -> Option<OsString> {
    let mut expecting_option_value = false;
    let mut after_double_dash = false;
    let mut saw_help = false;
    let mut saw_version = false;

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

        if let Some(long_token) = arg.strip_prefix("--") {
            let (key, has_attached_value) = long_token
                .split_once('=')
                .map_or((long_token, false), |(key, _)| (key, true));
            let spec = match resolve_long_option(key) {
                Ok(spec) => spec,
                Err(_) => return None,
            };
            match spec.kind {
                LongOptionKind::Help => {
                    if has_attached_value {
                        return None;
                    }
                    saw_help = true;
                },
                LongOptionKind::Version => {
                    if has_attached_value {
                        return None;
                    }
                    saw_version = true;
                },
                LongOptionKind::Value(_) => {
                    expecting_option_value = !has_attached_value;
                },
                LongOptionKind::Verbose => {},
                LongOptionKind::Bool | LongOptionKind::Mode(_) => {
                    if has_attached_value {
                        return None;
                    }
                },
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            let cluster = &arg[1..];
            for (index, flag) in cluster.char_indices() {
                if !is_known_short_option(flag) {
                    return None;
                }

                if flag == 'h' {
                    saw_help = true;
                } else if flag == 'V' {
                    saw_version = true;
                }

                if flag == 'v' {
                    let rest = &cluster[index + flag.len_utf8()..];
                    if starts_with_verbose_numeric_value(rest) {
                        break;
                    }
                }

                if matches!(flag, 't' | 'd') {
                    expecting_option_value = index + flag.len_utf8() == cluster.len();
                    break;
                }
            }
            continue;
        }
    }

    if saw_help {
        Some(OsString::from("--help"))
    } else if saw_version {
        Some(OsString::from("--version"))
    } else {
        None
    }
}

fn is_known_short_option(flag: char) -> bool {
    SHORT_OPTION_SPECS.contains(&flag)
}

fn parse_short_verbose_cluster(arg: &str, verbosity: &mut u8) -> Result<bool, clap::Error> {
    let cluster = &arg[1..];

    for (index, flag) in cluster.char_indices() {
        if short_cluster_stops_value_parsing(flag) {
            break;
        }

        if flag == 'v' {
            let rest = &cluster[index + flag.len_utf8()..];
            if rest.is_empty() {
                increment_verbose_level(verbosity)?;
                return Ok(true);
            }
            if starts_with_verbose_numeric_value(rest) {
                *verbosity = parse_verbose_numeric_value(rest)?;
                return Ok(false);
            }
            increment_verbose_level(verbosity)?;
        }
    }

    Ok(false)
}

fn short_verbose_cluster_accepts_next_value(arg: &str) -> bool {
    let cluster = &arg[1..];

    for (index, flag) in cluster.char_indices() {
        if short_cluster_stops_value_parsing(flag) {
            return false;
        }

        if flag == 'v' {
            let rest = &cluster[index + flag.len_utf8()..];
            if rest.is_empty() {
                return true;
            }
            if starts_with_verbose_numeric_value(rest) {
                return false;
            }
        }
    }

    false
}

fn short_cluster_stops_value_parsing(flag: char) -> bool {
    matches!(flag, 't' | 'd')
}

fn increment_verbose_level(verbosity: &mut u8) -> Result<(), clap::Error> {
    *verbosity = verbosity.saturating_add(1);
    Ok(())
}

fn verbose_level_error(value: &str) -> clap::Error {
    clap::Error::raw(
        clap::error::ErrorKind::InvalidValue,
        format!("verbosity level must be a non-negative integer: {value}"),
    )
}

fn parse_operation_groups(argv: &[OsString]) -> Vec<OperationGroup> {
    let mut groups: Vec<OperationGroup> = Vec::new();
    let mut current_mode = OperationMode::Stow;
    let mut expecting_option_value = false;
    let mut expecting_verbose_value = false;
    let mut after_double_dash = false;

    for arg in argv.iter().skip(1) {
        let arg = arg.to_string_lossy();

        if expecting_option_value {
            expecting_option_value = false;
            continue;
        }

        if expecting_verbose_value {
            expecting_verbose_value = false;
            if is_verbose_numeric_token(&arg) {
                continue;
            }
        }

        if after_double_dash {
            push_package_operation(&mut groups, current_mode, arg.into_owned());
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
            if let Some(long_token) = arg.strip_prefix("--") {
                let (key, attached_value) = long_token
                    .split_once('=')
                    .map_or((long_token, None), |(key, value)| (key, Some(value)));
                if let Ok(spec) = resolve_long_option(key) {
                    match spec.kind {
                        LongOptionKind::Mode(mode) if attached_value.is_none() => {
                            current_mode = mode;
                        },
                        LongOptionKind::Verbose if attached_value.is_none() => {
                            expecting_verbose_value = true;
                        },
                        _ => {},
                    }
                }
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            if short_option_cluster_consumes_value(&arg, &mut current_mode) {
                expecting_option_value = short_option_cluster_needs_next_value(&arg);
            } else {
                expecting_verbose_value = short_verbose_cluster_accepts_next_value(&arg);
            }
            continue;
        }

        push_package_operation(&mut groups, current_mode, arg.into_owned());
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

fn short_operation_mode(flag: char) -> Option<OperationMode> {
    match flag {
        'S' => Some(OperationMode::Stow),
        'D' => Some(OperationMode::Delete),
        'R' => Some(OperationMode::Restow),
        _ => None,
    }
}

fn is_option_requiring_separate_value(arg: &str) -> bool {
    if matches!(arg, "-t" | "-d") {
        return true;
    }

    let Some(long_token) = arg.strip_prefix("--") else {
        return false;
    };
    let (key, has_attached_value) = long_token
        .split_once('=')
        .map_or((long_token, false), |(key, _)| (key, true));

    !has_attached_value
        && matches!(
            resolve_long_option(key).map(|spec| spec.kind),
            Ok(LongOptionKind::Value(_))
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
    use std::collections::BTreeSet;
    use std::fs::{self, File};
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn process_env_lock() -> crate::test_sync::IsolatedProcessEnv {
        crate::test_sync::IsolatedProcessEnv::new()
    }

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

    #[cfg(unix)]
    struct OsEnvVarGuard {
        key: &'static str,
        original_value: Option<OsString>,
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

    #[cfg(unix)]
    impl OsEnvVarGuard {
        fn new(key: &'static str, value: &std::ffi::OsStr) -> Self {
            let original_value = std::env::var_os(key);
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
            if let Err(err) = std::env::set_current_dir(&self.original) {
                eprintln!(
                    "warning: failed to restore current directory {}: {}",
                    self.original.display(),
                    err
                );
            }
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

    #[cfg(unix)]
    impl Drop for OsEnvVarGuard {
        fn drop(&mut self) {
            match &self.original_value {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[cfg(unix)]
    fn non_utf8_child(parent: &std::path::Path, child: &[u8]) -> PathBuf {
        let mut bytes = parent.as_os_str().as_bytes().to_vec();
        bytes.push(b'/');
        bytes.extend_from_slice(child);
        PathBuf::from(OsString::from_vec(bytes))
    }

    fn write_file(path: &std::path::Path, content: &str) -> std::path::PathBuf {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path.to_path_buf()
    }

    fn parse_runtime_args<I, T>(itr: I) -> Args
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Args::parse_runtime_from_with_operation_groups(itr)
            .parsed_args
            .args
    }

    fn try_parse_runtime_args<I, T>(itr: I) -> Result<Args, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Args::try_parse_runtime_from_with_operation_groups(itr)
            .map(|parsed| parsed.parsed_args.args)
    }

    fn parse_runtime_with_operation_groups<I, T>(itr: I) -> ParsedArgs
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        Args::parse_runtime_from_with_operation_groups(itr).parsed_args
    }

    #[test]
    fn test_compat_option_tables_cover_clap_options() {
        let command = <Args as clap::CommandFactory>::command();
        let mut long_options = BTreeSet::new();
        let mut long_value_options = BTreeSet::new();
        let mut short_options = BTreeSet::new();
        let mut short_value_options = BTreeSet::new();

        for arg in command.get_arguments() {
            let takes_value = matches!(
                arg.get_action(),
                clap::ArgAction::Set | clap::ArgAction::Append
            );
            if let Some(options) = arg.get_long_and_visible_aliases() {
                for option in options {
                    long_options.insert(option.to_string());
                    if takes_value {
                        long_value_options.insert(option.to_string());
                    }
                }
            }
            if let Some(options) = arg.get_aliases() {
                for option in options {
                    long_options.insert(option.to_string());
                    if takes_value {
                        long_value_options.insert(option.to_string());
                    }
                }
            }
            if let Some(options) = arg.get_short_and_visible_aliases() {
                for option in options {
                    short_options.insert(option);
                    if takes_value {
                        short_value_options.insert(option);
                    }
                }
            }
        }
        long_options.extend(["help".to_string(), "version".to_string()]);
        short_options.extend(['h', 'V']);

        let compat_long_options = LONG_OPTION_SPECS
            .iter()
            .map(|spec| spec.name.to_string())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            compat_long_options, long_options,
            "compat long option table must match clap-visible options exactly"
        );

        let compat_short_options = SHORT_OPTION_SPECS.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            compat_short_options, short_options,
            "compat short option table must match clap-visible options exactly"
        );

        for option in &long_options {
            assert!(
                is_known_long_option(option),
                "compat parser must know --{}",
                option
            );
            assert_eq!(
                matches!(
                    resolve_long_option(option).map(|spec| spec.kind),
                    Ok(LongOptionKind::Value(_))
                ),
                long_value_options.contains(option),
                "compat parser value shape must match --{}",
                option
            );
        }
        for option in &short_options {
            assert!(
                is_known_short_option(*option),
                "compat parser must know -{}",
                option
            );
            assert_eq!(
                matches!(*option, 't' | 'd'),
                short_value_options.contains(option),
                "compat parser value shape must match -{}",
                option
            );
        }

        let expected_long_specs = [
            (
                "target",
                "target",
                LongOptionKind::Value(ResourceValueOption::Target),
            ),
            (
                "dir",
                "dir",
                LongOptionKind::Value(ResourceValueOption::Dir),
            ),
            ("stow", "stow", LongOptionKind::Mode(OperationMode::Stow)),
            (
                "delete",
                "delete",
                LongOptionKind::Mode(OperationMode::Delete),
            ),
            (
                "restow",
                "restow",
                LongOptionKind::Mode(OperationMode::Restow),
            ),
            ("adopt", "adopt", LongOptionKind::Bool),
            ("no-folding", "no-folding", LongOptionKind::Bool),
            ("dotfiles", "dotfiles", LongOptionKind::Bool),
            ("compat", "compat", LongOptionKind::Bool),
            (
                "override",
                "override",
                LongOptionKind::Value(ResourceValueOption::Override),
            ),
            (
                "defer",
                "defer",
                LongOptionKind::Value(ResourceValueOption::Defer),
            ),
            (
                "ignore",
                "ignore",
                LongOptionKind::Value(ResourceValueOption::Ignore),
            ),
            ("simulate", "simulate", LongOptionKind::Bool),
            ("no", "simulate", LongOptionKind::Bool),
            ("verbose", "verbose", LongOptionKind::Verbose),
            ("help", "help", LongOptionKind::Help),
            ("version", "version", LongOptionKind::Version),
        ];

        for (name, canonical, kind) in expected_long_specs {
            let spec = resolve_long_option(name).unwrap();
            assert_eq!(spec.canonical, canonical, "--{} canonical drifted", name);
            assert_eq!(spec.kind, kind, "--{} kind drifted", name);
        }
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-D", "mypackage"]);
        assert!(args.delete);
        assert!(!args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_runtime_long_mode_options_are_valid_cli_options() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        let parsed_args =
            Args::parse_runtime_from_with_operation_groups(["rustow", "--delete", "mypackage"]);

        assert!(parsed_args.parsed_args.args.delete);
        assert_eq!(
            parsed_args.parsed_args.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Delete,
                packages: vec!["mypackage".to_string()],
            }]
        );
    }

    #[test]
    fn test_restow_option() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-R", "mypackage"]);
        assert!(args.restow);
        assert!(!args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_target_and_dir_options() {
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let args_long_six = Args::parse_from(["rustow", "--verbose=6", "mypackage"]);
        assert_eq!(args_long_six.verbose, 6);
        assert!(Args::try_parse_from(["rustow", "--verbose=invalid", "mypackage"]).is_err());
    }

    #[test]
    fn test_verbose_optional_numeric_values_match_gnu() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "--verbose", "2", "mypackage"]);
        assert_eq!(args.verbose, 2);
        assert_eq!(args.packages, vec!["mypackage"]);

        let args = Args::parse_from(["rustow", "--verbose", "0", "mypackage"]);
        assert_eq!(args.verbose, 0);
        assert_eq!(args.packages, vec!["mypackage"]);

        let args = Args::parse_from(["rustow", "-v", "0", "mypackage"]);
        assert_eq!(args.verbose, 0);
        assert_eq!(args.packages, vec!["mypackage"]);

        let args = Args::parse_from(["rustow", "-v2", "mypackage"]);
        assert_eq!(args.verbose, 2);
        assert_eq!(args.packages, vec!["mypackage"]);

        let args = Args::parse_from(["rustow", "-pv2", "mypackage"]);
        assert!(args.compat);
        assert_eq!(args.verbose, 2);
        assert_eq!(args.packages, vec!["mypackage"]);

        let args = Args::parse_from(["rustow", "--verbose", "6", "mypackage"]);
        assert_eq!(args.verbose, 6);
        assert_eq!(args.packages, vec!["mypackage"]);

        let args = Args::parse_from(["rustow", "-v6", "mypackage"]);
        assert_eq!(args.verbose, 6);
        assert_eq!(args.packages, vec!["mypackage"]);

        let args = Args::parse_from(["rustow", "--verbose", "999", "mypackage"]);
        assert_eq!(args.verbose, u8::MAX);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_verbose_option_cluster_with_compat_before() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-pv", "mypackage"]);

        assert!(args.compat);
        assert_eq!(args.verbose, 1);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_verbose_option_cluster_with_compat_after() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-vp", "mypackage"]);

        assert!(args.compat);
        assert_eq!(args.verbose, 1);
    }

    #[test]
    fn test_verbose_non_numeric_value_reports_error() {
        let _lock = process_env_lock();
        let error = Args::try_parse_from(["rustow", "--verbose=invalid", "mypackage"]).unwrap_err();

        assert!(error.to_string().contains("non-negative integer"));

        let error = Args::try_parse_from(["rustow", "-v2x", "mypackage"]).unwrap_err();
        assert!(error.to_string().contains("non-negative integer"));
    }

    #[test]
    fn test_long_option_abbreviations_match_gnu() {
        let _lock = process_env_lock();
        let args = Args::parse_from([
            "rustow",
            "--targ",
            "/target/dir",
            "--sim",
            "--no-f",
            "mypackage",
        ]);

        assert_eq!(args.target, Some(PathBuf::from("/target/dir")));
        assert!(args.simulate);
        assert!(args.no_folding);
        assert_eq!(args.packages, vec!["mypackage"]);

        let error = Args::try_parse_from(["rustow", "--ver", "mypackage"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Option ver is ambiguous (verbose, version)")
        );
    }

    #[test]
    fn test_hyphen_prefixed_option_values_are_preserved() {
        let _lock = process_env_lock();
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
    fn test_hyphen_prefixed_separate_option_values_are_preserved() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-d", "-D", "mypackage"]);
        assert_eq!(args.dir, Some(PathBuf::from("-D")));
        assert!(!args.delete);

        let args = Args::parse_from(["rustow", "--target", "--restow", "mypackage"]);
        assert_eq!(args.target, Some(PathBuf::from("--restow")));
        assert!(!args.restow);

        let args = Args::parse_from(["rustow", "--ignore", "-S", "mypackage"]);
        assert_eq!(args.ignore_patterns, vec!["-S"]);
        assert!(!args.stow);

        let args = Args::parse_from(["rustow", "-d", "--simulate", "mypackage"]);
        assert_eq!(args.dir, Some(PathBuf::from("--simulate")));
        assert!(!args.simulate);

        let args = Args::parse_from(["rustow", "-d", "-n", "mypackage"]);
        assert_eq!(args.dir, Some(PathBuf::from("-n")));
        assert!(!args.simulate);

        let args = Args::parse_from(["rustow", "--target", "--adopt", "mypackage"]);
        assert_eq!(args.target, Some(PathBuf::from("--adopt")));
        assert!(!args.adopt);

        let args = Args::parse_from(["rustow", "-d", "--verbose", "mypackage"]);
        assert_eq!(args.dir, Some(PathBuf::from("--verbose")));
        assert_eq!(args.verbose, 0);

        let args = Args::parse_from(["rustow", "--target", "--verbose", "mypackage"]);
        assert_eq!(args.target, Some(PathBuf::from("--verbose")));
        assert_eq!(args.verbose, 0);

        let args = Args::parse_from(["rustow", "--defer", "--verbose", "mypackage"]);
        assert_eq!(args.defer_conflicts, vec!["--verbose"]);
        assert_eq!(args.verbose, 0);

        let args = Args::parse_from(["rustow", "-Dt", "--verbose", "mypackage"]);
        assert!(args.delete);
        assert_eq!(args.target, Some(PathBuf::from("--verbose")));
        assert_eq!(args.verbose, 0);

        let args = Args::parse_from(["rustow", "-Dt", "--help", "mypackage"]);
        assert!(args.delete);
        assert_eq!(args.target, Some(PathBuf::from("--help")));

        let args = Args::parse_from(["rustow", "-Sd", "-n", "mypackage"]);
        assert!(args.stow);
        assert_eq!(args.dir, Some(PathBuf::from("-n")));
        assert!(!args.simulate);

        let args = Args::parse_from(["rustow", "-Rt", "--simulate", "mypackage"]);
        assert!(args.restow);
        assert_eq!(args.target, Some(PathBuf::from("--simulate")));
        assert!(!args.simulate);

        let args = Args::parse_from(["rustow", "--defer", "--help", "mypackage"]);
        assert_eq!(args.defer_conflicts, vec!["--help"]);

        let args = Args::parse_from(["rustow", "--override", "-V", "mypackage"]);
        assert_eq!(args.override_conflicts, vec!["-V"]);
    }

    #[test]
    fn test_reserved_flags_can_be_passed_as_explicit_hyphen_values() {
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
    fn test_help_takes_precedence_over_verbose_value() {
        let _lock = process_env_lock();
        let error = Args::try_parse_from(["rustow", "--help", "--verbose=6"]).unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn test_help_takes_precedence_after_packages() {
        let _lock = process_env_lock();
        let error = Args::try_parse_from(["rustow", "mypackage", "--help"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);

        let error = Args::try_parse_from(["rustow", "mypackage", "-h"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);

        let error = Args::try_parse_from(["rustow", "mypackage", "-V"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);

        let error = Args::try_parse_from(["rustow", "--version", "--help"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);

        let error = Args::try_parse_from(["rustow", "-Vh"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);

        let error = Args::try_parse_from(["rustow", "-hV"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn test_unknown_option_prevents_help_or_version_precedence() {
        let _lock = process_env_lock();
        let error = Args::try_parse_from(["rustow", "--help", "--bad-option"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("Unknown option: bad-option"));

        let error = Args::try_parse_from(["rustow", "--version", "--bad-option"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("Unknown option: bad-option"));
    }

    #[test]
    fn test_invalid_option_precedes_invalid_verbose_for_public_and_runtime_parsers() {
        let _lock = process_env_lock();
        let public =
            Args::try_parse_from(["rustow", "--bad-option", "--verbose=bad", "pkg"]).unwrap_err();
        assert_eq!(public.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(public.to_string().contains("Unknown option: bad-option"));
        assert!(!public.to_string().contains("verbosity level"));

        let runtime = Args::try_parse_runtime_from_with_operation_groups([
            "rustow",
            "--bad-option",
            "--verbose=bad",
            "pkg",
        ])
        .unwrap_err();
        assert_eq!(runtime.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(runtime.to_string().contains("Unknown option: bad-option"));
        assert!(!runtime.to_string().contains("verbosity level"));
    }

    #[test]
    fn test_try_parse_from_with_operation_groups_returns_parse_errors() {
        let _lock = process_env_lock();
        let error =
            Args::try_parse_from_with_operation_groups(["rustow", "--verbose=bad", "mypackage"])
                .unwrap_err();

        assert!(error.to_string().contains("non-negative integer"));
    }

    #[test]
    fn test_verbose_numeric_option_sets_level_in_argument_order() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-v", "--verbose=0", "mypackage"]);
        assert_eq!(args.verbose, 0);

        let args = Args::parse_from(["rustow", "--verbose=2", "-v", "mypackage"]);
        assert_eq!(args.verbose, 3);

        let args = Args::parse_from(["rustow", "--verbose=2", "--verbose=1", "mypackage"]);
        assert_eq!(args.verbose, 1);
    }

    #[test]
    fn test_verbose_numeric_option_after_double_dash_is_package() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "--", "--verbose=0"]);
        assert_eq!(args.verbose, 0);
        assert_eq!(args.packages, vec!["--verbose=0"]);
    }

    #[test]
    fn test_multiple_packages() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "pkg1", "pkg2", "pkg3"]);
        assert_eq!(args.packages, vec!["pkg1", "pkg2", "pkg3"]);
    }

    #[test]
    fn test_simulate_option() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-n", "mypackage"]);
        assert!(args.simulate);
        let args_long = Args::parse_from(["rustow", "--simulate", "mypackage"]);
        assert!(args_long.simulate);
        let args_alias = Args::parse_from(["rustow", "--no", "mypackage"]);
        assert!(args_alias.simulate);
    }

    #[test]
    fn test_override_defer_options() {
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "--compat", "mypackage"]);
        assert!(args.compat);

        let args = Args::parse_from(["rustow", "-p", "mypackage"]);
        assert!(args.compat);
    }

    #[test]
    fn test_stow_dir_from_env() {
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new(); // Ensure STOW_DIR is clear initially
        unsafe {
            std::env::set_var("STOW_DIR", "/env/stow/path");
        }
        let args = Args::parse_from(["rustow", "-d", "/cmd/stow/path", "mypackage"]);
        assert_eq!(args.dir, Some(PathBuf::from("/cmd/stow/path")));
    }

    #[test]
    fn test_stow_option_short() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-S", "mypackage"]);
        assert!(args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_stow_option_long() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "--stow", "mypackage"]);
        assert!(args.stow);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_ignore_option_single() {
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "--ignore=\\.git", "mypackage"]);
        assert_eq!(args.ignore_patterns, vec!["\\.git"]);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_parse_operation_groups_mixed_modes() {
        let _lock = process_env_lock();
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
    fn test_parse_operation_groups_skips_verbose_optional_numeric_values() {
        let _lock = process_env_lock();
        let parsed_args = Args::parse_from_with_operation_groups([
            "rustow",
            "--delete",
            "--verbose",
            "2",
            "old",
            "--stow",
            "-v0",
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
    fn test_parse_operation_groups_defaults_to_stow_until_mode_changes() {
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
        let args = Args::parse_from(["rustow", "-tv", "mypackage"]);
        assert_eq!(args.target, Some(PathBuf::from("v")));
        assert_eq!(args.verbose, 0);
        assert_eq!(args.packages, vec!["mypackage"]);
    }

    #[test]
    fn test_short_cluster_mode_value_attached_to_target() {
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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
        let _lock = process_env_lock();
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

        let pure_args = Args::parse_from(["rustow", "my-package"]);
        assert_eq!(pure_args.dir, None);
        assert_eq!(pure_args.target, None);
        assert!(pure_args.ignore_patterns.is_empty());

        let args = parse_runtime_args(["rustow", "my-package"]);
        assert_eq!(args.dir, Some(PathBuf::from("/local")));
        assert_eq!(args.target, Some(PathBuf::from("/local_target")));
        assert_eq!(args.ignore_patterns, vec!["home", "local"]);
        assert_eq!(args.packages, vec!["my-package"]);

        let args_override = parse_runtime_args(["rustow", "--dir", "/cli-dir", "my-package"]);
        assert_eq!(args_override.dir, Some(PathBuf::from("/cli-dir")));
        drop(home_guard);
    }

    #[test]
    fn test_stowrc_verbose_optional_numeric_values_match_gnu() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--verbose 2\n");
        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.verbose, 2);
        assert_eq!(args.packages, vec!["pkg"]);

        write_file(&cwd.join(".stowrc"), "--verbose 6\n");
        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.verbose, 6);
        assert_eq!(args.packages, vec!["pkg"]);

        write_file(&cwd.join(".stowrc"), "--verbose 999\n");
        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.verbose, u8::MAX);
        assert_eq!(args.packages, vec!["pkg"]);

        write_file(&cwd.join(".stowrc"), "-v2\n");
        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.verbose, 2);
        assert_eq!(args.packages, vec!["pkg"]);
    }

    #[test]
    fn test_stowrc_long_option_abbreviations_match_gnu() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            "--targ=/abbr-target\n--sim\n--verb 2\n",
        );
        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.target, Some(PathBuf::from("/abbr-target")));
        assert!(args.simulate);
        assert_eq!(args.verbose, 2);
        assert_eq!(args.packages, vec!["pkg"]);

        write_file(&cwd.join(".stowrc"), "--ver\n");
        let error = try_parse_runtime_args(["rustow", "pkg"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Option ver is ambiguous (verbose, version)")
        );
        assert!(error.to_string().contains("./.stowrc:1"));

        write_file(&cwd.join(".stowrc"), "--bad-option\n");
        let error = try_parse_runtime_args(["rustow", "pkg"]).unwrap_err();
        assert!(error.to_string().contains("Unknown option: bad-option"));
        assert!(error.to_string().contains("./.stowrc:1"));
    }

    #[test]
    fn test_stowrc_double_dash_ignores_following_options() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            "--ignore=before\n--\n--dir=/after\n--target=/after\n--ignore=after\n",
        );

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, None);
        assert_eq!(args.target, None);
        assert_eq!(args.ignore_patterns, vec!["before"]);
        assert_eq!(args.packages, vec!["pkg"]);
    }

    #[test]
    fn test_stowrc_missing_value_does_not_consume_cli_package() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--target\n");

        let error = try_parse_runtime_args(["rustow", "pkg"]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("resource file option '--target' requires a value")
        );
        assert!(error.to_string().contains("./.stowrc:1"));
    }

    #[test]
    fn test_stowrc_missing_value_does_not_cross_resource_file_boundary() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("secret-home-from-stowrc-origin");
        let cwd = temp_dir.path().join("secret-cwd-from-stowrc-origin");
        fs::create_dir_all(&home_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&home_dir.join(".stowrc"), "--target\n");
        write_file(&cwd.join(".stowrc"), "--dir=/local-stow\n");

        let error = try_parse_runtime_args(["rustow", "pkg"]).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("resource file option '--target' requires a value"));
        assert!(message.contains("~/.stowrc:1"));
        assert!(!message.contains("secret-home-from-stowrc-origin"));
        assert!(!message.contains("secret-cwd-from-stowrc-origin"));
    }

    #[test]
    fn test_stowrc_trailing_verbose_does_not_consume_numeric_cli_package() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--verbose\n");
        let args = parse_runtime_args(["rustow", "2"]);
        assert_eq!(args.verbose, 1);
        assert_eq!(args.packages, vec!["2"]);

        write_file(&cwd.join(".stowrc"), "-v\n");
        let args = parse_runtime_args(["rustow", "2"]);
        assert_eq!(args.verbose, 1);
        assert_eq!(args.packages, vec!["2"]);
    }

    #[test]
    fn test_public_parse_from_ignores_stowrc_resource_files() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--target\n--dir=/from-stowrc\n");

        let args = Args::parse_from(["rustow", "pkg"]);
        assert_eq!(args.dir, None);
        assert_eq!(args.target, None);
        assert_eq!(args.packages, vec!["pkg"]);
    }

    #[test]
    fn test_public_parse_from_with_operation_groups_ignores_stowrc_resource_files() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--target\n--dir=/from-stowrc\n");

        let parsed = Args::parse_from_with_operation_groups(["rustow", "--delete", "pkg"]);
        assert_eq!(parsed.args.dir, None);
        assert_eq!(parsed.args.target, None);
        assert_eq!(
            parsed.operation_groups,
            vec![OperationGroup {
                mode: OperationMode::Delete,
                packages: vec!["pkg".to_string()],
            }]
        );
    }

    #[test]
    fn test_stowrc_undefined_env_in_final_path_errors() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        unsafe {
            std::env::remove_var("RUSTOW_UNDEFINED_STOWRC_VAR");
        }

        write_file(&cwd.join(".stowrc"), "--dir=$RUSTOW_UNDEFINED_STOWRC_VAR\n");

        let error = try_parse_runtime_args(["rustow", "pkg"]).unwrap_err();
        assert!(error.to_string().contains(
            "--dir option references undefined environment variable $RUSTOW_UNDEFINED_STOWRC_VAR; aborting!"
        ));
    }

    #[test]
    fn test_stowrc_lower_priority_undefined_env_does_not_override_later_path() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&home_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        unsafe {
            std::env::remove_var("RUSTOW_LOW_PRIORITY_UNDEFINED");
        }

        write_file(
            &home_dir.join(".stowrc"),
            "--dir=$RUSTOW_LOW_PRIORITY_UNDEFINED\n",
        );
        write_file(&cwd.join(".stowrc"), "--dir=/local\n");

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(PathBuf::from("/local")));
    }

    #[test]
    fn test_stowrc_final_undefined_env_errors_even_with_cli_override() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        unsafe {
            std::env::remove_var("RUSTOW_CLI_OVERRIDE_UNDEFINED");
        }

        write_file(
            &cwd.join(".stowrc"),
            "--dir=$RUSTOW_CLI_OVERRIDE_UNDEFINED\n",
        );

        let error = try_parse_runtime_args(["rustow", "--dir", "/cli-dir", "pkg"]).unwrap_err();
        assert!(error.to_string().contains(
            "--dir option references undefined environment variable $RUSTOW_CLI_OVERRIDE_UNDEFINED; aborting!"
        ));
    }

    #[test]
    fn test_stowrc_home_file_is_not_read_when_home_is_unset() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let fallback_home = temp_dir.path().join("fallback-home");
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&fallback_home).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let _logdir_guard = EnvVarGuard::new("LOGDIR", fallback_home.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        unsafe {
            std::env::remove_var("HOME");
        }

        write_file(&fallback_home.join(".stowrc"), "--dir=/from-logdir\n");
        write_file(&cwd.join(".stowrc"), "--target=/from-current\n");

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, None);
        assert_eq!(args.target, Some(PathBuf::from("/from-current")));
    }

    #[test]
    fn test_stowrc_home_path_matches_gnu_when_home_is_empty() {
        assert_eq!(
            home_stowrc_path(OsString::from("")),
            PathBuf::from("/.stowrc")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_unreadable_stowrc_is_skipped() {
        let _lock = process_env_lock();
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        let stowrc = cwd.join(".stowrc");
        write_file(&stowrc, "--dir=/unreadable\n");
        fs::set_permissions(&stowrc, fs::Permissions::from_mode(0o000)).unwrap();

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, None);

        fs::set_permissions(&stowrc, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn test_non_regular_stowrc_is_skipped() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(cwd.join(".stowrc")).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, None);
    }

    #[test]
    fn test_cli_help_ignores_malformed_stowrc() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--target\n");

        let error =
            Args::try_parse_runtime_from_with_operation_groups(["rustow", "--help"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn test_invalid_cli_option_takes_precedence_over_malformed_stowrc() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--target\n");

        let error =
            Args::try_parse_runtime_from_with_operation_groups(["rustow", "--bad-option", "pkg"])
                .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("Unknown option: bad-option"));
        assert!(!error.to_string().contains("resource file option"));
    }

    #[test]
    fn test_invalid_cli_option_takes_precedence_over_undefined_stowrc_env() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        unsafe {
            std::env::remove_var("RUSTOW_UNDEFINED_STOWRC_VAR");
        }

        write_file(&cwd.join(".stowrc"), "--dir=$RUSTOW_UNDEFINED_STOWRC_VAR\n");

        let error =
            Args::try_parse_runtime_from_with_operation_groups(["rustow", "--bad-option", "pkg"])
                .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        assert!(error.to_string().contains("Unknown option: bad-option"));
        assert!(
            !error
                .to_string()
                .contains("undefined environment variable $RUSTOW_UNDEFINED_STOWRC_VAR")
        );
    }

    #[test]
    fn test_invalid_short_clusters_with_help_or_version_do_not_display_help() {
        let _lock = process_env_lock();

        for cluster in ["-xh", "-hx", "-xV", "-Vx"] {
            let error =
                Args::try_parse_runtime_from_with_operation_groups(["rustow", cluster, "pkg"])
                    .unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
            assert!(error.to_string().contains("-x"));
        }
    }

    #[test]
    fn test_stowrc_ignores_mode_flags_and_package_names() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "-D\npkg-from-rc\n--ignore=from-rc\n");

        let args = parse_runtime_args(["rustow", "cli-pkg"]);
        assert!(!args.delete);
        assert_eq!(args.ignore_patterns, vec!["from-rc"]);
        assert_eq!(args.packages, vec!["cli-pkg"]);

        let grouped = parse_runtime_with_operation_groups(["rustow", "cli-pkg"]);
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
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        fs::create_dir_all(&home_dir).unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();

        let home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _target_guard = EnvVarGuard::new("RUSTOW_TARGET_FROM_ENV", "~/env_target");
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            "--dir=\"$HOME/.stowrc dir\"\n--target=$RUSTOW_TARGET_FROM_ENV\n",
        );

        let args = parse_runtime_args(["rustow", "pkg"]);
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
                "{}/env_target",
                home_dir.to_string_lossy()
            )))
        );

        write_file(&cwd.join(".stowrc"), "--dir=\\\\$HOME/.stowrc_noparse\n");
        let with_escaped_home = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(
            with_escaped_home.dir,
            Some(PathBuf::from("$HOME/.stowrc_noparse"))
        );
        drop(home_guard);
    }

    #[cfg(unix)]
    #[test]
    fn test_stowrc_env_path_preserves_non_utf8_value() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        let stow_dir = non_utf8_child(temp_dir.path(), b"stow-\xff");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        let _stow_dir_guard = OsEnvVarGuard::new("RUSTOW_NONUTF_STOW_DIR", stow_dir.as_os_str());

        write_file(&cwd.join(".stowrc"), "--dir=$RUSTOW_NONUTF_STOW_DIR\n");

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(stow_dir));
    }

    #[cfg(unix)]
    #[test]
    fn test_stowrc_short_attached_path_options_preserve_non_utf8_values() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        let stow_dir = non_utf8_child(temp_dir.path(), b"stow-\xff");
        let target_dir = non_utf8_child(temp_dir.path(), b"target-\xfe");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        let mut stowrc = b"-d".to_vec();
        stowrc.extend_from_slice(stow_dir.as_os_str().as_bytes());
        stowrc.extend_from_slice(b"\n-pt");
        stowrc.extend_from_slice(target_dir.as_os_str().as_bytes());
        stowrc.push(b'\n');
        fs::write(cwd.join(".stowrc"), stowrc).unwrap();

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(stow_dir));
        assert_eq!(args.target, Some(target_dir));
        assert!(args.compat);
    }

    #[cfg(unix)]
    #[test]
    fn test_stowrc_raw_path_expands_env_before_non_utf8_suffix() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        let home_dir = temp_dir.path().join("home");
        let stow_dir = non_utf8_child(&home_dir, b"stow-\xff");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&home_dir).unwrap();
        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        let mut stowrc = b"-d$HOME/".to_vec();
        stowrc.extend_from_slice(b"stow-\xff\n");
        fs::write(cwd.join(".stowrc"), stowrc).unwrap();

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(stow_dir));
    }

    #[cfg(unix)]
    #[test]
    fn test_short_attached_path_options_preserve_non_utf8_values() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let stow_dir = non_utf8_child(temp_dir.path(), b"stow-\xff");
        let target_dir = non_utf8_child(temp_dir.path(), b"target-\xfe");

        let mut dir_arg = b"-d".to_vec();
        dir_arg.extend_from_slice(stow_dir.as_os_str().as_bytes());
        let mut target_arg = b"-pt".to_vec();
        target_arg.extend_from_slice(target_dir.as_os_str().as_bytes());

        let args = Args::parse_from([
            OsString::from("rustow"),
            OsString::from_vec(dir_arg),
            OsString::from_vec(target_arg),
            OsString::from("pkg"),
        ]);

        assert_eq!(args.dir, Some(stow_dir));
        assert_eq!(args.target, Some(target_dir));
        assert!(args.compat);
    }

    #[cfg(unix)]
    #[test]
    fn test_stowrc_tilde_path_preserves_non_utf8_home() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        let home_dir = non_utf8_child(temp_dir.path(), b"home-\xff");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        let _home_guard = OsEnvVarGuard::new("HOME", home_dir.as_os_str());

        write_file(&cwd.join(".stowrc"), "--dir=~/stow\n");

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(home_dir.join("stow")));
    }

    #[test]
    fn test_runtime_args_debug_redacts_env_expanded_path_values() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        let cwd = temp_dir.path().join("cwd");
        let secret_dir = temp_dir.path().join("secret-value-from-env");
        fs::create_dir_all(&home_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _secret_guard =
            EnvVarGuard::new("RUSTOW_SECRET_STOW_DIR", secret_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);
        write_file(
            &cwd.join(".stowrc"),
            "--dir=$RUSTOW_SECRET_STOW_DIR/missing\n",
        );

        let parsed = Args::parse_runtime_from_with_operation_groups(["rustow", "pkg"]);
        let debug = format!("{:?}", parsed);
        assert!(!debug.contains("secret-value-from-env"));
        assert!(debug.contains("$RUSTOW_SECRET_STOW_DIR/missing"));
    }

    #[test]
    fn test_stowrc_parse_local_file_with_quoted_value() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        fs::create_dir_all(&home_dir).unwrap();
        let stow_dir = temp_dir.path().join("stow").join("quotedpkg");
        let target_dir = home_dir.join("target");
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            &format!(
                "--dir=\"{}\"\n--target=\"$HOME/target\"\n",
                stow_dir.to_string_lossy()
            ),
        );

        let args = parse_runtime_args(["rustow", "quotedpkg"]);
        assert_eq!(args.dir, Some(stow_dir.clone()));
        assert_eq!(args.target, Some(target_dir));
    }

    #[test]
    fn test_stowrc_tokenization_handles_quotes_and_comments() {
        let _lock = process_env_lock();
        assert_eq!(
            tokenize_stowrc_line(r#"--ignore='one # two' "three four" # comment"#).unwrap(),
            vec![
                "--ignore=one # two".to_string(),
                "three four".to_string(),
                "#".to_string(),
                "comment".to_string()
            ]
        );
    }

    #[test]
    fn test_stowrc_hash_tokens_match_gnu_shellwords() {
        let _lock = process_env_lock();
        assert_eq!(
            tokenize_stowrc_line(r#"# --dir=/shellwords-token"#).unwrap(),
            vec!["#".to_string(), "--dir=/shellwords-token".to_string()]
        );
    }

    #[test]
    fn test_stowrc_trailing_backslash_matches_gnu_shellwords() {
        let _lock = process_env_lock();
        assert!(tokenize_stowrc_line(r"--dir=/ignored\").unwrap().is_empty());
    }

    #[test]
    fn test_stowrc_hash_prefixed_options_match_gnu_shellwords() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "# --dir=/shellwords-dir\n");

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(PathBuf::from("/shellwords-dir")));
    }

    #[test]
    fn test_stowrc_tokenization_preserves_empty_quoted_values() {
        let _lock = process_env_lock();
        assert_eq!(
            tokenize_stowrc_line(r#"--ignore "" --target ''"#).unwrap(),
            vec![
                "--ignore".to_string(),
                "".to_string(),
                "--target".to_string(),
                "".to_string(),
            ]
        );
    }

    #[test]
    fn test_stowrc_unterminated_quotes_match_gnu_shellwords() {
        let _lock = process_env_lock();
        assert!(
            tokenize_stowrc_line(r#"before "unterminated"#)
                .unwrap()
                .is_empty()
        );
        assert!(
            tokenize_stowrc_line("before 'unterminated")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_stowrc_tokenization_supports_escaped_hash_character() {
        let _lock = process_env_lock();
        assert_eq!(
            tokenize_stowrc_line(r#"--ignore=a\#b"#).unwrap(),
            vec!["--ignore=a#b".to_string()]
        );
    }

    #[test]
    fn test_stowrc_tokenization_preserves_embedded_hash_character() {
        let _lock = process_env_lock();
        assert_eq!(
            tokenize_stowrc_line(r#"--ignore=.*#.*"#).unwrap(),
            vec!["--ignore=.*#.*".to_string()]
        );
    }

    #[test]
    fn test_expand_path_value_supports_braced_and_unbraced_env_vars() {
        let _lock = process_env_lock();
        let _home_guard = EnvVarGuard::new("HOME_STOW", "/home/example");
        let _number_guard = EnvVarGuard::new("1", "one");

        assert_eq!(
            expand_path_value(r"$HOME_STOW/.stowrc", ResourceValueOption::Dir).unwrap(),
            "/home/example/.stowrc".to_string()
        );
        assert_eq!(
            expand_path_value(r"$1/.stowrc", ResourceValueOption::Dir).unwrap(),
            "one/.stowrc".to_string()
        );
        assert_eq!(
            expand_path_value(
                r"${HOME_STOW}/nested/${HOME_STOW}",
                ResourceValueOption::Target
            )
            .unwrap(),
            "/home/example/nested//home/example".to_string()
        );
    }

    #[test]
    fn test_expand_path_value_leaves_non_gnu_braced_env_literals() {
        let _lock = process_env_lock();
        unsafe {
            std::env::remove_var("MISSING");
        }

        assert_eq!(
            expand_path_value(r"${MISSING:-/tmp}", ResourceValueOption::Dir).unwrap(),
            "${MISSING:-/tmp}".to_string()
        );
        assert_eq!(
            expand_path_value(r"${}/path", ResourceValueOption::Dir).unwrap(),
            "${}/path".to_string()
        );
        assert_eq!(
            expand_path_value(r"${MISSING/path", ResourceValueOption::Dir).unwrap(),
            "${MISSING/path".to_string()
        );
    }

    #[test]
    fn test_expand_path_value_preserves_escaped_markers() {
        let _lock = process_env_lock();
        let _home_guard = EnvVarGuard::new("HOME_STOW", "/home/example");

        assert_eq!(
            expand_path_value(r"\$HOME_STOW/keep-this", ResourceValueOption::Dir).unwrap(),
            "$HOME_STOW/keep-this".to_string()
        );
        assert_eq!(
            expand_path_value(r"\~/.keep-this", ResourceValueOption::Dir).unwrap(),
            "~/.keep-this".to_string()
        );
        assert_eq!(
            expand_path_value(r"\${HOME_STOW}/path", ResourceValueOption::Dir).unwrap(),
            "${HOME_STOW}/path".to_string()
        );
    }

    #[test]
    fn test_expand_path_value_display_tracks_tilde_expansion() {
        let _lock = process_env_lock();
        let _home_guard = EnvVarGuard::new("HOME", "/home/example");
        let _secret_guard = EnvVarGuard::new("RUSTOW_SECRET_FROM_ENV", "~/env-stow");

        let expanded =
            expand_path_value_with_display("~/tilde-stow", ResourceValueOption::Dir).unwrap();
        assert_eq!(expanded.value, "/home/example/tilde-stow");
        assert_eq!(expanded.display, Some("~/tilde-stow".to_string()));

        let expanded =
            expand_path_value_with_display("$RUSTOW_SECRET_FROM_ENV/pkg", ResourceValueOption::Dir)
                .unwrap();
        assert_eq!(expanded.value, "/home/example/env-stow/pkg");
        assert_eq!(
            expanded.display,
            Some("$RUSTOW_SECRET_FROM_ENV/pkg".to_string())
        );
    }

    #[test]
    fn test_stowrc_single_backslash_allows_env_and_tilde_expansion() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&home_dir).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "--dir=\\$HOME/d\n--target=\\~/t\n");

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(
            args.dir,
            Some(PathBuf::from(format!("{}/d", home_dir.to_string_lossy())))
        );
        assert_eq!(
            args.target,
            Some(PathBuf::from(format!("{}/t", home_dir.to_string_lossy())))
        );
    }

    #[test]
    fn test_stowrc_path_options_preserve_attached_hyphen_values() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            "--dir=-D\n--target=--verbose\n--ignore=from-rc\n",
        );

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(PathBuf::from("-D")));
        assert_eq!(args.target, Some(PathBuf::from("--verbose")));
        assert_eq!(args.ignore_patterns, vec!["from-rc"]);

        write_file(&cwd.join(".stowrc"), "-d-D\n-t--help\n");
        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(PathBuf::from("-D")));
        assert_eq!(args.target, Some(PathBuf::from("--help")));
    }

    #[test]
    fn test_stowrc_value_options_consume_separate_hyphen_values() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(
            &cwd.join(".stowrc"),
            "--dir\n-D\n--target\n--verbose\n--ignore\n--foo\n--defer\n--bar\n--override\n--baz\n",
        );

        let args = parse_runtime_args(["rustow", "pkg"]);
        assert_eq!(args.dir, Some(PathBuf::from("-D")));
        assert_eq!(args.target, Some(PathBuf::from("--verbose")));
        assert_eq!(args.ignore_patterns, vec!["--foo"]);
        assert_eq!(args.defer_conflicts, vec!["--bar"]);
        assert_eq!(args.override_conflicts, vec!["--baz"]);
    }

    #[cfg(unix)]
    #[test]
    fn test_expand_path_value_uses_system_user_lookup_for_tilde_user() {
        let _lock = process_env_lock();
        let expanded = expand_path_value("~root/.stowrc", ResourceValueOption::Dir).unwrap();
        let root_home = home_dir_for_user("root").expect("root user should exist");
        assert_eq!(expanded, format!("{}/.stowrc", root_home.to_string_lossy()));
    }

    #[test]
    fn test_stowrc_short_option_cluster_parsing_expands_attached_path_values() {
        let _lock = process_env_lock();
        let _guard = StowDirEnvGuard::new();
        let temp_dir = tempdir().unwrap();
        let home_dir = temp_dir.path().join("home");
        fs::create_dir_all(&home_dir).unwrap();
        let cwd = temp_dir.path().join("cwd");
        fs::create_dir_all(&cwd).unwrap();

        let _home_guard = EnvVarGuard::new("HOME", home_dir.to_str().unwrap());
        let _cwd_guard = CurrentDirGuard::set(&cwd);

        write_file(&cwd.join(".stowrc"), "-d$HOME/d\n-t${HOME}/t\n");

        let args = parse_runtime_args(["rustow", "pkg"]);
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
