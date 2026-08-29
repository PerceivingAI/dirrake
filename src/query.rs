use std::{
    fs::Metadata,
    io,
    path::Path,
    time::{Duration, SystemTime},
};

use crate::pathutil;

const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    SizeGreaterThan {
        bytes: u64,
        threshold_mib: u64,
    },
    Word {
        needle: String,
        needle_lower: String,
    },
    Extension {
        extension: String,
        extension_lower: String,
    },
    OlderThan {
        days: u64,
    },
    NewerThan {
        days: u64,
    },
    Empty,
}

impl Filter {
    pub fn size_greater_than_mib(mib: u64) -> anyhow::Result<Self> {
        let bytes = mib
            .checked_mul(1024 * 1024)
            .ok_or_else(|| anyhow::anyhow!("size threshold is too large"))?;
        Ok(Self::SizeGreaterThan {
            bytes,
            threshold_mib: mib,
        })
    }

    pub fn word(needle: String) -> anyhow::Result<Self> {
        if needle.is_empty() {
            anyhow::bail!("word must not be empty");
        }
        let needle_lower = needle.to_lowercase();
        Ok(Self::Word {
            needle,
            needle_lower,
        })
    }

    pub fn extension(extension: String) -> anyhow::Result<Self> {
        let extension = extension.trim_start_matches('.').to_owned();
        if extension.is_empty() {
            anyhow::bail!("extension must not be empty");
        }
        let extension_lower = extension.to_lowercase();
        Ok(Self::Extension {
            extension,
            extension_lower,
        })
    }

    pub fn older_than_days(days: u64) -> anyhow::Result<Self> {
        validate_days(days)?;
        Ok(Self::OlderThan { days })
    }

    pub fn newer_than_days(days: u64) -> anyhow::Result<Self> {
        validate_days(days)?;
        Ok(Self::NewerThan { days })
    }

    pub fn path_candidate(&self, path: &Path) -> bool {
        match self {
            Self::Word { needle_lower, .. } => path
                .file_name()
                .is_some_and(|name| pathutil::contains_case_insensitive(name, needle_lower)),
            Self::Extension {
                extension_lower, ..
            } => path
                .extension()
                .is_some_and(|extension| pathutil::eq_case_insensitive(extension, extension_lower)),
            _ => true,
        }
    }

    pub fn metadata_matches(&self, metadata: &Metadata, now: SystemTime) -> io::Result<bool> {
        match self {
            Self::SizeGreaterThan { bytes, .. } => Ok(metadata.len() > *bytes),
            Self::Word { .. } | Self::Extension { .. } => Ok(true),
            Self::OlderThan { days } => modified_is_older(metadata.modified()?, now, *days),
            Self::NewerThan { days } => modified_is_newer(metadata.modified()?, now, *days),
            Self::Empty => Ok(metadata.len() == 0),
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::SizeGreaterThan { threshold_mib, .. } => {
                format!("files larger than {threshold_mib} MiB")
            }
            Self::Word { needle, .. } => {
                format!("filenames containing {needle:?} (case-insensitive)")
            }
            Self::Extension { extension, .. } => {
                format!("files with extension .{extension} (case-insensitive)")
            }
            Self::OlderThan { days } => format!("files older than {days} days"),
            Self::NewerThan { days } => format!("files modified within {days} days"),
            Self::Empty => "empty files".to_owned(),
        }
    }

    pub fn has_size_sort(&self) -> bool {
        matches!(self, Self::SizeGreaterThan { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundQuery {
    filters: Vec<Filter>,
}

impl CompoundQuery {
    pub fn new(primary: Filter) -> Self {
        Self {
            filters: vec![primary],
        }
    }

    pub fn push(&mut self, filter: Filter) {
        self.filters.push(filter);
    }

    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    pub fn path_candidate(&self, path: &Path) -> bool {
        self.filters
            .iter()
            .all(|filter| filter.path_candidate(path))
    }

    pub fn metadata_matches(&self, metadata: &Metadata, now: SystemTime) -> io::Result<bool> {
        for filter in &self.filters {
            if !filter.metadata_matches(metadata, now)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn description(&self) -> String {
        self.filters
            .iter()
            .map(Filter::description)
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    pub fn sorts_by_size(&self) -> bool {
        self.filters.iter().any(Filter::has_size_sort)
    }
}

fn validate_days(days: u64) -> anyhow::Result<()> {
    days.checked_mul(SECONDS_PER_DAY)
        .ok_or_else(|| anyhow::anyhow!("day threshold is too large"))?;
    Ok(())
}

fn modified_is_older(modified: SystemTime, now: SystemTime, days: u64) -> io::Result<bool> {
    Ok(modified < cutoff(now, days)?)
}

fn modified_is_newer(modified: SystemTime, now: SystemTime, days: u64) -> io::Result<bool> {
    Ok(modified >= cutoff(now, days)?)
}

fn cutoff(now: SystemTime, days: u64) -> io::Result<SystemTime> {
    let seconds = days
        .checked_mul(SECONDS_PER_DAY)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "day threshold is too large"))?;
    now.checked_sub(Duration::from_secs(seconds))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "day threshold is too large"))
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn size_is_strictly_greater_than_threshold() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("exact.bin");
        let file = fs::File::create(&path).unwrap();
        file.set_len(1024 * 1024).unwrap();
        let metadata = fs::metadata(&path).unwrap();

        let query = CompoundQuery::new(Filter::size_greater_than_mib(1).unwrap());
        assert!(
            !query
                .metadata_matches(&metadata, SystemTime::now())
                .unwrap()
        );
    }

    #[test]
    fn size_threshold_overflow_is_rejected() {
        let error = Filter::size_greater_than_mib(u64::MAX).unwrap_err();
        assert!(error.to_string().contains("size threshold is too large"));
    }

    #[test]
    fn word_match_is_case_insensitive_and_filename_only() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("camera-folder");
        fs::create_dir(&nested).unwrap();
        let path = nested.join("FrontCamera.JSON");
        let mut file = fs::File::create(&path).unwrap();
        writeln!(file, "nothing relevant").unwrap();

        let query = CompoundQuery::new(Filter::word("camera".to_owned()).unwrap());
        assert!(query.path_candidate(&path));

        let other = nested.join("config.json");
        fs::write(&other, "camera appears only in file contents").unwrap();
        assert!(!query.path_candidate(&other));
    }

    #[test]
    fn extension_normalizes_leading_dot_and_case() {
        let query = CompoundQuery::new(Filter::extension(".MP4".to_owned()).unwrap());
        assert!(query.path_candidate(Path::new("clip.mp4")));
        assert!(query.path_candidate(Path::new("CLIP.Mp4")));
        assert!(!query.path_candidate(Path::new("clip.mp3")));
    }

    #[test]
    fn compound_query_requires_every_filter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("camera.mp4");
        let file = fs::File::create(&path).unwrap();
        file.set_len(2 * 1024 * 1024).unwrap();
        let metadata = fs::metadata(&path).unwrap();

        let mut query = CompoundQuery::new(Filter::word("camera".into()).unwrap());
        query.push(Filter::extension("mp4".into()).unwrap());
        query.push(Filter::size_greater_than_mib(1).unwrap());

        assert!(query.path_candidate(&path));
        assert!(
            query
                .metadata_matches(&metadata, SystemTime::now())
                .unwrap()
        );
    }

    #[test]
    fn older_and_newer_use_the_same_run_reference_time() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("fresh.txt");
        fs::write(&path, "x").unwrap();
        let metadata = fs::metadata(path).unwrap();
        let modified = metadata.modified().unwrap();
        let two_days_later = modified + Duration::from_secs(2 * SECONDS_PER_DAY);

        let older = CompoundQuery::new(Filter::older_than_days(1).unwrap());
        let newer = CompoundQuery::new(Filter::newer_than_days(3).unwrap());

        assert!(older.metadata_matches(&metadata, two_days_later).unwrap());
        assert!(newer.metadata_matches(&metadata, two_days_later).unwrap());
    }

    #[test]
    fn day_threshold_overflow_is_rejected() {
        let error = Filter::older_than_days(u64::MAX).unwrap_err();
        assert!(error.to_string().contains("day threshold is too large"));
    }

    #[test]
    fn age_boundary_is_complementary_and_exact() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10 * SECONDS_PER_DAY);
        let boundary = now - Duration::from_secs(SECONDS_PER_DAY);
        let before = boundary - Duration::from_secs(1);
        let after = boundary + Duration::from_secs(1);

        assert!(!modified_is_older(boundary, now, 1).unwrap());
        assert!(modified_is_newer(boundary, now, 1).unwrap());
        assert!(modified_is_older(before, now, 1).unwrap());
        assert!(!modified_is_newer(before, now, 1).unwrap());
        assert!(!modified_is_older(after, now, 1).unwrap());
        assert!(modified_is_newer(after, now, 1).unwrap());
    }

    #[test]
    fn empty_filter_uses_zero_byte_size() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        fs::write(&path, []).unwrap();
        let metadata = fs::metadata(path).unwrap();
        let query = CompoundQuery::new(Filter::Empty);
        assert!(
            query
                .metadata_matches(&metadata, SystemTime::now())
                .unwrap()
        );
    }
}
