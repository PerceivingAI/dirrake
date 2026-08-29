use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

use ignore::{
    DirEntry, Error as WalkError, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState,
};

use crate::{
    model::{DirsRequest, InfoRequest, PathMode},
    pathutil,
    scan::FileMatch,
    walker,
    warnings::WarningAccumulator,
};

const LOCAL_DIR_FLUSH_THRESHOLD: usize = 4096;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectoryMatch {
    pub path: PathBuf,
    pub size_bytes: u128,
    pub file_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionStat {
    pub extension: String,
    pub file_count: u64,
    pub size_bytes: u128,
}

#[derive(Debug, Clone)]
pub struct DirectoryStats {
    pub files_seen: u64,
    pub directories_seen: u64,
    pub directories_total: usize,
    pub directories_returned: usize,
    pub total_file_bytes: u128,
    pub truncated: bool,
    pub warning_count: u64,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct DirectoryReport {
    pub root: PathBuf,
    pub path_mode: PathMode,
    pub max_depth: Option<usize>,
    pub requested_limit: Option<usize>,
    pub directories: Vec<DirectoryMatch>,
    pub stats: DirectoryStats,
    pub warning_samples: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InfoStats {
    pub files_seen: u64,
    pub directories_seen: u64,
    pub total_file_bytes: u128,
    pub extensions_total: usize,
    pub extensions_returned: usize,
    pub extensions_truncated: bool,
    pub warning_count: u64,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct InfoReport {
    pub root: PathBuf,
    pub path_mode: PathMode,
    pub max_depth: Option<usize>,
    pub requested_limit: Option<usize>,
    pub largest_file: Option<FileMatch>,
    pub largest_directory: Option<DirectoryMatch>,
    pub extensions: Vec<ExtensionStat>,
    pub stats: InfoStats,
    pub warning_samples: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct AnalysisCounts {
    files_seen: u64,
    directories_seen: u64,
    total_file_bytes: u128,
}

#[derive(Debug, Clone, Default)]
struct AnalysisAccum {
    directories: HashMap<PathBuf, DirectoryMatch>,
    extensions: HashMap<String, ExtensionStat>,
    largest_file: Option<FileMatch>,
    counts: AnalysisCounts,
    warnings: WarningAccumulator,
}

struct SharedAnalysis {
    inner: Mutex<AnalysisAccum>,
}

struct AnalysisVisitorBuilder<'a> {
    shared: &'a SharedAnalysis,
}

struct AnalysisVisitor<'a> {
    shared: &'a SharedAnalysis,
    local: AnalysisAccum,
}

impl<'a> ParallelVisitorBuilder<'a> for AnalysisVisitorBuilder<'a> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 'a> {
        Box::new(AnalysisVisitor {
            shared: self.shared,
            local: AnalysisAccum::default(),
        })
    }
}

impl ParallelVisitor for AnalysisVisitor<'_> {
    fn visit(&mut self, entry_result: Result<DirEntry, WalkError>) -> WalkState {
        match entry_result {
            Ok(entry) => self.visit_entry(entry),
            Err(error) => self.record_warning(error.to_string()),
        }
        WalkState::Continue
    }
}

impl AnalysisVisitor<'_> {
    fn visit_entry(&mut self, entry: DirEntry) {
        let Some(file_type) = entry.file_type() else {
            self.record_warning(format!(
                "{}: filesystem entry type unavailable",
                pathutil::path_string(entry.path())
            ));
            return;
        };

        if file_type.is_dir() {
            self.local.counts.directories_seen += 1;
            let path = entry.into_path();
            self.local
                .directories
                .entry(path.clone())
                .or_insert(DirectoryMatch {
                    path,
                    ..DirectoryMatch::default()
                });
            self.flush_dirs_if_needed();
            return;
        }
        if !file_type.is_file() {
            return;
        }

        self.local.counts.files_seen += 1;
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                self.record_warning(format!("{}: {error}", pathutil::path_string(entry.path())));
                return;
            }
        };
        let size = metadata.len();
        let path = entry.into_path();
        self.local.counts.total_file_bytes += u128::from(size);
        update_largest_file(&mut self.local.largest_file, &path, size);
        update_extension(&mut self.local.extensions, &path, size);
        update_direct_directory(&mut self.local.directories, &path, size);
        self.flush_dirs_if_needed();
    }

    fn flush_dirs_if_needed(&mut self) {
        if self.local.directories.len() < LOCAL_DIR_FLUSH_THRESHOLD {
            return;
        }
        let dirs = std::mem::take(&mut self.local.directories);
        let mut shared = lock(&self.shared.inner);
        merge_directories(&mut shared.directories, dirs);
    }

    fn record_warning(&mut self, warning: String) {
        self.local.warnings.record(warning);
    }
}

impl Drop for AnalysisVisitor<'_> {
    fn drop(&mut self) {
        let mut shared = lock(&self.shared.inner);
        merge_directories(
            &mut shared.directories,
            std::mem::take(&mut self.local.directories),
        );
        merge_extensions(
            &mut shared.extensions,
            std::mem::take(&mut self.local.extensions),
        );
        merge_largest_file(&mut shared.largest_file, self.local.largest_file.take());
        shared.counts.files_seen += self.local.counts.files_seen;
        shared.counts.directories_seen += self.local.counts.directories_seen;
        shared.counts.total_file_bytes += self.local.counts.total_file_bytes;
        shared
            .warnings
            .merge(std::mem::take(&mut self.local.warnings));
    }
}

pub fn analyze_dirs(request: &DirsRequest) -> DirectoryReport {
    let started = Instant::now();
    let mut analysis = analyze(&request.options.root, request.options.max_depth);
    let root = request.options.root.clone();
    analysis.directories.remove(&root);

    let mut directories: Vec<_> = analysis.directories.into_values().collect();
    directories.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    let directories_total = directories.len();
    if let Some(limit) = request.options.limit {
        directories.truncate(limit);
    }
    let directories_returned = directories.len();
    DirectoryReport {
        root,
        path_mode: request.options.path_mode,
        max_depth: request.options.max_depth,
        requested_limit: request.options.limit,
        directories,
        stats: DirectoryStats {
            files_seen: analysis.counts.files_seen,
            directories_seen: analysis.counts.directories_seen,
            directories_total,
            directories_returned,
            total_file_bytes: analysis.counts.total_file_bytes,
            truncated: directories_total > directories_returned,
            warning_count: analysis.warnings.count(),
            elapsed_ms: started.elapsed().as_millis(),
        },
        warning_samples: analysis.warnings.samples(),
    }
}

pub fn analyze_info(request: &InfoRequest) -> InfoReport {
    let started = Instant::now();
    let mut analysis = analyze(&request.options.root, request.options.max_depth);
    let root = request.options.root.clone();
    analysis.directories.remove(&root);

    let largest_directory = analysis.directories.into_values().max_by(|left, right| {
        left.size_bytes
            .cmp(&right.size_bytes)
            .then_with(|| right.path.cmp(&left.path))
    });

    let mut extensions: Vec<_> = analysis.extensions.into_values().collect();
    extensions.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| right.file_count.cmp(&left.file_count))
            .then_with(|| left.extension.cmp(&right.extension))
    });
    let extensions_total = extensions.len();
    if let Some(limit) = request.options.limit {
        extensions.truncate(limit);
    }
    let extensions_returned = extensions.len();
    InfoReport {
        root,
        path_mode: request.options.path_mode,
        max_depth: request.options.max_depth,
        requested_limit: request.options.limit,
        largest_file: analysis.largest_file,
        largest_directory,
        extensions,
        stats: InfoStats {
            files_seen: analysis.counts.files_seen,
            directories_seen: analysis.counts.directories_seen,
            total_file_bytes: analysis.counts.total_file_bytes,
            extensions_total,
            extensions_returned,
            extensions_truncated: extensions_total > extensions_returned,
            warning_count: analysis.warnings.count(),
            elapsed_ms: started.elapsed().as_millis(),
        },
        warning_samples: analysis.warnings.samples(),
    }
}

fn analyze(root: &Path, max_depth: Option<usize>) -> AnalysisAccum {
    let shared = SharedAnalysis {
        inner: Mutex::new(AnalysisAccum::default()),
    };
    let mut builder = WalkBuilder::new(root);
    walker::configure(&mut builder, max_depth);

    let mut visitor_builder = AnalysisVisitorBuilder { shared: &shared };
    builder.build_parallel().visit(&mut visitor_builder);

    let mut analysis = std::mem::take(&mut *lock(&shared.inner));
    rollup_directories(&mut analysis.directories, root);
    analysis
}

// The parallel walk records only each file's immediate parent contribution. Recursive
// totals are derived once after traversal, so file processing does not scale with tree depth.
fn update_direct_directory(
    directories: &mut HashMap<PathBuf, DirectoryMatch>,
    file: &Path,
    size: u64,
) {
    let Some(directory) = file.parent() else {
        return;
    };
    let path = directory.to_path_buf();
    let entry = directories.entry(path.clone()).or_insert(DirectoryMatch {
        path,
        ..DirectoryMatch::default()
    });
    entry.size_bytes += u128::from(size);
    entry.file_count += 1;
}

// Convert direct-directory totals into recursive totals by processing deepest directories first.
// Each directory contributes to its parent exactly once.
fn rollup_directories(directories: &mut HashMap<PathBuf, DirectoryMatch>, root: &Path) {
    directories
        .entry(root.to_path_buf())
        .or_insert(DirectoryMatch {
            path: root.to_path_buf(),
            ..DirectoryMatch::default()
        });

    let root_depth = path_depth(root);
    let mut paths: Vec<_> = directories
        .keys()
        .filter(|path| path.as_path() != root)
        .map(|path| (path_depth(path).saturating_sub(root_depth), path.clone()))
        .collect();
    paths.sort_by(|(left_depth, left), (right_depth, right)| {
        right_depth.cmp(left_depth).then_with(|| left.cmp(right))
    });

    for (_, path) in paths {
        let Some(parent) = path.parent() else {
            continue;
        };
        if !parent.starts_with(root) {
            continue;
        }
        let Some((child_size, child_count)) = directories
            .get(&path)
            .map(|child| (child.size_bytes, child.file_count))
        else {
            continue;
        };
        let parent_path = parent.to_path_buf();
        let parent_entry = directories
            .entry(parent_path.clone())
            .or_insert(DirectoryMatch {
                path: parent_path,
                ..DirectoryMatch::default()
            });
        parent_entry.size_bytes += child_size;
        parent_entry.file_count += child_count;
    }
}

fn path_depth(path: &Path) -> usize {
    path.components().count()
}

fn update_extension(extensions: &mut HashMap<String, ExtensionStat>, path: &Path, size: u64) {
    let extension = path
        .extension()
        .map(pathutil::normalized_os_text)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "<none>".to_owned());
    let entry = extensions
        .entry(extension.clone())
        .or_insert(ExtensionStat {
            extension,
            file_count: 0,
            size_bytes: 0,
        });
    entry.file_count += 1;
    entry.size_bytes += u128::from(size);
}

fn update_largest_file(largest: &mut Option<FileMatch>, path: &Path, size: u64) {
    let candidate = FileMatch {
        path: path.to_path_buf(),
        size_bytes: size,
    };
    match largest {
        None => *largest = Some(candidate),
        Some(current)
            if candidate.size_bytes > current.size_bytes
                || (candidate.size_bytes == current.size_bytes
                    && candidate.path < current.path) =>
        {
            *current = candidate;
        }
        Some(_) => {}
    }
}

fn merge_directories(
    target: &mut HashMap<PathBuf, DirectoryMatch>,
    source: HashMap<PathBuf, DirectoryMatch>,
) {
    for (path, value) in source {
        let entry = target.entry(path.clone()).or_insert(DirectoryMatch {
            path,
            ..DirectoryMatch::default()
        });
        entry.size_bytes += value.size_bytes;
        entry.file_count += value.file_count;
    }
}

fn merge_extensions(
    target: &mut HashMap<String, ExtensionStat>,
    source: HashMap<String, ExtensionStat>,
) {
    for (extension, value) in source {
        let entry = target.entry(extension.clone()).or_insert(ExtensionStat {
            extension,
            file_count: 0,
            size_bytes: 0,
        });
        entry.file_count += value.file_count;
        entry.size_bytes += value.size_bytes;
    }
}

fn merge_largest_file(target: &mut Option<FileMatch>, source: Option<FileMatch>) {
    if let Some(source) = source {
        update_largest_file(target, &source.path, source.size_bytes);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::model::{OutputMode, ScanOptions};

    use super::*;

    fn options(root: &Path) -> ScanOptions {
        ScanOptions {
            root: root.to_path_buf(),
            report_dir: root.to_path_buf(),
            output: OutputMode::Terminal,
            limit: None,
            max_depth: None,
            path_mode: PathMode::Absolute,
        }
    }

    #[test]
    fn dirs_reports_recursive_sizes_largest_first() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir_all(a.join("nested")).unwrap();
        fs::create_dir(&b).unwrap();
        fs::File::create(a.join("nested").join("big.bin"))
            .unwrap()
            .set_len(300)
            .unwrap();
        fs::File::create(b.join("small.bin"))
            .unwrap()
            .set_len(100)
            .unwrap();

        let report = analyze_dirs(&DirsRequest {
            options: options(dir.path()),
        });
        assert_eq!(report.directories[0].path, a);
        assert_eq!(report.directories[0].size_bytes, 300);
        assert_eq!(report.directories[0].file_count, 1);
    }

    #[test]
    fn dirs_rolls_direct_and_nested_files_up_exactly_once() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let nested = a.join("nested");
        let deep = nested.join("deep");
        fs::create_dir_all(&deep).unwrap();
        fs::File::create(a.join("direct.bin"))
            .unwrap()
            .set_len(10)
            .unwrap();
        fs::File::create(nested.join("nested.bin"))
            .unwrap()
            .set_len(20)
            .unwrap();
        fs::File::create(deep.join("deep.bin"))
            .unwrap()
            .set_len(30)
            .unwrap();
        fs::File::create(dir.path().join("root.bin"))
            .unwrap()
            .set_len(40)
            .unwrap();

        let report = analyze_dirs(&DirsRequest {
            options: options(dir.path()),
        });
        let by_path: HashMap<_, _> = report
            .directories
            .iter()
            .map(|entry| (entry.path.clone(), (entry.size_bytes, entry.file_count)))
            .collect();

        assert_eq!(by_path[&a], (60, 3));
        assert_eq!(by_path[&nested], (50, 2));
        assert_eq!(by_path[&deep], (30, 1));
        assert_eq!(report.stats.total_file_bytes, 100);
    }

    #[test]
    fn dirs_depth_rollup_counts_only_the_scanned_portion() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a");
        let nested = a.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.path().join("root.bin"), vec![0_u8; 5]).unwrap();
        fs::write(a.join("child.bin"), vec![0_u8; 7]).unwrap();
        fs::write(nested.join("deep.bin"), vec![0_u8; 11]).unwrap();

        let mut opts = options(dir.path());
        opts.max_depth = Some(2);
        let report = analyze_dirs(&DirsRequest { options: opts });
        let by_path: HashMap<_, _> = report
            .directories
            .iter()
            .map(|entry| (entry.path.clone(), (entry.size_bytes, entry.file_count)))
            .collect();

        assert_eq!(by_path[&a], (7, 1));
        assert_eq!(by_path[&nested], (0, 0));
        assert_eq!(report.stats.total_file_bytes, 12);
        assert_eq!(report.stats.files_seen, 2);
    }

    #[test]
    fn dirs_preserves_empty_directory_paths() {
        let dir = tempdir().unwrap();
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();

        let report = analyze_dirs(&DirsRequest {
            options: options(dir.path()),
        });
        assert!(report.directories.iter().any(|entry| entry.path == empty));
    }

    #[test]
    fn dirs_limit_counts_all_but_returns_bounded_rows() {
        let dir = tempdir().unwrap();
        for name in ["a", "b", "c"] {
            let path = dir.path().join(name);
            fs::create_dir(&path).unwrap();
            fs::write(path.join("file"), "x").unwrap();
        }
        let mut opts = options(dir.path());
        opts.limit = Some(2);
        let report = analyze_dirs(&DirsRequest { options: opts });
        assert_eq!(report.stats.directories_total, 3);
        assert_eq!(report.stats.directories_returned, 2);
        assert!(report.stats.truncated);
    }

    #[test]
    fn info_collects_census_largest_items_and_extensions() {
        let dir = tempdir().unwrap();
        let media = dir.path().join("media");
        fs::create_dir(&media).unwrap();
        fs::File::create(media.join("clip.MP4"))
            .unwrap()
            .set_len(200)
            .unwrap();
        fs::File::create(dir.path().join("readme.txt"))
            .unwrap()
            .set_len(50)
            .unwrap();

        let report = analyze_info(&InfoRequest {
            options: options(dir.path()),
        });
        assert_eq!(report.stats.files_seen, 2);
        assert_eq!(report.stats.total_file_bytes, 250);
        assert!(report.largest_file.unwrap().path.ends_with("clip.MP4"));
        assert_eq!(report.largest_directory.unwrap().path, media);
        assert_eq!(report.extensions[0].extension, "mp4");
    }

    #[test]
    fn info_limit_bounds_extensions_but_preserves_total_group_count() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        fs::write(dir.path().join("b.mp4"), "xx").unwrap();
        fs::write(dir.path().join("c.json"), "xxx").unwrap();
        let mut opts = options(dir.path());
        opts.limit = Some(2);

        let report = analyze_info(&InfoRequest { options: opts });
        assert_eq!(report.stats.extensions_total, 3);
        assert_eq!(report.stats.extensions_returned, 2);
        assert!(report.stats.extensions_truncated);
        assert_eq!(report.extensions.len(), 2);
    }

    #[test]
    fn info_depth_is_respected() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a");
        fs::create_dir(&nested).unwrap();
        fs::write(dir.path().join("root.txt"), "x").unwrap();
        fs::write(nested.join("deep.txt"), "x").unwrap();
        let mut opts = options(dir.path());
        opts.max_depth = Some(1);
        let report = analyze_info(&InfoRequest { options: opts });
        assert_eq!(report.stats.files_seen, 1);
    }
}
