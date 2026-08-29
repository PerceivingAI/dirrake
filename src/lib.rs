mod agent_json;
pub mod analyze;
pub mod capabilities;
pub mod cli;
pub mod error;
pub mod model;
pub mod output;
pub mod pathutil;
pub mod query;
pub mod scan;
pub mod walker;
pub(crate) mod warnings;

use std::path::Path;

use crate::{cli::Cli, error::AppResult, model::Request, output::Report};

pub fn run(cli: Cli, cwd: &Path) -> AppResult<()> {
    let request = cli.into_request(cwd)?;
    match request {
        Request::Files(request) => {
            let mode = request.options.output;
            let report_dir = request.options.report_dir.clone();
            let report = Report::Files(scan::scan_files(&request));
            output::emit(&report, mode, &report_dir)
        }
        Request::Dirs(request) => {
            let mode = request.options.output;
            let report_dir = request.options.report_dir.clone();
            let report = Report::Directories(analyze::analyze_dirs(&request));
            output::emit(&report, mode, &report_dir)
        }
        Request::Info(request) => {
            let mode = request.options.output;
            let report_dir = request.options.report_dir.clone();
            let report = Report::Info(analyze::analyze_info(&request));
            output::emit(&report, mode, &report_dir)
        }
        Request::Capabilities(request) => {
            let report = Report::Capabilities(capabilities::report());
            output::emit(&report, request.output, &request.report_dir)
        }
    }
}
