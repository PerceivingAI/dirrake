use std::{
    path::PathBuf,
    sync::Mutex,
    time::{Instant, SystemTime},
};

use ignore::{
    DirEntry, Error as WalkError, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState,
};

use crate::{
    model::{FileOperation, FileRequest, PathMode},
    pathutil, walker,
    warnings::WarningAccumulator,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatch {
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct FileScanStats {
    pub files_seen: u64,
    pub directories_seen: u64,
    pub matches_total: u64,
    pub matches_returned: usize,
    pub matched_bytes_total: u128,
    pub returned_bytes: u128,
    pub truncated: bool,
    pub warning_count: u64,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct FileReport {
    pub root: PathBuf,
    pub operation: FileOperation,
    pub path_mode: PathMode,
    pub max_depth: Option<usize>,
    pub requested_limit: Option<usize>,
    pub matches: Vec<FileMatch>,
    pub stats: FileScanStats,
    pub warning_samples: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortMode {
    PathAsc,
    SizeDesc,
}

#[derive(Default)]
struct Counts {
    files_seen: u64,
    directories_seen: u64,
    matches_total: u64,
    matched_bytes_total: u128,
}

struct SharedResults {
    inner: Mutex<SharedInner>,
}

#[derive(Default)]
struct SharedInner {
    matches: Vec<FileMatch>,
    counts: Counts,
    warnings: WarningAccumulator,
}

struct VisitorBuilder<'a> {
    operation: &'a FileOperation,
    shared: &'a SharedResults,
    sort_mode: SortMode,
    retention_limit: Option<usize>,
    now: SystemTime,
}

struct Visitor<'a> {
    operation: &'a FileOperation,
    shared: &'a SharedResults,
    sort_mode: SortMode,
    retention_limit: Option<usize>,
    now: SystemTime,
    matches: Vec<FileMatch>,
    counts: Counts,
    warnings: WarningAccumulator,
}

impl<'a> ParallelVisitorBuilder<'a> for VisitorBuilder<'a> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 'a> {
        Box::new(Visitor {
            operation: self.operation,
            shared: self.shared,
            sort_mode: self.sort_mode,
            retention_limit: self.retention_limit,
            now: self.now,
            matches: Vec::new(),
            counts: Counts::default(),
            warnings: WarningAccumulator::default(),
        })
    }
}

impl ParallelVisitor for Visitor<'_> {
    fn visit(&mut self, entry_result: Result<DirEntry, WalkError>) -> WalkState {
        match entry_result {
            Ok(entry) => self.visit_entry(entry),
            Err(error) => self.record_warning(error.to_string()),
        }
        WalkState::Continue
    }
}

impl Visitor<'_> {
    fn visit_entry(&mut self, entry: DirEntry) {
        let Some(file_type) = entry.file_type() else {
            self.record_warning(format!(
                "{}: filesystem entry type unavailable",
                pathutil::path_string(entry.path())
            ));
            return;
        };

        if file_type.is_dir() {
            self.counts.directories_seen += 1;
            return;
        }
        if !file_type.is_file() {
            return;
        }
        self.counts.files_seen += 1;

        if let FileOperation::Filter(query) = self.operation
            && !query.path_candidate(entry.path())
        {
            return;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                self.record_warning(format!("{}: {error}", pathutil::path_string(entry.path())));
                return;
            }
        };

        let matches = match self.operation {
            FileOperation::Top { .. } => true,
            FileOperation::Filter(query) => match query.metadata_matches(&metadata, self.now) {
                Ok(matches) => matches,
                Err(error) => {
                    self.record_warning(format!(
                        "{}: {error}",
                        pathutil::path_string(entry.path())
                    ));
                    return;
                }
            },
        };

        if !matches {
            return;
        }

        self.counts.matches_total += 1;
        self.counts.matched_bytes_total += u128::from(metadata.len());
        self.matches.push(FileMatch {
            path: entry.into_path(),
            size_bytes: metadata.len(),
        });
        self.trim_local_if_needed();
    }

    fn trim_local_if_needed(&mut self) {
        let Some(limit) = self.retention_limit else {
            return;
        };
        let trim_at = limit.saturating_mul(2).max(limit.saturating_add(1));
        if self.matches.len() >= trim_at {
            sort_and_truncate(&mut self.matches, self.sort_mode, limit);
        }
    }

    fn record_warning(&mut self, warning: String) {
        self.warnings.record(warning);
    }
}

impl Drop for Visitor<'_> {
    fn drop(&mut self) {
        if let Some(limit) = self.retention_limit {
            sort_and_truncate(&mut self.matches, self.sort_mode, limit);
        }

        let mut shared = lock(&self.shared.inner);
        shared.counts.files_seen += self.counts.files_seen;
        shared.counts.directories_seen += self.counts.directories_seen;
        shared.counts.matches_total += self.counts.matches_total;
        shared.counts.matched_bytes_total += self.counts.matched_bytes_total;
        shared.warnings.merge(std::mem::take(&mut self.warnings));
        shared.matches.append(&mut self.matches);

        if let Some(limit) = self.retention_limit {
            sort_and_truncate(&mut shared.matches, self.sort_mode, limit);
        }
    }
}

pub fn scan_files(request: &FileRequest) -> FileReport {
    let started = Instant::now();
    let sort_mode = sort_mode(&request.operation);
    let retention_limit = match request.operation {
        FileOperation::Top { count } => Some(count),
        FileOperation::Filter(_) => request.options.limit,
    };
    let shared = SharedResults {
        inner: Mutex::new(SharedInner::default()),
    };

    let mut builder = WalkBuilder::new(&request.options.root);
    walker::configure(&mut builder, request.options.max_depth);

    let mut visitor_builder = VisitorBuilder {
        operation: &request.operation,
        shared: &shared,
        sort_mode,
        retention_limit,
        now: SystemTime::now(),
    };
    builder.build_parallel().visit(&mut visitor_builder);

    let mut inner = std::mem::take(&mut *lock(&shared.inner));
    sort_matches(&mut inner.matches, sort_mode);
    if let Some(limit) = retention_limit {
        inner.matches.truncate(limit);
    }
    let returned_bytes = inner
        .matches
        .iter()
        .map(|entry| u128::from(entry.size_bytes))
        .sum();
    let matches_returned = inner.matches.len();
    let truncated = inner.counts.matches_total > matches_returned as u64;

    FileReport {
        root: request.options.root.clone(),
        operation: request.operation.clone(),
        path_mode: request.options.path_mode,
        max_depth: request.options.max_depth,
        requested_limit: retention_limit,
        matches: inner.matches,
        stats: FileScanStats {
            files_seen: inner.counts.files_seen,
            directories_seen: inner.counts.directories_seen,
            matches_total: inner.counts.matches_total,
            matches_returned,
            matched_bytes_total: inner.counts.matched_bytes_total,
            returned_bytes,
            truncated,
            warning_count: inner.warnings.count(),
            elapsed_ms: started.elapsed().as_millis(),
        },
        warning_samples: inner.warnings.samples(),
    }
}

fn sort_mode(operation: &FileOperation) -> SortMode {
    match operation {
        FileOperation::Top { .. } => SortMode::SizeDesc,
        FileOperation::Filter(query) if query.sorts_by_size() => SortMode::SizeDesc,
        FileOperation::Filter(_) => SortMode::PathAsc,
    }
}

fn sort_and_truncate(matches: &mut Vec<FileMatch>, mode: SortMode, limit: usize) {
    sort_matches(matches, mode);
    matches.truncate(limit);
}

fn sort_matches(matches: &mut [FileMatch], mode: SortMode) {
    match mode {
        SortMode::PathAsc => matches.sort_by(|left, right| left.path.cmp(&right.path)),
        SortMode::SizeDesc => matches.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.path.cmp(&right.path))
        }),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use crate::{
        model::{OutputMode, ScanOptions},
        query::{CompoundQuery, Filter},
    };

    use super::*;

    fn request(root: &Path, operation: FileOperation) -> FileRequest {
        FileRequest {
            options: ScanOptions {
                root: root.to_path_buf(),
                report_dir: root.to_path_buf(),
                output: OutputMode::Terminal,
                limit: None,
                max_depth: None,
                path_mode: PathMode::Absolute,
            },
            operation,
        }
    }

    #[test]
    fn size_filter_is_recursive_and_strict() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::File::create(dir.path().join("exact.bin"))
            .unwrap()
            .set_len(1024 * 1024)
            .unwrap();
        let large = nested.join("large.bin");
        fs::File::create(&large)
            .unwrap()
            .set_len(2 * 1024 * 1024)
            .unwrap();

        let query = CompoundQuery::new(Filter::size_greater_than_mib(1).unwrap());
        let report = scan_files(&request(dir.path(), FileOperation::Filter(query)));
        assert_eq!(report.matches.len(), 1);
        assert_eq!(report.matches[0].path, large);
    }

    #[test]
    fn hidden_and_gitignored_files_are_included() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "camera-hidden.txt\n").unwrap();
        fs::write(dir.path().join("camera-hidden.txt"), "x").unwrap();
        fs::write(dir.path().join(".camera-dot"), "x").unwrap();

        let query = CompoundQuery::new(Filter::word("camera".into()).unwrap());
        let report = scan_files(&request(dir.path(), FileOperation::Filter(query)));
        assert_eq!(report.matches.len(), 2);
    }

    #[test]
    fn compound_filter_applies_in_one_scan() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("camera.mp4"))
            .unwrap()
            .set_len(2 * 1024 * 1024)
            .unwrap();
        fs::File::create(dir.path().join("camera.jpg"))
            .unwrap()
            .set_len(2 * 1024 * 1024)
            .unwrap();

        let mut query = CompoundQuery::new(Filter::word("camera".into()).unwrap());
        query.push(Filter::extension("mp4".into()).unwrap());
        query.push(Filter::size_greater_than_mib(1).unwrap());
        let report = scan_files(&request(dir.path(), FileOperation::Filter(query)));
        assert_eq!(report.matches.len(), 1);
        assert!(report.matches[0].path.ends_with("camera.mp4"));
    }

    #[test]
    fn limit_retains_only_needed_rows_but_counts_all_matches() {
        let dir = tempdir().unwrap();
        for name in ["camera-a.txt", "camera-b.txt", "camera-c.txt"] {
            fs::write(dir.path().join(name), "x").unwrap();
        }
        let mut request = request(
            dir.path(),
            FileOperation::Filter(CompoundQuery::new(Filter::word("camera".into()).unwrap())),
        );
        request.options.limit = Some(2);

        let report = scan_files(&request);
        assert_eq!(report.stats.matches_total, 3);
        assert_eq!(report.stats.matches_returned, 2);
        assert!(report.stats.truncated);
        assert!(report.matches[0].path.ends_with("camera-a.txt"));
        assert!(report.matches[1].path.ends_with("camera-b.txt"));
    }

    #[test]
    fn top_keeps_largest_files_without_returning_every_file() {
        let dir = tempdir().unwrap();
        for (name, size) in [("one.bin", 1), ("three.bin", 3), ("two.bin", 2)] {
            fs::File::create(dir.path().join(name))
                .unwrap()
                .set_len(size * 1024)
                .unwrap();
        }
        let report = scan_files(&request(dir.path(), FileOperation::Top { count: 2 }));
        assert_eq!(report.stats.matches_total, 3);
        assert_eq!(report.matches.len(), 2);
        assert!(report.matches[0].path.ends_with("three.bin"));
        assert!(report.matches[1].path.ends_with("two.bin"));
    }

    #[test]
    fn depth_bounds_traversal() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("camera-root.txt"), "x").unwrap();
        let nested = dir.path().join("one");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("camera-deep.txt"), "x").unwrap();

        let mut request = request(
            dir.path(),
            FileOperation::Filter(CompoundQuery::new(Filter::word("camera".into()).unwrap())),
        );
        request.options.max_depth = Some(1);
        let report = scan_files(&request);
        assert_eq!(report.matches.len(), 1);
        assert!(report.matches[0].path.ends_with("camera-root.txt"));
    }
}
