use std::path::PathBuf;

use crate::query::CompoundQuery;

pub const AGENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Terminal,
    Markdown,
    Json,
    Jsonl,
}

impl OutputMode {
    pub fn from_token(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "terminal" => Some(Self::Terminal),
            "md" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            "jsonl" => Some(Self::Jsonl),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Markdown => "md",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathMode {
    Absolute,
    Relative,
}

impl PathMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absolute => "absolute",
            Self::Relative => "relative",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub report_dir: PathBuf,
    pub output: OutputMode,
    pub limit: Option<usize>,
    pub max_depth: Option<usize>,
    pub path_mode: PathMode,
}

#[derive(Debug, Clone)]
pub enum FileOperation {
    Filter(CompoundQuery),
    Top { count: usize },
}

impl FileOperation {
    pub fn description(&self) -> String {
        match self {
            Self::Filter(query) => query.description(),
            Self::Top { count } => format!("Top {count} largest files"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileRequest {
    pub options: ScanOptions,
    pub operation: FileOperation,
}

#[derive(Debug, Clone)]
pub struct DirsRequest {
    pub options: ScanOptions,
}

#[derive(Debug, Clone)]
pub struct InfoRequest {
    pub options: ScanOptions,
}

#[derive(Debug, Clone)]
pub struct CapabilitiesRequest {
    pub report_dir: PathBuf,
    pub output: OutputMode,
}

#[derive(Debug, Clone)]
pub enum Request {
    Files(FileRequest),
    Dirs(DirsRequest),
    Info(InfoRequest),
    Capabilities(CapabilitiesRequest),
}
