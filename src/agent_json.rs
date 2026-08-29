use std::{fmt, io, path::Path};

use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::Value;

use crate::{
    analyze::{DirectoryMatch, DirectoryReport, ExtensionStat, InfoReport},
    capabilities::CapabilitiesReport,
    model::{AGENT_SCHEMA_VERSION, FileOperation, PathMode},
    output::Report,
    pathutil,
    query::Filter,
    scan::{FileMatch, FileReport},
};

pub(crate) struct AgentReport<'a>(pub(crate) &'a Report);

impl Serialize for AgentReport<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            Report::Files(report) => FileReportView::new(report).serialize(serializer),
            Report::Directories(report) => DirectoryReportView::new(report).serialize(serializer),
            Report::Info(report) => InfoReportView::new(report).serialize(serializer),
            Report::Capabilities(report) => CapabilitiesView::new(report).serialize(serializer),
        }
    }
}

pub(crate) fn value(report: &Report) -> Value {
    serde_json::to_value(AgentReport(report)).expect("agent report serialization cannot fail")
}

pub(crate) fn write_jsonl<W: io::Write>(
    report: &Report,
    writer: &mut W,
) -> Result<(), StreamError> {
    match report {
        Report::Files(report) => {
            write_record(writer, &FileMetaView::new(report))?;
            for entry in &report.matches {
                write_record(writer, &FileJsonlMatchView::new(entry, report))?;
            }
            write_record(writer, &FileSummaryView::new(report))?;
        }
        Report::Directories(report) => {
            write_record(writer, &DirectoryMetaView::new(report))?;
            for entry in &report.directories {
                write_record(writer, &DirectoryJsonlMatchView::new(entry, report))?;
            }
            write_record(writer, &DirectorySummaryView::new(report))?;
        }
        Report::Info(_) | Report::Capabilities(_) => {
            write_record(writer, &AgentReport(report))?;
        }
    }
    Ok(())
}

fn write_record<W: io::Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), StreamError> {
    serde_json::to_writer(&mut *writer, value).map_err(StreamError::Serialize)?;
    writeln!(writer).map_err(StreamError::Io)
}

#[derive(Debug)]
pub(crate) enum StreamError {
    Serialize(serde_json::Error),
    Io(io::Error),
}

impl StreamError {
    pub(crate) fn io_error_kind(&self) -> Option<io::ErrorKind> {
        match self {
            Self::Serialize(error) => error.io_error_kind(),
            Self::Io(error) => Some(error.kind()),
        }
    }
}

impl fmt::Display for StreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialize(error) => write!(formatter, "{error}"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for StreamError {}

#[derive(Serialize)]
struct FileReportView<'a> {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    root: String,
    operation: OperationView<'a>,
    controls: ControlsView,
    results: FileMatchesView<'a>,
    stats: FileStatsView,
    warnings: WarningsView<'a>,
}

impl<'a> FileReportView<'a> {
    fn new(report: &'a FileReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "file_results",
            root: pathutil::path_string(&report.root),
            operation: OperationView(&report.operation),
            controls: ControlsView::new(report.path_mode, report.max_depth, report.requested_limit),
            results: FileMatchesView { report },
            stats: FileStatsView::new(report),
            warnings: WarningsView::new(report.stats.warning_count, &report.warning_samples),
        }
    }
}

#[derive(Serialize)]
struct DirectoryReportView<'a> {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    root: String,
    controls: ControlsView,
    results: DirectoryMatchesView<'a>,
    stats: DirectoryStatsView,
    warnings: WarningsView<'a>,
}

impl<'a> DirectoryReportView<'a> {
    fn new(report: &'a DirectoryReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "directory_results",
            root: pathutil::path_string(&report.root),
            controls: ControlsView::new(report.path_mode, report.max_depth, report.requested_limit),
            results: DirectoryMatchesView { report },
            stats: DirectoryStatsView::new(report),
            warnings: WarningsView::new(report.stats.warning_count, &report.warning_samples),
        }
    }
}

#[derive(Serialize)]
struct InfoReportView<'a> {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    root: String,
    controls: ControlsView,
    largest_file: Option<FileMatchView>,
    largest_directory: Option<DirectoryMatchView>,
    extensions: ExtensionListView<'a>,
    stats: InfoStatsView,
    warnings: WarningsView<'a>,
}

impl<'a> InfoReportView<'a> {
    fn new(report: &'a InfoReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "info",
            root: pathutil::path_string(&report.root),
            controls: ControlsView::new(report.path_mode, report.max_depth, report.requested_limit),
            largest_file: report
                .largest_file
                .as_ref()
                .map(|entry| FileMatchView::new(entry, &report.root, report.path_mode)),
            largest_directory: report
                .largest_directory
                .as_ref()
                .map(|entry| DirectoryMatchView::new(entry, &report.root, report.path_mode)),
            extensions: ExtensionListView(&report.extensions),
            stats: InfoStatsView::new(report),
            warnings: WarningsView::new(report.stats.warning_count, &report.warning_samples),
        }
    }
}

#[derive(Serialize)]
struct CapabilitiesView<'a> {
    #[serde(flatten)]
    report: &'a CapabilitiesReport,
    #[serde(rename = "type")]
    kind: &'static str,
}

impl<'a> CapabilitiesView<'a> {
    fn new(report: &'a CapabilitiesReport) -> Self {
        Self {
            report,
            kind: "capabilities",
        }
    }
}

#[derive(Serialize)]
struct ControlsView {
    path_mode: &'static str,
    max_depth: Option<usize>,
    limit: Option<usize>,
}

impl ControlsView {
    fn new(path_mode: PathMode, max_depth: Option<usize>, limit: Option<usize>) -> Self {
        Self {
            path_mode: path_mode.as_str(),
            max_depth,
            limit,
        }
    }
}

struct OperationView<'a>(&'a FileOperation);

impl Serialize for OperationView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            FileOperation::Top { count } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "top")?;
                map.serialize_entry("count", count)?;
                map.end()
            }
            FileOperation::Filter(query) => {
                let description = query.description();
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry("type", "filters")?;
                map.serialize_entry("operator", "and")?;
                map.serialize_entry("description", &description)?;
                map.serialize_entry("filters", &FilterListView(query.filters()))?;
                map.end()
            }
        }
    }
}

struct FilterListView<'a>(&'a [Filter]);

impl Serialize for FilterListView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for filter in self.0 {
            sequence.serialize_element(&FilterView(filter))?;
        }
        sequence.end()
    }
}

struct FilterView<'a>(&'a Filter);

impl Serialize for FilterView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0 {
            Filter::SizeGreaterThan {
                bytes,
                threshold_mib,
            } => {
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry("type", "size")?;
                map.serialize_entry("operator", "gt")?;
                map.serialize_entry("mib", threshold_mib)?;
                map.serialize_entry("bytes", bytes)?;
                map.end()
            }
            Filter::Word { needle, .. } => {
                let mut map = serializer.serialize_map(Some(4))?;
                map.serialize_entry("type", "word")?;
                map.serialize_entry("text", needle)?;
                map.serialize_entry("case_sensitive", &false)?;
                map.serialize_entry("target", "filename")?;
                map.end()
            }
            Filter::Extension { extension, .. } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("type", "ext")?;
                map.serialize_entry("extension", extension)?;
                map.serialize_entry("case_sensitive", &false)?;
                map.end()
            }
            Filter::OlderThan { days } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "older")?;
                map.serialize_entry("days", days)?;
                map.end()
            }
            Filter::NewerThan { days } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "newer")?;
                map.serialize_entry("days", days)?;
                map.end()
            }
            Filter::Empty => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "empty")?;
                map.end()
            }
        }
    }
}

struct FileMatchesView<'a> {
    report: &'a FileReport,
}

impl Serialize for FileMatchesView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.report.matches.len()))?;
        for entry in &self.report.matches {
            sequence.serialize_element(&FileMatchView::new(
                entry,
                &self.report.root,
                self.report.path_mode,
            ))?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct FileMatchView {
    path: String,
    size_bytes: u64,
}

impl FileMatchView {
    fn new(entry: &FileMatch, root: &Path, path_mode: PathMode) -> Self {
        Self {
            path: display_path(&entry.path, root, path_mode),
            size_bytes: entry.size_bytes,
        }
    }
}

struct DirectoryMatchesView<'a> {
    report: &'a DirectoryReport,
}

impl Serialize for DirectoryMatchesView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.report.directories.len()))?;
        for entry in &self.report.directories {
            sequence.serialize_element(&DirectoryMatchView::new(
                entry,
                &self.report.root,
                self.report.path_mode,
            ))?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct DirectoryMatchView {
    path: String,
    size_bytes: u128,
    file_count: u64,
}

impl DirectoryMatchView {
    fn new(entry: &DirectoryMatch, root: &Path, path_mode: PathMode) -> Self {
        Self {
            path: display_path(&entry.path, root, path_mode),
            size_bytes: entry.size_bytes,
            file_count: entry.file_count,
        }
    }
}

struct ExtensionListView<'a>(&'a [ExtensionStat]);

impl Serialize for ExtensionListView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for extension in self.0 {
            sequence.serialize_element(&ExtensionView(extension))?;
        }
        sequence.end()
    }
}

struct ExtensionView<'a>(&'a ExtensionStat);

impl Serialize for ExtensionView<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("extension", &self.0.extension)?;
        map.serialize_entry("file_count", &self.0.file_count)?;
        map.serialize_entry("size_bytes", &self.0.size_bytes)?;
        map.end()
    }
}

#[derive(Serialize)]
struct FileStatsView {
    files_seen: u64,
    directories_seen: u64,
    matches_total: u64,
    matches_returned: usize,
    matched_bytes_total: u128,
    returned_bytes: u128,
    truncated: bool,
    warning_count: u64,
    elapsed_ms: u128,
}

impl FileStatsView {
    fn new(report: &FileReport) -> Self {
        Self {
            files_seen: report.stats.files_seen,
            directories_seen: report.stats.directories_seen,
            matches_total: report.stats.matches_total,
            matches_returned: report.stats.matches_returned,
            matched_bytes_total: report.stats.matched_bytes_total,
            returned_bytes: report.stats.returned_bytes,
            truncated: report.stats.truncated,
            warning_count: report.stats.warning_count,
            elapsed_ms: report.stats.elapsed_ms,
        }
    }
}

#[derive(Serialize)]
struct DirectoryStatsView {
    files_seen: u64,
    directories_seen: u64,
    directories_total: usize,
    directories_returned: usize,
    total_file_bytes: u128,
    truncated: bool,
    warning_count: u64,
    elapsed_ms: u128,
}

impl DirectoryStatsView {
    fn new(report: &DirectoryReport) -> Self {
        Self {
            files_seen: report.stats.files_seen,
            directories_seen: report.stats.directories_seen,
            directories_total: report.stats.directories_total,
            directories_returned: report.stats.directories_returned,
            total_file_bytes: report.stats.total_file_bytes,
            truncated: report.stats.truncated,
            warning_count: report.stats.warning_count,
            elapsed_ms: report.stats.elapsed_ms,
        }
    }
}

#[derive(Serialize)]
struct InfoStatsView {
    files_seen: u64,
    directories_seen: u64,
    total_file_bytes: u128,
    extensions_total: usize,
    extensions_returned: usize,
    extensions_truncated: bool,
    warning_count: u64,
    elapsed_ms: u128,
}

impl InfoStatsView {
    fn new(report: &InfoReport) -> Self {
        Self {
            files_seen: report.stats.files_seen,
            directories_seen: report.stats.directories_seen,
            total_file_bytes: report.stats.total_file_bytes,
            extensions_total: report.stats.extensions_total,
            extensions_returned: report.stats.extensions_returned,
            extensions_truncated: report.stats.extensions_truncated,
            warning_count: report.stats.warning_count,
            elapsed_ms: report.stats.elapsed_ms,
        }
    }
}

#[derive(Serialize)]
struct WarningsView<'a> {
    count: u64,
    samples: &'a [String],
    samples_truncated: bool,
}

impl<'a> WarningsView<'a> {
    fn new(count: u64, samples: &'a [String]) -> Self {
        Self {
            count,
            samples,
            samples_truncated: count > samples.len() as u64,
        }
    }
}

#[derive(Serialize)]
struct FileMetaView<'a> {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    report_type: &'static str,
    root: String,
    operation: OperationView<'a>,
    controls: ControlsView,
}

impl<'a> FileMetaView<'a> {
    fn new(report: &'a FileReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "meta",
            report_type: "file_results",
            root: pathutil::path_string(&report.root),
            operation: OperationView(&report.operation),
            controls: ControlsView::new(report.path_mode, report.max_depth, report.requested_limit),
        }
    }
}

#[derive(Serialize)]
struct FileJsonlMatchView {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    result: FileMatchView,
}

impl FileJsonlMatchView {
    fn new(entry: &FileMatch, report: &FileReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "match",
            result: FileMatchView::new(entry, &report.root, report.path_mode),
        }
    }
}

#[derive(Serialize)]
struct FileSummaryView<'a> {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    report_type: &'static str,
    stats: FileStatsView,
    warnings: WarningsView<'a>,
}

impl<'a> FileSummaryView<'a> {
    fn new(report: &'a FileReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "summary",
            report_type: "file_results",
            stats: FileStatsView::new(report),
            warnings: WarningsView::new(report.stats.warning_count, &report.warning_samples),
        }
    }
}

#[derive(Serialize)]
struct DirectoryMetaView {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    report_type: &'static str,
    root: String,
    controls: ControlsView,
}

impl DirectoryMetaView {
    fn new(report: &DirectoryReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "meta",
            report_type: "directory_results",
            root: pathutil::path_string(&report.root),
            controls: ControlsView::new(report.path_mode, report.max_depth, report.requested_limit),
        }
    }
}

#[derive(Serialize)]
struct DirectoryJsonlMatchView {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    result: DirectoryMatchView,
}

impl DirectoryJsonlMatchView {
    fn new(entry: &DirectoryMatch, report: &DirectoryReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "directory",
            result: DirectoryMatchView::new(entry, &report.root, report.path_mode),
        }
    }
}

#[derive(Serialize)]
struct DirectorySummaryView<'a> {
    schema_version: u32,
    #[serde(rename = "type")]
    kind: &'static str,
    report_type: &'static str,
    stats: DirectoryStatsView,
    warnings: WarningsView<'a>,
}

impl<'a> DirectorySummaryView<'a> {
    fn new(report: &'a DirectoryReport) -> Self {
        Self {
            schema_version: AGENT_SCHEMA_VERSION,
            kind: "summary",
            report_type: "directory_results",
            stats: DirectoryStatsView::new(report),
            warnings: WarningsView::new(report.stats.warning_count, &report.warning_samples),
        }
    }
}
fn display_path(path: &Path, root: &Path, mode: PathMode) -> String {
    match mode {
        PathMode::Absolute => pathutil::path_string(path),
        PathMode::Relative => match path.strip_prefix(root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".to_owned(),
            Ok(relative) => pathutil::path_string(relative),
            Err(_) => pathutil::path_string(path),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        analyze::{DirectoryMatch, DirectoryReport, DirectoryStats},
        model::FileOperation,
        query::{CompoundQuery, Filter},
        scan::{FileReport, FileScanStats},
    };

    use super::*;

    fn report_with_operation(operation: FileOperation) -> Report {
        Report::Files(FileReport {
            root: PathBuf::from("/root"),
            operation,
            path_mode: PathMode::Relative,
            max_depth: Some(3),
            requested_limit: Some(10),
            matches: Vec::new(),
            stats: FileScanStats {
                files_seen: 0,
                directories_seen: 0,
                matches_total: 0,
                matches_returned: 0,
                matched_bytes_total: 0,
                returned_bytes: 0,
                truncated: false,
                warning_count: 0,
                elapsed_ms: 0,
            },
            warning_samples: Vec::new(),
        })
    }

    #[test]
    fn every_filter_keeps_its_schema_v1_record_shape() {
        let mut query = CompoundQuery::new(Filter::size_greater_than_mib(100).unwrap());
        query.push(Filter::word("camera".to_owned()).unwrap());
        query.push(Filter::extension("MP4".to_owned()).unwrap());
        query.push(Filter::older_than_days(90).unwrap());
        query.push(Filter::newer_than_days(7).unwrap());
        query.push(Filter::Empty);

        let value = value(&report_with_operation(FileOperation::Filter(query)));
        let filters = value["operation"]["filters"].as_array().unwrap();
        assert_eq!(
            filters[0],
            serde_json::json!({
                "type": "size",
                "operator": "gt",
                "mib": 100,
                "bytes": 104857600_u64,
            })
        );
        assert_eq!(
            filters[1],
            serde_json::json!({
                "type": "word",
                "text": "camera",
                "case_sensitive": false,
                "target": "filename",
            })
        );
        assert_eq!(
            filters[2],
            serde_json::json!({
                "type": "ext",
                "extension": "MP4",
                "case_sensitive": false,
            })
        );
        assert_eq!(filters[3], serde_json::json!({"type": "older", "days": 90}));
        assert_eq!(filters[4], serde_json::json!({"type": "newer", "days": 7}));
        assert_eq!(filters[5], serde_json::json!({"type": "empty"}));
    }

    #[test]
    fn json_value_preserves_full_width_u128_totals() {
        let root = PathBuf::from("/root");
        let report = Report::Directories(DirectoryReport {
            root: root.clone(),
            path_mode: PathMode::Relative,
            max_depth: None,
            requested_limit: None,
            directories: vec![DirectoryMatch {
                path: root.join("huge"),
                size_bytes: u128::MAX,
                file_count: 1,
            }],
            stats: DirectoryStats {
                files_seen: 1,
                directories_seen: 2,
                directories_total: 1,
                directories_returned: 1,
                total_file_bytes: u128::MAX,
                truncated: false,
                warning_count: 0,
                elapsed_ms: 1,
            },
            warning_samples: Vec::new(),
        });

        let value = value(&report);
        assert_eq!(
            value["results"][0]["size_bytes"].to_string(),
            u128::MAX.to_string()
        );
        assert_eq!(
            value["stats"]["total_file_bytes"].to_string(),
            u128::MAX.to_string()
        );
    }

    #[test]
    fn warning_truncation_uses_event_count_not_unique_sample_count() {
        let mut report = report_with_operation(FileOperation::Top { count: 1 });
        let Report::Files(report) = &mut report else {
            unreachable!();
        };
        report.stats.warning_count = 3;
        report.warning_samples = vec!["a".to_owned(), "b".to_owned()];

        let value = value(&Report::Files(report.clone()));
        assert_eq!(value["warnings"]["count"], 3);
        assert_eq!(value["warnings"]["samples"].as_array().unwrap().len(), 2);
        assert_eq!(value["warnings"]["samples_truncated"], true);
    }

    #[test]
    fn top_operation_keeps_schema_v1_record_shape() {
        let value = value(&report_with_operation(FileOperation::Top { count: 20 }));
        assert_eq!(
            value["operation"],
            serde_json::json!({"type": "top", "count": 20})
        );
        assert_eq!(value["controls"]["path_mode"], "relative");
        assert_eq!(value["controls"]["max_depth"], 3);
        assert_eq!(value["controls"]["limit"], 10);
    }
}
