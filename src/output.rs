use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use chrono::{Local, SecondsFormat};
use serde_json::Value;

use crate::{
    agent_json::{self, AgentReport},
    analyze::{DirectoryReport, InfoReport},
    capabilities::CapabilitiesReport,
    error::{AppError, AppResult},
    model::{OutputMode, PathMode},
    pathutil,
    scan::FileReport,
};

pub enum Report {
    Files(FileReport),
    Directories(DirectoryReport),
    Info(InfoReport),
    Capabilities(CapabilitiesReport),
}

pub fn emit(report: &Report, mode: OutputMode, report_dir: &Path) -> AppResult<()> {
    match mode {
        OutputMode::Terminal => {
            let stdout = io::stdout();
            let mut writer = stdout.lock();
            write_terminal_output(report, &mut writer)?;
            print_terminal_warnings(report);
            Ok(())
        }
        OutputMode::Markdown => {
            let path = write_markdown(report, report_dir)?;
            let stdout = io::stdout();
            let mut writer = stdout.lock();
            write_markdown_confirmation(&path, &mut writer)
        }
        OutputMode::Json => {
            let stdout = io::stdout();
            let mut writer = stdout.lock();
            write_json_output(report, &mut writer)
        }
        OutputMode::Jsonl => {
            let stdout = io::stdout();
            let mut writer = stdout.lock();
            write_jsonl(report, &mut writer)
        }
    }
}

fn write_terminal_output<W: Write>(report: &Report, writer: &mut W) -> AppResult<()> {
    match write_terminal(report, writer) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(AppError::output(anyhow!(
            "failed to write terminal output: {error}"
        ))),
    }
}

fn write_markdown_confirmation<W: Write>(path: &Path, writer: &mut W) -> AppResult<()> {
    match writeln!(writer, "Wrote {}", pathutil::terminal_path(path)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(AppError::output(anyhow!(
            "failed to write Markdown confirmation: {error}"
        ))),
    }
}

fn write_json_output<W: Write>(report: &Report, writer: &mut W) -> AppResult<()> {
    match serde_json::to_writer_pretty(&mut *writer, &AgentReport(report)) {
        Ok(()) => {}
        Err(error) if error.io_error_kind() == Some(io::ErrorKind::BrokenPipe) => return Ok(()),
        Err(error) => {
            return Err(AppError::output(anyhow!(
                "failed to serialize JSON output: {error}"
            )));
        }
    }

    match writeln!(writer) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(AppError::output(anyhow!(
            "failed to write JSON output: {error}"
        ))),
    }
}

pub fn json_value(report: &Report) -> Value {
    agent_json::value(report)
}

fn write_terminal<W: Write>(report: &Report, writer: &mut W) -> io::Result<()> {
    match report {
        Report::Files(report) => write_file_terminal(report, writer),
        Report::Directories(report) => write_directory_terminal(report, writer),
        Report::Info(report) => write_info_terminal(report, writer),
        Report::Capabilities(report) => write_capabilities_terminal(report, writer),
    }
}

fn write_file_terminal<W: Write>(report: &FileReport, writer: &mut W) -> io::Result<()> {
    for entry in &report.matches {
        writeln!(
            writer,
            "{:>10}  {}",
            human_size(u128::from(entry.size_bytes)),
            terminal_path(&entry.path, &report.root, report.path_mode)
        )?;
    }

    if report.matches.is_empty() {
        writeln!(writer, "No files matched.")?;
    }

    writeln!(writer)?;
    if report.stats.truncated {
        writeln!(
            writer,
            "{} of {} matches returned | {} returned | {} matched total | truncated",
            report.stats.matches_returned,
            report.stats.matches_total,
            human_size(report.stats.returned_bytes),
            human_size(report.stats.matched_bytes_total)
        )?;
    } else {
        writeln!(
            writer,
            "{} {} | {} total",
            report.stats.matches_total,
            if report.stats.matches_total == 1 {
                "file"
            } else {
                "files"
            },
            human_size(report.stats.matched_bytes_total)
        )?;
    }
    writeln!(
        writer,
        "Scanned {} files, {} directories in {} ms{}",
        report.stats.files_seen,
        report.stats.directories_seen,
        report.stats.elapsed_ms,
        depth_suffix(report.max_depth)
    )
}

fn write_directory_terminal<W: Write>(report: &DirectoryReport, writer: &mut W) -> io::Result<()> {
    for entry in &report.directories {
        writeln!(
            writer,
            "{:>10}  {:>8} files  {}",
            human_size(entry.size_bytes),
            entry.file_count,
            terminal_path(&entry.path, &report.root, report.path_mode)
        )?;
    }
    if report.directories.is_empty() {
        writeln!(writer, "No subdirectories found.")?;
    }
    writeln!(writer)?;
    if report.stats.truncated {
        writeln!(
            writer,
            "{} of {} directories returned | truncated",
            report.stats.directories_returned, report.stats.directories_total
        )?;
    } else {
        writeln!(writer, "{} directories", report.stats.directories_total)?;
    }
    writeln!(
        writer,
        "Scanned {} files, {} directories | {} total file data | {} ms{}",
        report.stats.files_seen,
        report.stats.directories_seen,
        human_size(report.stats.total_file_bytes),
        report.stats.elapsed_ms,
        depth_suffix(report.max_depth)
    )
}

fn write_info_terminal<W: Write>(report: &InfoReport, writer: &mut W) -> io::Result<()> {
    writeln!(writer, "Root: {}", pathutil::terminal_path(&report.root))?;
    writeln!(writer, "Files seen: {}", report.stats.files_seen)?;
    writeln!(
        writer,
        "Directories seen: {}",
        report.stats.directories_seen
    )?;
    writeln!(
        writer,
        "Total size: {}",
        human_size(report.stats.total_file_bytes)
    )?;
    writeln!(
        writer,
        "Elapsed: {} ms{}",
        report.stats.elapsed_ms,
        depth_suffix(report.max_depth)
    )?;
    writeln!(writer)?;

    match &report.largest_file {
        Some(file) => writeln!(
            writer,
            "Largest file: {}  {}",
            human_size(u128::from(file.size_bytes)),
            terminal_path(&file.path, &report.root, report.path_mode)
        )?,
        None => writeln!(writer, "Largest file: none")?,
    }
    match &report.largest_directory {
        Some(directory) => writeln!(
            writer,
            "Largest directory: {}  {}",
            human_size(directory.size_bytes),
            terminal_path(&directory.path, &report.root, report.path_mode)
        )?,
        None => writeln!(writer, "Largest directory: none")?,
    }

    writeln!(writer)?;
    writeln!(writer, "Extensions:")?;
    if report.extensions.is_empty() {
        writeln!(writer, "  none")?;
    } else {
        for extension in &report.extensions {
            writeln!(
                writer,
                "  {:>10}  {:>8} files  {}",
                human_size(extension.size_bytes),
                extension.file_count,
                display_extension(&extension.extension)
            )?;
        }
        if report.stats.extensions_truncated {
            writeln!(
                writer,
                "  ... {} of {} extension groups shown",
                report.stats.extensions_returned, report.stats.extensions_total
            )?;
        }
    }
    Ok(())
}

fn write_capabilities_terminal<W: Write>(
    report: &CapabilitiesReport,
    writer: &mut W,
) -> io::Result<()> {
    writeln!(writer, "DirRake {}", report.tool_version)?;
    writeln!(writer, "Agent schema: v{}", report.schema_version)?;
    writeln!(writer)?;
    writeln!(writer, "Commands:")?;
    for command in &report.commands {
        writeln!(writer, "  {:<13} {}", command.name, command.purpose)?;
        writeln!(writer, "                {}", command.syntax)?;
    }
    writeln!(writer)?;
    writeln!(writer, "Outputs: {}", report.outputs.join(", "))?;
    writeln!(writer, "Path modes: {}", report.path_modes.join(", "))?;
    writeln!(writer)?;
    writeln!(writer, "Guarantees:")?;
    writeln!(writer, "  read-only: yes")?;
    writeln!(writer, "  recursive: yes")?;
    writeln!(writer, "  parallel: yes")?;
    writeln!(writer, "  hidden files included: yes")?;
    writeln!(writer, "  ignore files honored: no")?;
    writeln!(writer, "  follows symlinks/reparse points: no")?;
    writeln!(writer, "  closed downstream stdout is success: yes")?;
    writeln!(writer)?;
    writeln!(writer, "Exit codes:")?;
    for exit in &report.exit_codes {
        writeln!(writer, "  {}  {}", exit.code, exit.meaning)?;
    }
    Ok(())
}

fn write_markdown(report: &Report, directory: &Path) -> AppResult<PathBuf> {
    let now = Local::now();
    let filename_stamp = now.format("%Y-%m-%d_%H%M%S_%3f").to_string();
    let generated = now.to_rfc3339_opts(SecondsFormat::Secs, false);

    for suffix in 0..1000_u16 {
        let filename = if suffix == 0 {
            format!("dirrake_{filename_stamp}.md")
        } else {
            format!("dirrake_{filename_stamp}_{suffix}.md")
        };
        let path = directory.join(filename);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                let mut writer = BufWriter::new(file);
                if let Err(error) = write_markdown_body(report, &generated, &mut writer)
                    .and_then(|()| writer.flush())
                {
                    drop(writer);
                    let _ = fs::remove_file(&path);
                    return Err(AppError::output(anyhow!(
                        "failed to write `{}`: {error}",
                        path.display()
                    )));
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(AppError::output(anyhow!(
                    "failed to create `{}`: {error}",
                    path.display()
                )));
            }
        }
    }
    Err(AppError::output(anyhow!(
        "could not allocate a unique Markdown report filename"
    )))
}

fn write_markdown_body<W: Write>(
    report: &Report,
    generated: &str,
    writer: &mut W,
) -> io::Result<()> {
    match report {
        Report::Files(report) => write_file_markdown(report, generated, writer),
        Report::Directories(report) => write_directory_markdown(report, generated, writer),
        Report::Info(report) => write_info_markdown(report, generated, writer),
        Report::Capabilities(report) => write_capabilities_markdown(report, generated, writer),
    }
}

fn markdown_header<W: Write>(
    writer: &mut W,
    title: &str,
    generated: &str,
    root: Option<&Path>,
) -> io::Result<()> {
    writeln!(writer, "# {title}")?;
    writeln!(writer)?;
    writeln!(writer, "Generated: {}", html_code(generated, false))?;
    if let Some(root) = root {
        writeln!(writer)?;
        writeln!(
            writer,
            "Root: {}",
            html_code(&pathutil::path_string(root), false)
        )?;
    }
    Ok(())
}

fn write_file_markdown<W: Write>(
    report: &FileReport,
    generated: &str,
    writer: &mut W,
) -> io::Result<()> {
    markdown_header(
        writer,
        "DirRake File Results",
        generated,
        Some(&report.root),
    )?;
    writeln!(writer)?;
    writeln!(
        writer,
        "Query: {}",
        html_code(&report.operation.description(), false)
    )?;
    writeln!(writer)?;
    writeln!(writer, "| Size | Path |")?;
    writeln!(writer, "|---:|---|")?;
    for entry in &report.matches {
        writeln!(
            writer,
            "| {} | {} |",
            human_size(u128::from(entry.size_bytes)),
            html_code(
                &display_path(&entry.path, &report.root, report.path_mode),
                true
            )
        )?;
    }
    if report.matches.is_empty() {
        writeln!(writer, "| — | _No files matched_ |")?;
    }
    writeln!(writer)?;
    writeln!(
        writer,
        "**Matches total:** {}  ",
        report.stats.matches_total
    )?;
    writeln!(
        writer,
        "**Matches returned:** {}  ",
        report.stats.matches_returned
    )?;
    writeln!(
        writer,
        "**Matched bytes total:** {}  ",
        human_size(report.stats.matched_bytes_total)
    )?;
    writeln!(writer, "**Truncated:** {}  ", report.stats.truncated)?;
    writeln!(writer, "**Files seen:** {}  ", report.stats.files_seen)?;
    writeln!(
        writer,
        "**Directories seen:** {}  ",
        report.stats.directories_seen
    )?;
    writeln!(writer, "**Elapsed:** {} ms", report.stats.elapsed_ms)?;
    write_warning_markdown(report.stats.warning_count, &report.warning_samples, writer)
}

fn write_directory_markdown<W: Write>(
    report: &DirectoryReport,
    generated: &str,
    writer: &mut W,
) -> io::Result<()> {
    markdown_header(
        writer,
        "DirRake Directory Sizes",
        generated,
        Some(&report.root),
    )?;
    writeln!(writer)?;
    writeln!(writer, "| Size | Files | Directory |")?;
    writeln!(writer, "|---:|---:|---|")?;
    for entry in &report.directories {
        writeln!(
            writer,
            "| {} | {} | {} |",
            human_size(entry.size_bytes),
            entry.file_count,
            html_code(
                &display_path(&entry.path, &report.root, report.path_mode),
                true
            )
        )?;
    }
    if report.directories.is_empty() {
        writeln!(writer, "| — | — | _No subdirectories found_ |")?;
    }
    writeln!(writer)?;
    writeln!(
        writer,
        "**Directories total:** {}  ",
        report.stats.directories_total
    )?;
    writeln!(
        writer,
        "**Directories returned:** {}  ",
        report.stats.directories_returned
    )?;
    writeln!(writer, "**Truncated:** {}  ", report.stats.truncated)?;
    writeln!(
        writer,
        "**Total file data:** {}  ",
        human_size(report.stats.total_file_bytes)
    )?;
    writeln!(writer, "**Elapsed:** {} ms", report.stats.elapsed_ms)?;
    write_warning_markdown(report.stats.warning_count, &report.warning_samples, writer)
}

fn write_info_markdown<W: Write>(
    report: &InfoReport,
    generated: &str,
    writer: &mut W,
) -> io::Result<()> {
    markdown_header(
        writer,
        "DirRake Directory Census",
        generated,
        Some(&report.root),
    )?;
    writeln!(writer)?;
    writeln!(writer, "- Files seen: **{}**", report.stats.files_seen)?;
    writeln!(
        writer,
        "- Directories seen: **{}**",
        report.stats.directories_seen
    )?;
    writeln!(
        writer,
        "- Total size: **{}**",
        human_size(report.stats.total_file_bytes)
    )?;
    writeln!(writer, "- Elapsed: **{} ms**", report.stats.elapsed_ms)?;
    writeln!(writer)?;
    writeln!(writer, "## Largest")?;
    writeln!(writer)?;
    write_largest_markdown(report, writer)?;
    writeln!(writer)?;
    writeln!(writer, "## Extensions")?;
    writeln!(writer)?;
    writeln!(writer, "| Extension | Files | Size |")?;
    writeln!(writer, "|---|---:|---:|")?;
    for extension in &report.extensions {
        writeln!(
            writer,
            "| {} | {} | {} |",
            html_code(&display_extension(&extension.extension), true),
            extension.file_count,
            human_size(extension.size_bytes)
        )?;
    }
    if report.extensions.is_empty() {
        writeln!(writer, "| — | — | — |")?;
    }
    if report.stats.extensions_truncated {
        writeln!(writer)?;
        writeln!(
            writer,
            "_{} of {} extension groups shown._",
            report.stats.extensions_returned, report.stats.extensions_total
        )?;
    }
    write_warning_markdown(report.stats.warning_count, &report.warning_samples, writer)
}

fn write_largest_markdown<W: Write>(report: &InfoReport, writer: &mut W) -> io::Result<()> {
    if let Some(file) = &report.largest_file {
        writeln!(
            writer,
            "- File: **{}** — {}",
            human_size(u128::from(file.size_bytes)),
            html_code(
                &display_path(&file.path, &report.root, report.path_mode),
                false
            )
        )?;
    } else {
        writeln!(writer, "- File: none")?;
    }
    if let Some(directory) = &report.largest_directory {
        writeln!(
            writer,
            "- Directory: **{}** — {}",
            human_size(directory.size_bytes),
            html_code(
                &display_path(&directory.path, &report.root, report.path_mode),
                false
            )
        )?;
    } else {
        writeln!(writer, "- Directory: none")?;
    }
    Ok(())
}

fn write_capabilities_markdown<W: Write>(
    report: &CapabilitiesReport,
    generated: &str,
    writer: &mut W,
) -> io::Result<()> {
    markdown_header(writer, "DirRake Capabilities", generated, None)?;
    writeln!(writer)?;
    writeln!(writer, "Tool version: `{}`  ", report.tool_version)?;
    writeln!(writer, "Agent schema: `v{}`", report.schema_version)?;
    writeln!(writer)?;
    writeln!(writer, "## Commands")?;
    writeln!(writer)?;
    writeln!(writer, "| Command | Purpose | Syntax |")?;
    writeln!(writer, "|---|---|---|")?;
    for command in &report.commands {
        writeln!(
            writer,
            "| `{}` | {} | `{}` |",
            command.name, command.purpose, command.syntax
        )?;
    }
    writeln!(writer)?;
    writeln!(writer, "## Exit codes")?;
    writeln!(writer)?;
    for exit in &report.exit_codes {
        writeln!(writer, "- `{}` — {}", exit.code, exit.meaning)?;
    }
    Ok(())
}

fn write_warning_markdown<W: Write>(
    warning_count: u64,
    samples: &[String],
    writer: &mut W,
) -> io::Result<()> {
    if warning_count == 0 {
        return Ok(());
    }
    writeln!(writer)?;
    writeln!(writer, "## Warnings ({warning_count})")?;
    writeln!(writer)?;
    for warning in samples {
        writeln!(writer, "- {}", html_code(warning, false))?;
    }
    if warning_count > samples.len() as u64 {
        writeln!(
            writer,
            "- _{} additional warning(s) omitted_",
            warning_count - samples.len() as u64
        )?;
    }
    Ok(())
}

fn write_jsonl<W: Write>(report: &Report, writer: &mut W) -> AppResult<()> {
    match agent_json::write_jsonl(report, writer) {
        Ok(()) => Ok(()),
        Err(error) if error.io_error_kind() == Some(io::ErrorKind::BrokenPipe) => Ok(()),
        Err(error) => Err(AppError::output(anyhow!(
            "failed to serialize JSONL output: {error}"
        ))),
    }
}
fn print_terminal_warnings(report: &Report) {
    let (count, samples) = match report {
        Report::Files(report) => (
            report.stats.warning_count,
            report.warning_samples.as_slice(),
        ),
        Report::Directories(report) => (
            report.stats.warning_count,
            report.warning_samples.as_slice(),
        ),
        Report::Info(report) => (
            report.stats.warning_count,
            report.warning_samples.as_slice(),
        ),
        Report::Capabilities(_) => return,
    };
    if count == 0 {
        return;
    }
    eprintln!("Completed with {count} filesystem warning(s):");
    for warning in samples {
        eprintln!("  - {}", pathutil::visible_controls(warning));
    }
    if count > samples.len() as u64 {
        eprintln!(
            "  - ... {} additional warning(s) omitted",
            count - samples.len() as u64
        );
    }
}

fn terminal_path(path: &Path, root: &Path, mode: PathMode) -> String {
    pathutil::visible_controls(&display_path(path, root, mode))
}

fn display_path(path: &Path, root: &Path, mode: PathMode) -> String {
    match mode {
        PathMode::Absolute => path_string(path),
        PathMode::Relative => match path.strip_prefix(root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
            Ok(relative) => path_string(relative),
            Err(_) => path_string(path),
        },
    }
}

fn path_string(path: &Path) -> String {
    pathutil::path_string(path)
}

fn display_extension(extension: &str) -> String {
    if extension == "<none>" {
        extension.to_owned()
    } else {
        format!(".{extension}")
    }
}

fn depth_suffix(max_depth: Option<usize>) -> String {
    max_depth
        .map(|depth| format!(" | depth {depth}"))
        .unwrap_or_default()
}

fn human_size(bytes: u128) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn html_code(value: &str, table_cell: bool) -> String {
    let mut escaped = pathutil::visible_controls(value)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    if table_cell {
        escaped = escaped.replace('|', "&#124;");
    }
    format!("<code>{escaped}</code>")
}

#[cfg(test)]
mod tests {
    use crate::{
        model::{FileOperation, PathMode},
        query::{CompoundQuery, Filter},
        scan::{FileMatch, FileReport, FileScanStats},
    };

    use super::*;

    fn file_report() -> FileReport {
        let root = PathBuf::from("/root");
        FileReport {
            root: root.clone(),
            operation: FileOperation::Filter(CompoundQuery::new(
                Filter::word("camera".into()).unwrap(),
            )),
            path_mode: PathMode::Relative,
            max_depth: None,
            requested_limit: Some(1),
            matches: vec![FileMatch {
                path: root.join("nested/camera.jpg"),
                size_bytes: 42,
            }],
            stats: FileScanStats {
                files_seen: 10,
                directories_seen: 3,
                matches_total: 2,
                matches_returned: 1,
                matched_bytes_total: 84,
                returned_bytes: 42,
                truncated: true,
                warning_count: 0,
                elapsed_ms: 5,
            },
            warning_samples: vec![],
        }
    }

    struct FailingWriter {
        kind: io::ErrorKind,
    }

    impl FailingWriter {
        fn new(kind: io::ErrorKind) -> Self {
            Self { kind }
        }
    }

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(self.kind, "forced writer failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(self.kind, "forced writer failure"))
        }
    }

    #[test]
    fn broken_pipe_is_success_for_all_stdout_surfaces() {
        let report = Report::Files(file_report());

        let mut terminal = FailingWriter::new(io::ErrorKind::BrokenPipe);
        assert!(write_terminal_output(&report, &mut terminal).is_ok());

        let mut json = FailingWriter::new(io::ErrorKind::BrokenPipe);
        assert!(write_json_output(&report, &mut json).is_ok());

        let mut jsonl = FailingWriter::new(io::ErrorKind::BrokenPipe);
        assert!(write_jsonl(&report, &mut jsonl).is_ok());

        let mut markdown = FailingWriter::new(io::ErrorKind::BrokenPipe);
        assert!(write_markdown_confirmation(Path::new("report.md"), &mut markdown).is_ok());
    }

    #[test]
    fn genuine_stdout_failures_remain_exit_code_four() {
        let report = Report::Files(file_report());

        let mut terminal = FailingWriter::new(io::ErrorKind::Other);
        assert_eq!(
            write_terminal_output(&report, &mut terminal)
                .unwrap_err()
                .code(),
            4
        );

        let mut json = FailingWriter::new(io::ErrorKind::Other);
        assert_eq!(write_json_output(&report, &mut json).unwrap_err().code(), 4);

        let mut jsonl = FailingWriter::new(io::ErrorKind::Other);
        assert_eq!(write_jsonl(&report, &mut jsonl).unwrap_err().code(), 4);

        let mut markdown = FailingWriter::new(io::ErrorKind::Other);
        assert_eq!(
            write_markdown_confirmation(Path::new("report.md"), &mut markdown)
                .unwrap_err()
                .code(),
            4
        );
    }

    #[test]
    fn json_has_stable_schema_and_truncation_metadata() {
        let value = json_value(&Report::Files(file_report()));
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["type"], "file_results");
        assert_eq!(value["results"][0]["path"], "nested/camera.jpg");
        assert_eq!(value["stats"]["matches_total"], 2);
        assert_eq!(value["stats"]["truncated"], true);
    }

    #[test]
    fn markdown_creation_failure_uses_exit_code_four() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let error = emit(
            &Report::Files(file_report()),
            OutputMode::Markdown,
            &missing,
        )
        .unwrap_err();
        assert_eq!(error.code(), 4);
    }

    #[test]
    fn capabilities_json_declares_type() {
        let value = json_value(&Report::Capabilities(crate::capabilities::report()));
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["type"], "capabilities");
    }

    #[test]
    fn markdown_code_escaping_preserves_windows_backslashes() {
        let encoded = html_code(r"C:\camera|clip.bin", true);
        assert!(encoded.contains(r"C:\camera"));
        assert!(encoded.contains("&#124;"));
    }
}
