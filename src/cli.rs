use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use clap::{Parser, Subcommand};

use crate::{
    error::{AppError, AppResult},
    model::{
        CapabilitiesRequest, DirsRequest, FileOperation, FileRequest, InfoRequest, OutputMode,
        PathMode, Request, ScanOptions,
    },
    pathutil,
    query::{CompoundQuery, Filter},
};

const SEARCH_AFTER_HELP: &str = r#"Additional filters:
  and size <MIB>
  and word <TEXT>
  and ext <EXT>
  and older <DAYS>
  and newer <DAYS>
  and empty

Common modifiers:
  PATH | md | json | jsonl | limit N | depth N | relative | absolute

Examples:
  dirrake word camera and ext jpg /repo limit 50 relative json
  dirrake size 100 and older 30 "D:\Videos" md"#;

const SCAN_AFTER_HELP: &str = r#"Common modifiers:
  PATH | md | json | jsonl | depth N | relative | absolute

This command defines its own result count and does not accept `limit`."#;

const ANALYZE_AFTER_HELP: &str = r#"Common modifiers:
  PATH | md | json | jsonl | limit N | depth N | relative | absolute

For `dirs`, `limit N` bounds returned directories. `dirs N` is shorthand.
For `info`, `limit N` bounds returned extension groups."#;

const AFTER_HELP: &str = r#"Examples:
  dirrake size 100
  dirrake size 100 "D:\Videos" md
  dirrake word camera /srv/projects json
  dirrake size 100 and ext mp4 and older 30 /data limit 50 relative json
  dirrake top 20 /data json
  dirrake dirs 20 /data
  dirrake info /data json
  dirrake capabilities json

Common modifiers (order-independent after the command query):
  PATH         directory to scan; omitted means the current directory
  md           write a timestamped Markdown report in the launch directory
  json         print schema-versioned JSON
  jsonl        print schema-versioned JSON Lines
  limit N      retain/report at most N result rows while still counting all matches
  depth N      scan at most N levels below the root (root is depth 0)
  relative     show result paths relative to the scan root
  absolute     show absolute/rooted result paths (default)

Search commands also accept `and` filters, for example:
  dirrake word camera and ext jpg and newer 7 json

DirRake is recursive, parallel, read-only, includes hidden/gitignored files, and does not follow symlinks/reparse points.
If a directory is literally named like a modifier (for example `json`), pass an explicit path such as `./json`."#;

#[derive(Debug, Parser)]
#[command(
    name = "dirrake",
    version,
    about = "Fast, read-only filesystem inspection for humans and agents",
    long_about = "DirRake recursively inspects directory trees in parallel using a compact query language. It is read-only, includes hidden and gitignored files, does not follow symlinks/reparse points, and offers stable JSON/JSONL output for agents.",
    after_help = AFTER_HELP,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List files strictly larger than the given size in MiB.
    #[command(after_help = SEARCH_AFTER_HELP)]
    Size {
        /// Size threshold in MiB.
        mib: u64,
        /// Optional filters, path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// List files whose filename contains the given text (case-insensitive).
    #[command(after_help = SEARCH_AFTER_HELP)]
    Word {
        /// Text to find in filenames.
        word: String,
        /// Optional filters, path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// List files with the given extension (case-insensitive).
    #[command(after_help = SEARCH_AFTER_HELP)]
    Ext {
        /// Extension with or without a leading dot.
        extension: String,
        /// Optional filters, path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// List files older than the given number of days.
    #[command(after_help = SEARCH_AFTER_HELP)]
    Older {
        /// Age threshold in days.
        days: u64,
        /// Optional filters, path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// List files modified within the given number of days.
    #[command(after_help = SEARCH_AFTER_HELP)]
    Newer {
        /// Recency window in days.
        days: u64,
        /// Optional filters, path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// List empty (zero-byte) files.
    #[command(after_help = SEARCH_AFTER_HELP)]
    Empty {
        /// Optional filters, path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// Show the N largest files.
    #[command(after_help = SCAN_AFTER_HELP)]
    Top {
        /// Number of files to return.
        count: usize,
        /// Optional path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// Show recursive directory sizes, largest first.
    #[command(after_help = ANALYZE_AFTER_HELP)]
    Dirs {
        /// Optional N shorthand, path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// Show a one-pass census of the directory tree.
    #[command(after_help = ANALYZE_AFTER_HELP)]
    Info {
        /// Optional path, output mode, and controls.
        #[arg(value_name = "ARGS", num_args = 0..)]
        tail: Vec<OsString>,
    },
    /// Describe DirRake's commands, outputs, guarantees, schema, and exit codes.
    Capabilities {
        /// Optional output mode: terminal, md, json, or jsonl.
        #[arg(value_name = "OUTPUT")]
        output: Option<OsString>,
    },
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }

    pub fn into_request(self, cwd: &Path) -> AppResult<Request> {
        let cwd = validate_working_directory(cwd.to_path_buf())?;
        match self.command {
            Command::Size { mib, tail } => file_search(
                Filter::size_greater_than_mib(mib).map_err(AppError::usage)?,
                tail,
                &cwd,
            ),
            Command::Word { word, tail } => {
                file_search(Filter::word(word).map_err(AppError::usage)?, tail, &cwd)
            }
            Command::Ext { extension, tail } => file_search(
                Filter::extension(extension).map_err(AppError::usage)?,
                tail,
                &cwd,
            ),
            Command::Older { days, tail } => file_search(
                Filter::older_than_days(days).map_err(AppError::usage)?,
                tail,
                &cwd,
            ),
            Command::Newer { days, tail } => file_search(
                Filter::newer_than_days(days).map_err(AppError::usage)?,
                tail,
                &cwd,
            ),
            Command::Empty { tail } => file_search(Filter::Empty, tail, &cwd),
            Command::Top { count, tail } => {
                if count == 0 {
                    return Err(AppError::usage(anyhow!(
                        "top count must be greater than zero"
                    )));
                }
                let state = parse_scan_tail(tail, &cwd, None, false)?;
                Ok(Request::Files(FileRequest {
                    options: state.into_options(&cwd),
                    operation: FileOperation::Top { count },
                }))
            }
            Command::Dirs { mut tail } => {
                let shorthand_limit = take_dirs_limit_shorthand(&mut tail)?;
                let state = parse_scan_tail(tail, &cwd, shorthand_limit, true)?;
                Ok(Request::Dirs(DirsRequest {
                    options: state.into_options(&cwd),
                }))
            }
            Command::Info { tail } => {
                let state = parse_scan_tail(tail, &cwd, None, true)?;
                Ok(Request::Info(InfoRequest {
                    options: state.into_options(&cwd),
                }))
            }
            Command::Capabilities { output } => {
                let mode = match output {
                    None => OutputMode::Terminal,
                    Some(value) => parse_output(&value)?.ok_or_else(|| {
                        AppError::usage(anyhow!(
                            "capabilities accepts only terminal, md, json, or jsonl"
                        ))
                    })?,
                };
                Ok(Request::Capabilities(CapabilitiesRequest {
                    report_dir: cwd,
                    output: mode,
                }))
            }
        }
    }
}

fn file_search(primary: Filter, tail: Vec<OsString>, cwd: &Path) -> AppResult<Request> {
    let mut query = CompoundQuery::new(primary);
    let state = parse_search_tail(tail, cwd, &mut query)?;
    Ok(Request::Files(FileRequest {
        options: state.into_options(cwd),
        operation: FileOperation::Filter(query),
    }))
}

#[derive(Debug)]
struct TailState {
    root: Option<PathBuf>,
    output: OutputMode,
    output_set: bool,
    limit: Option<usize>,
    max_depth: Option<usize>,
    path_mode: PathMode,
    path_mode_set: bool,
}

impl TailState {
    fn new(initial_limit: Option<usize>) -> Self {
        Self {
            root: None,
            output: OutputMode::Terminal,
            output_set: false,
            limit: initial_limit,
            max_depth: None,
            path_mode: PathMode::Absolute,
            path_mode_set: false,
        }
    }

    fn into_options(self, cwd: &Path) -> ScanOptions {
        ScanOptions {
            root: self.root.unwrap_or_else(|| cwd.to_path_buf()),
            report_dir: cwd.to_path_buf(),
            output: self.output,
            limit: self.limit,
            max_depth: self.max_depth,
            path_mode: self.path_mode,
        }
    }
}

fn parse_search_tail(
    tail: Vec<OsString>,
    cwd: &Path,
    query: &mut CompoundQuery,
) -> AppResult<TailState> {
    let mut state = TailState::new(None);
    let mut index = 0;
    while index < tail.len() {
        if token_eq(&tail[index], "and") {
            let (filter, consumed) = parse_and_filter(&tail[index + 1..])?;
            query.push(filter);
            index += consumed + 1;
            continue;
        }
        if parse_control(&tail, &mut index, cwd, &mut state, true)? {
            continue;
        }
        assign_path(&tail[index], cwd, &mut state)?;
        index += 1;
    }
    Ok(state)
}

fn parse_scan_tail(
    tail: Vec<OsString>,
    cwd: &Path,
    initial_limit: Option<usize>,
    allow_limit: bool,
) -> AppResult<TailState> {
    let mut state = TailState::new(initial_limit);
    let mut index = 0;
    while index < tail.len() {
        if token_eq(&tail[index], "and") {
            return Err(AppError::usage(anyhow!(
                "`and` filters are supported only by size, word, ext, older, newer, and empty"
            )));
        }
        if parse_control(&tail, &mut index, cwd, &mut state, allow_limit)? {
            continue;
        }
        assign_path(&tail[index], cwd, &mut state)?;
        index += 1;
    }
    Ok(state)
}

fn parse_control(
    tail: &[OsString],
    index: &mut usize,
    _cwd: &Path,
    state: &mut TailState,
    allow_limit: bool,
) -> AppResult<bool> {
    let token = &tail[*index];

    if let Some(output) = parse_output(token)? {
        if state.output_set {
            return Err(AppError::usage(anyhow!(
                "output mode specified more than once"
            )));
        }
        state.output = output;
        state.output_set = true;
        *index += 1;
        return Ok(true);
    }

    if token_eq(token, "limit") {
        if !allow_limit {
            return Err(AppError::usage(anyhow!(
                "this command defines its result count directly and does not accept `limit`"
            )));
        }
        if state.limit.is_some() {
            return Err(AppError::usage(anyhow!("limit specified more than once")));
        }
        let value = tail
            .get(*index + 1)
            .ok_or_else(|| AppError::usage(anyhow!("`limit` requires a positive integer")))?;
        state.limit = Some(parse_positive_usize(value, "limit")?);
        *index += 2;
        return Ok(true);
    }

    if token_eq(token, "depth") {
        if state.max_depth.is_some() {
            return Err(AppError::usage(anyhow!("depth specified more than once")));
        }
        let value = tail
            .get(*index + 1)
            .ok_or_else(|| AppError::usage(anyhow!("`depth` requires a non-negative integer")))?;
        state.max_depth = Some(parse_usize(value, "depth")?);
        *index += 2;
        return Ok(true);
    }

    let path_mode = if token_eq(token, "relative") {
        Some(PathMode::Relative)
    } else if token_eq(token, "absolute") {
        Some(PathMode::Absolute)
    } else {
        None
    };
    if let Some(path_mode) = path_mode {
        if state.path_mode_set {
            return Err(AppError::usage(anyhow!(
                "path mode specified more than once"
            )));
        }
        state.path_mode = path_mode;
        state.path_mode_set = true;
        *index += 1;
        return Ok(true);
    }

    Ok(false)
}

fn parse_and_filter(tokens: &[OsString]) -> AppResult<(Filter, usize)> {
    let kind = tokens
        .first()
        .ok_or_else(|| AppError::usage(anyhow!("`and` must be followed by a filter")))?;

    if token_eq(kind, "empty") {
        return Ok((Filter::Empty, 1));
    }

    let value = tokens.get(1).ok_or_else(|| {
        AppError::usage(anyhow!(
            "filter `{}` requires a value",
            pathutil::terminal_os_str(kind)
        ))
    })?;

    let filter = if token_eq(kind, "size") {
        Filter::size_greater_than_mib(parse_u64(value, "size")?).map_err(AppError::usage)?
    } else if token_eq(kind, "word") {
        Filter::word(required_utf8(value, "word")?.to_owned()).map_err(AppError::usage)?
    } else if token_eq(kind, "ext") {
        Filter::extension(required_utf8(value, "extension")?.to_owned()).map_err(AppError::usage)?
    } else if token_eq(kind, "older") {
        Filter::older_than_days(parse_u64(value, "older")?).map_err(AppError::usage)?
    } else if token_eq(kind, "newer") {
        Filter::newer_than_days(parse_u64(value, "newer")?).map_err(AppError::usage)?
    } else {
        return Err(AppError::usage(anyhow!(
            "unknown `and` filter `{}`; expected size, word, ext, older, newer, or empty",
            pathutil::terminal_os_str(kind)
        )));
    };

    Ok((filter, 2))
}

fn take_dirs_limit_shorthand(tail: &mut Vec<OsString>) -> AppResult<Option<usize>> {
    let Some(first) = tail.first() else {
        return Ok(None);
    };
    let Some(text) = first.to_str() else {
        return Ok(None);
    };
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let limit = parse_positive_usize(first, "directory count")?;
    tail.remove(0);
    Ok(Some(limit))
}

fn assign_path(value: &OsString, cwd: &Path, state: &mut TailState) -> AppResult<()> {
    if state.root.is_some() {
        return Err(AppError::usage(anyhow!(
            "unexpected argument `{}`; only one scan PATH is allowed",
            pathutil::terminal_os_str(value)
        )));
    }
    state.root = Some(resolve_directory(value, cwd)?);
    Ok(())
}

fn parse_output(value: &OsStr) -> AppResult<Option<OutputMode>> {
    let Some(text) = value.to_str() else {
        return Ok(None);
    };
    Ok(OutputMode::from_token(text))
}

fn token_eq(value: &OsStr, expected: &str) -> bool {
    value
        .to_str()
        .is_some_and(|text| text.eq_ignore_ascii_case(expected))
}

fn required_utf8<'a>(value: &'a OsStr, label: &str) -> AppResult<&'a str> {
    value
        .to_str()
        .ok_or_else(|| AppError::usage(anyhow!("{label} must be valid UTF-8 text")))
}

fn parse_u64(value: &OsStr, label: &str) -> AppResult<u64> {
    required_utf8(value, label)?
        .parse::<u64>()
        .map_err(|_| AppError::usage(anyhow!("{label} must be a non-negative integer")))
}

fn parse_usize(value: &OsStr, label: &str) -> AppResult<usize> {
    required_utf8(value, label)?
        .parse::<usize>()
        .map_err(|_| AppError::usage(anyhow!("{label} must be a non-negative integer")))
}

fn parse_positive_usize(value: &OsStr, label: &str) -> AppResult<usize> {
    let parsed = parse_usize(value, label)?;
    if parsed == 0 {
        return Err(AppError::usage(anyhow!(
            "{label} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn resolve_directory(value: &OsString, cwd: &Path) -> AppResult<PathBuf> {
    let path = PathBuf::from(value);
    let rooted = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    validate_scan_root(rooted)
}

fn validate_working_directory(path: PathBuf) -> AppResult<PathBuf> {
    let metadata = fs::metadata(&path).map_err(|error| {
        AppError::root(anyhow!(
            "failed to access current working directory `{}`: {error}",
            pathutil::terminal_path(&path)
        ))
    })?;
    if !metadata.is_dir() {
        return Err(AppError::root(anyhow!(
            "current working directory `{}` is not a directory",
            pathutil::terminal_path(&path)
        )));
    }
    Ok(path)
}

fn validate_scan_root(path: PathBuf) -> AppResult<PathBuf> {
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        AppError::root(anyhow!(
            "failed to access scan root `{}`: {error}",
            pathutil::terminal_path(&path)
        ))
    })?;
    if metadata_is_link_like(&metadata) {
        return Err(AppError::root(anyhow!(
            "scan root `{}` is a symbolic link or reparse point; DirRake does not follow link targets",
            pathutil::terminal_path(&path)
        )));
    }
    if !metadata.is_dir() {
        return Err(AppError::root(anyhow!(
            "scan root `{}` is not a directory",
            pathutil::terminal_path(&path)
        )));
    }
    Ok(path)
}

#[cfg(windows)]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn original_size_syntax_still_defaults_to_cwd_and_terminal() {
        let cwd = tempdir().unwrap();
        let cli = Cli::try_parse_from(["dirrake", "size", "100"]).unwrap();
        let Request::Files(request) = cli.into_request(cwd.path()).unwrap() else {
            panic!("expected file request");
        };
        assert_eq!(request.options.root, cwd.path());
        assert_eq!(request.options.output, OutputMode::Terminal);
    }

    #[test]
    fn compound_filters_and_agent_controls_parse_together() {
        let cwd = tempdir().unwrap();
        let target = cwd.path().join("target");
        fs::create_dir(&target).unwrap();
        let target_text = target.to_string_lossy().into_owned();
        let cli = Cli::try_parse_from([
            "dirrake",
            "word",
            "camera",
            "and",
            "ext",
            "jpg",
            "and",
            "size",
            "1",
            &target_text,
            "limit",
            "25",
            "depth",
            "3",
            "relative",
            "json",
        ])
        .unwrap();
        let Request::Files(request) = cli.into_request(cwd.path()).unwrap() else {
            panic!("expected file request");
        };
        let FileOperation::Filter(query) = request.operation else {
            panic!("expected filter operation");
        };
        assert_eq!(query.filters().len(), 3);
        assert_eq!(request.options.limit, Some(25));
        assert_eq!(request.options.max_depth, Some(3));
        assert_eq!(request.options.path_mode, PathMode::Relative);
        assert_eq!(request.options.output, OutputMode::Json);
        assert_eq!(request.options.root, target);
    }

    #[test]
    fn dirs_supports_count_shorthand() {
        let cwd = tempdir().unwrap();
        let cli = Cli::try_parse_from(["dirrake", "dirs", "20", "json"]).unwrap();
        let Request::Dirs(request) = cli.into_request(cwd.path()).unwrap() else {
            panic!("expected dirs request");
        };
        assert_eq!(request.options.limit, Some(20));
        assert_eq!(request.options.output, OutputMode::Json);
    }

    #[test]
    fn top_rejects_limit_modifier() {
        let cwd = tempdir().unwrap();
        let cli = Cli::try_parse_from(["dirrake", "top", "10", "limit", "2"]).unwrap();
        let error = cli.into_request(cwd.path()).unwrap_err();
        assert_eq!(error.code(), 2);
        assert!(error.to_string().contains("does not accept `limit`"));
    }

    #[test]
    fn explicit_modifier_named_directory_can_use_dot_prefix() {
        let cwd = tempdir().unwrap();
        fs::create_dir(cwd.path().join("json")).unwrap();
        let cli = Cli::try_parse_from(["dirrake", "word", "camera", "./json"]).unwrap();
        let Request::Files(request) = cli.into_request(cwd.path()).unwrap() else {
            panic!("expected file request");
        };
        assert_eq!(request.options.root, cwd.path().join("./json"));
    }

    #[test]
    fn invalid_root_is_exit_code_three() {
        let cwd = tempdir().unwrap();
        let file = cwd.path().join("not-a-dir.txt");
        fs::write(&file, "x").unwrap();
        let file_text = file.to_string_lossy().into_owned();
        let cli = Cli::try_parse_from(["dirrake", "word", "camera", &file_text]).unwrap();
        let error = cli.into_request(cwd.path()).unwrap_err();
        assert_eq!(error.code(), 3);
    }
}
