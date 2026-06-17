use clap::{Parser, builder::TypedValueParser};
use std::ffi::OsString;
#[cfg(unix)]
use std::ffi::{CStr, CString};
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::Arc;
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

    let verbose = parse_verbose_level(argv)?;
    let mut args = <Args as Parser>::try_parse_from(normalize_verbose_args(argv))?;
    args.verbose = verbose;
    Ok(args)
}

fn validate_cli_args_before_resource_files(argv: &[OsString]) -> Result<(), clap::Error> {
    validate_cli_option_tokens(argv)?;
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
            if !is_known_long_option(key) {
                return Err(unknown_option_error(&arg));
            }
            if map_long_value_option(key).is_some() {
                expecting_option_value = !has_attached_value;
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            let mut flags = arg[1..].chars().peekable();
            while let Some(flag) = flags.next() {
                if !is_known_short_option(flag) {
                    return Err(unknown_option_error(&format!("-{flag}")));
                }
                if matches!(flag, 't' | 'd') {
                    expecting_option_value = flags.peek().is_none();
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

fn is_known_long_option(option: &str) -> bool {
    map_long_value_option(option).is_some() || map_long_bool_option(option)
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

    let mut stowrc_args = Vec::new();
    if let Some(home_dir) = env::var_os("HOME") {
        let home_path = home_stowrc_path(home_dir);
        let home_entries = read_stowrc_file(&home_path)?;
        stowrc_args.extend(home_entries);
    }

    if let Ok(current_dir) = env::current_dir() {
        let local_path = current_dir.join(".stowrc");
        let local_entries = read_stowrc_file(&local_path)?;
        stowrc_args.extend(local_entries);
    }

    let merged_resource_args = normalize_stowrc_tokens(stowrc_args)?;

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
    value: String,
    origin: StowrcTokenOrigin,
}

#[derive(Debug, Clone)]
struct StowrcTokenOrigin {
    path: Arc<PathBuf>,
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

fn read_stowrc_file(path: &Path) -> Result<Vec<StowrcToken>, clap::Error> {
    let file = match open_stowrc_file(path)? {
        Some(file) => file,
        None => return Ok(Vec::new()),
    };

    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::PermissionDenied => return Ok(Vec::new()),
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::Io,
                format!(
                    "failed to inspect resource file '{}': {}",
                    path.display(),
                    err
                ),
            ));
        },
    };

    if !metadata.is_file() {
        return Ok(Vec::new());
    }

    stowrc_tokens_from_reader(path, file)
}

#[cfg(unix)]
fn open_stowrc_file(path: &Path) -> Result<Option<fs::File>, clap::Error> {
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
            format!("failed to open resource file '{}': {}", path.display(), err),
        )),
    }
}

#[cfg(not(unix))]
fn open_stowrc_file(path: &Path) -> Result<Option<fs::File>, clap::Error> {
    match fs::File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) if err.kind() == ErrorKind::PermissionDenied => Ok(None),
        Err(err) => Err(clap::Error::raw(
            clap::error::ErrorKind::Io,
            format!("failed to open resource file '{}': {}", path.display(), err),
        )),
    }
}

fn stowrc_tokens_from_reader(path: &Path, file: fs::File) -> Result<Vec<StowrcToken>, clap::Error> {
    let mut tokens = Vec::new();
    let path = Arc::new(path.to_path_buf());
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) if err.kind() == ErrorKind::PermissionDenied => return Ok(Vec::new()),
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::Io,
                    format!("failed to read resource file '{}': {}", path.display(), err),
                ));
            },
        };
        let origin = StowrcTokenOrigin {
            path: path.clone(),
            line: index + 1,
        };
        let line_tokens = tokenize_stowrc_line(&line).map_err(|err| {
            clap::Error::raw(
                clap::error::ErrorKind::InvalidValue,
                format!("{} in '{}:{}'", err, path.display(), index + 1),
            )
        })?;
        tokens.extend(line_tokens.into_iter().map(|value| StowrcToken {
            value,
            origin: origin.clone(),
        }));
    }

    Ok(tokens)
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
}

#[derive(Debug)]
struct NormalizedStowrcArgs {
    argv: Vec<OsString>,
    path_displays: Vec<ResourcePathDisplay>,
}

fn normalize_stowrc_tokens(
    raw_tokens: Vec<StowrcToken>,
) -> Result<NormalizedStowrcArgs, clap::Error> {
    let mut normalized = Vec::new();
    let mut expecting_value: Option<PendingResourceValue> = None;
    let mut path_values = Vec::new();
    let mut after_double_dash = false;

    for token in raw_tokens {
        let token_value = token.value;

        if let Some(pending) = expecting_value {
            let normalized_value = format!("{}{}", pending.value_prefix, token_value);
            let value_start = pending.value_prefix.len();
            if pending.option_name.is_path_option() {
                path_values.push(ResourcePathValue {
                    index: normalized.len(),
                    option_name: pending.option_name,
                    value_start,
                });
            }
            normalized.push(OsString::from(normalized_value));
            expecting_value = None;
            continue;
        }

        if after_double_dash {
            continue;
        }

        if token_value == "--" {
            // Packages are ignored in resource files.
            after_double_dash = true;
            continue;
        }

        if !token_value.starts_with('-') {
            // Treated as package names in resource files.
            continue;
        }

        if token_value.starts_with("--") {
            if let Some(parsed) = parse_long_stowrc_option(&token_value) {
                let base_index = normalized.len();
                normalized.extend(parsed.tokens);
                if let Some(path_value) = parsed.path_value {
                    path_values.push(ResourcePathValue {
                        index: base_index + path_value.index,
                        option_name: path_value.option_name,
                        value_start: path_value.value_start,
                    });
                }
                if let Some(option_name) = parsed.expecting_value {
                    expecting_value = Some(PendingResourceValue {
                        option_name: option_name.option_name,
                        origin: token.origin,
                        value_prefix: option_name.value_prefix,
                    });
                }
            }

            continue;
        }

        if token_value.len() > 1 {
            if let Some(parsed) = parse_short_stowrc_option(&token_value) {
                let base_index = normalized.len();
                normalized.extend(parsed.tokens);
                if let Some(path_value) = parsed.path_value {
                    path_values.push(ResourcePathValue {
                        index: base_index + path_value.index,
                        option_name: path_value.option_name,
                        value_start: path_value.value_start,
                    });
                }
                expecting_value = parsed
                    .expecting_value
                    .map(|option_name| PendingResourceValue {
                        option_name: option_name.option_name,
                        origin: token.origin,
                        value_prefix: option_name.value_prefix,
                    });
            }
        }
    }

    if let Some(pending) = expecting_value {
        return Err(stowrc_missing_value_error(&pending));
    }

    let path_displays = expand_final_resource_path_values(&mut normalized, &path_values)?;

    Ok(NormalizedStowrcArgs {
        argv: normalized,
        path_displays,
    })
}

fn parse_long_stowrc_option(token: &str) -> Option<ParsedResourceOption> {
    let token = token.trim();
    let long_token = token.trim_start_matches("--");
    if long_token.is_empty() {
        return None;
    }

    if long_token == "stow" || long_token == "delete" || long_token == "restow" {
        return None;
    }

    if let Some((key, _value)) = long_token.split_once('=') {
        if let Some(option_name) = map_long_value_option(key) {
            if option_name.is_path_option() {
                return Some(ParsedResourceOption {
                    tokens: vec![OsString::from(token)],
                    expecting_value: None,
                    path_value: Some(ResourcePathValue {
                        index: 0,
                        option_name,
                        value_start: key.len() + 3,
                    }),
                });
            }
            return Some(ParsedResourceOption {
                tokens: vec![OsString::from(token)],
                expecting_value: None,
                path_value: None,
            });
        }

        return Some(ParsedResourceOption {
            tokens: vec![OsString::from(token)],
            expecting_value: None,
            path_value: None,
        });
    }

    if let Some(option_name) = map_long_value_option(long_token) {
        return Some(ParsedResourceOption {
            tokens: Vec::new(),
            expecting_value: Some(ResourceValueExpectation {
                option_name,
                value_prefix: format!("--{}=", long_token),
            }),
            path_value: None,
        });
    }

    if map_long_bool_option(long_token) {
        return Some(ParsedResourceOption {
            tokens: vec![OsString::from(token)],
            expecting_value: None,
            path_value: None,
        });
    }

    Some(ParsedResourceOption {
        tokens: vec![OsString::from(token)],
        expecting_value: None,
        path_value: None,
    })
}

fn map_long_bool_option(token: &str) -> bool {
    matches!(
        token,
        "stow"
            | "delete"
            | "restow"
            | "compat"
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

fn parse_short_stowrc_option(token: &str) -> Option<ParsedResourceOption> {
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
                        let value_prefix = format!("-{}", chars[i]);
                        return Some(ParsedResourceOption {
                            tokens: Vec::new(),
                            expecting_value: Some(ResourceValueExpectation {
                                option_name,
                                value_prefix,
                            }),
                            path_value: None,
                        });
                    }
                    let value_prefix =
                        format!("-{}{}", bool_flags.iter().collect::<String>(), chars[i]);
                    return Some(ParsedResourceOption {
                        tokens: Vec::new(),
                        expecting_value: Some(ResourceValueExpectation {
                            option_name,
                            value_prefix,
                        }),
                        path_value: None,
                    });
                }

                let prefix = if bool_flags.is_empty() {
                    format!("-{}", chars[i])
                } else {
                    format!("-{}{}", bool_flags.iter().collect::<String>(), chars[i])
                };
                return Some(ParsedResourceOption {
                    tokens: vec![OsString::from(format!("{}{}", prefix, token_remainder))],
                    expecting_value: None,
                    path_value: Some(ResourcePathValue {
                        index: 0,
                        option_name,
                        value_start: prefix.len(),
                    }),
                });
            },
            'p' | 'v' | 'h' | 'V' | 'n' => {
                bool_flags.push(chars[i]);
                i += 1;
            },
            other if other.is_ascii_alphabetic() => {
                return Some(ParsedResourceOption {
                    tokens: vec![OsString::from(token)],
                    expecting_value: None,
                    path_value: None,
                });
            },
            _ => {
                return Some(ParsedResourceOption {
                    tokens: vec![OsString::from(token)],
                    expecting_value: None,
                    path_value: None,
                });
            },
        }
    }

    if bool_flags.is_empty() {
        None
    } else {
        Some(ParsedResourceOption {
            tokens: vec![format!("-{}", bool_flags.iter().collect::<String>()).into()],
            expecting_value: None,
            path_value: None,
        })
    }
}

fn tokenize_stowrc_line(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
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
            return Ok(Vec::new());
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
                tokens.push(token.clone());
                token.clear();
                token_started = false;
            }
            continue;
        }

        token.push(ch);
        token_started = true;
    }

    if in_single_quote || in_double_quote {
        return Ok(Vec::new());
    }

    if token_started {
        tokens.push(token);
    }

    Ok(tokens)
}

#[cfg(test)]
fn expand_path_value(
    raw_value: &str,
    option_name: ResourceValueOption,
) -> Result<String, clap::Error> {
    let expanded = expand_path_value_with_display(raw_value, option_name)?;
    Ok(expanded.value)
}

struct ExpandedPathValue {
    value: String,
    display: Option<String>,
}

fn expand_path_value_with_display(
    raw_value: &str,
    option_name: ResourceValueOption,
) -> Result<ExpandedPathValue, clap::Error> {
    let env_expanded = expand_environment_value(raw_value, option_name)?;
    let tilde_expanded = expand_tilde_value_with_display(&env_expanded.value);
    let display = env_expanded
        .display
        .map(|display| unescape_tilde_markers(&display))
        .or(tilde_expanded.display);

    Ok(ExpandedPathValue {
        value: tilde_expanded.value,
        display,
    })
}

struct ExpandedEnvironmentValue {
    value: String,
    display: Option<String>,
}

struct ExpandedTildeValue {
    value: String,
    display: Option<String>,
}

fn expand_environment_value(
    raw_value: &str,
    option_name: ResourceValueOption,
) -> Result<ExpandedEnvironmentValue, clap::Error> {
    let mut output = String::new();
    let mut display = String::new();
    let mut changed_display = false;
    let mut chars = raw_value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                if next == '$' {
                    output.push(next);
                    display.push(next);
                } else {
                    output.push('\\');
                    output.push(next);
                    display.push('\\');
                    display.push(next);
                }
            } else {
                output.push('\\');
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
                    output.push_str("${");
                    output.push_str(&variable);
                    display.push_str("${");
                    display.push_str(&variable);
                    if closed {
                        output.push('}');
                        display.push('}');
                    }
                    continue;
                }
                let value = env_resource_value(&variable, option_name)?;
                output.push_str(&value);
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
                    output.push_str(&value);
                    display.push_str(&format!("${}", variable));
                    changed_display = true;
                    continue;
                }
            }

            output.push('$');
            display.push('$');
            continue;
        }

        output.push(ch);
        display.push(ch);
    }

    Ok(ExpandedEnvironmentValue {
        value: output,
        display: changed_display.then_some(display),
    })
}

fn env_resource_value(
    variable: &str,
    option_name: ResourceValueOption,
) -> Result<String, clap::Error> {
    // GNU Stow 2.4.1 aborts here via _safe_expand_env_var despite a nearby
    // source comment mentioning Perl's empty-string fallback.
    env::var(variable).map_err(|_| undefined_env_var_error(variable, option_name))
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

fn expand_tilde_value_with_display(value: &str) -> ExpandedTildeValue {
    let tilde_expanded = if let Some(rest) = value.strip_prefix('~') {
        let user_end = rest.find('/').unwrap_or(rest.len());
        let user = &rest[..user_end];
        let suffix = &rest[user_end..];
        let mut output = String::new();

        if user.is_empty() {
            if let Some(home_dir) = current_user_home_dir() {
                output.push_str(&home_dir.to_string_lossy());
            } else {
                output.push('~');
            }
        } else if let Some(user_home) = home_dir_for_user(user) {
            output.push_str(&user_home.to_string_lossy());
        } else {
            // GNU Stow's Perl tilde expansion substitutes undef here, which
            // effectively removes the unknown user part of the path.
        }

        output.push_str(suffix);

        output
    } else {
        value.to_string()
    };

    let tilde_expanded = unescape_tilde_markers(&tilde_expanded);
    let display = value
        .starts_with('~')
        .then(|| unescape_tilde_markers(value))
        .filter(|display| display != &tilde_expanded);

    ExpandedTildeValue {
        value: tilde_expanded,
        display,
    }
}

fn unescape_tilde_markers(value: &str) -> String {
    value.replace("\\~", "~")
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
            let raw_value = normalized[path_value.index].to_string_lossy();
            let (expanded_token, redacted_path_value, redacted_display) = if path_value.value_start
                == 0
            {
                let expanded = expand_path_value_with_display(&raw_value, path_value.option_name)?;
                (expanded.value.clone(), expanded.value, expanded.display)
            } else {
                let (prefix, raw_value) = raw_value.split_at(path_value.value_start);
                let expanded = expand_path_value_with_display(raw_value, path_value.option_name)?;
                (
                    format!("{}{}", prefix, expanded.value),
                    expanded.value,
                    expanded.display,
                )
            };
            if let Some(display) = redacted_display {
                path_displays.push(ResourcePathDisplay {
                    option_name: path_value.option_name,
                    path: PathBuf::from(redacted_path_value),
                    display,
                });
            }
            normalized[path_value.index] = expanded_token.into();
        }
    }

    Ok(path_displays)
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
            pending.origin.path.display(),
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
            if key == "help" {
                if has_attached_value {
                    return None;
                }
                saw_help = true;
                continue;
            }
            if key == "version" {
                if has_attached_value {
                    return None;
                }
                saw_version = true;
                continue;
            }
            if map_long_value_option(key).is_some() {
                expecting_option_value = !has_attached_value;
                continue;
            }
            if key == "verbose" || map_long_bool_option(key) {
                if has_attached_value && key != "verbose" {
                    return None;
                }
                continue;
            }
            return None;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            let mut flags = arg[1..].chars().peekable();
            while let Some(flag) = flags.next() {
                if !is_known_short_option(flag) {
                    return None;
                }

                if flag == 'h' {
                    saw_help = true;
                } else if flag == 'V' {
                    saw_version = true;
                }

                if matches!(flag, 't' | 'd') {
                    expecting_option_value = flags.peek().is_none();
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
    matches!(
        flag,
        'h' | 'V' | 'S' | 'D' | 'R' | 'p' | 'n' | 'v' | 't' | 'd'
    )
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
    use std::collections::BTreeSet;
    use std::fs::{self, File};
    use std::io::Write;
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
        let mut short_options = BTreeSet::new();

        for arg in command.get_arguments() {
            if let Some(options) = arg.get_long_and_visible_aliases() {
                long_options.extend(options.into_iter().map(str::to_string));
            }
            if let Some(options) = arg.get_aliases() {
                long_options.extend(options.into_iter().map(str::to_string));
            }
            if let Some(options) = arg.get_short_and_visible_aliases() {
                short_options.extend(options);
            }
        }
        long_options.extend(["help".to_string(), "version".to_string()]);
        short_options.extend(['h', 'V']);

        for option in long_options {
            assert!(
                is_known_long_option(&option),
                "compat parser must know --{}",
                option
            );
        }
        for option in short_options {
            assert!(
                is_known_short_option(option),
                "compat parser must know -{}",
                option
            );
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
        assert!(Args::try_parse_from(["rustow", "--verbose=invalid", "mypackage"]).is_err());
        assert!(Args::try_parse_from(["rustow", "--verbose=6", "mypackage"]).is_err());
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
    fn test_verbose_numeric_out_of_range_reports_range() {
        let _lock = process_env_lock();
        let error = Args::try_parse_from(["rustow", "--verbose=6", "mypackage"]).unwrap_err();

        assert!(error.to_string().contains("between 0 and 5"));
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
    fn test_help_takes_precedence_over_invalid_verbose() {
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
    fn test_try_parse_from_with_operation_groups_returns_parse_errors() {
        let _lock = process_env_lock();
        let error =
            Args::try_parse_from_with_operation_groups(["rustow", "--verbose=6", "mypackage"])
                .unwrap_err();

        assert!(error.to_string().contains("between 0 and 5"));
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
        assert!(error.to_string().contains(".stowrc:1"));
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
        assert!(error.to_string().contains("--bad-option"));
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
        assert!(error.to_string().contains("--bad-option"));
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
