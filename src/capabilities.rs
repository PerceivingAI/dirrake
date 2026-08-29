use serde::Serialize;

use crate::model::AGENT_SCHEMA_VERSION;

#[derive(Debug, Clone, Serialize)]
pub struct CommandCapability {
    pub name: &'static str,
    pub purpose: &'static str,
    pub syntax: &'static str,
    pub supports_and: bool,
    pub supports_limit: bool,
    pub supports_depth: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExitCodeCapability {
    pub code: u8,
    pub meaning: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilitiesReport {
    pub schema_version: u32,
    pub tool: &'static str,
    pub tool_version: &'static str,
    pub read_only: bool,
    pub recursive: bool,
    pub parallel: bool,
    pub hidden_files_included: bool,
    pub ignore_files_honored: bool,
    pub follows_symlinks: bool,
    pub broken_pipe_is_success: bool,
    pub outputs: &'static [&'static str],
    pub path_modes: &'static [&'static str],
    pub commands: Vec<CommandCapability>,
    pub exit_codes: Vec<ExitCodeCapability>,
}

pub fn report() -> CapabilitiesReport {
    CapabilitiesReport {
        schema_version: AGENT_SCHEMA_VERSION,
        tool: "dirrake",
        tool_version: env!("CARGO_PKG_VERSION"),
        read_only: true,
        recursive: true,
        parallel: true,
        hidden_files_included: true,
        ignore_files_honored: false,
        follows_symlinks: false,
        broken_pipe_is_success: true,
        outputs: &["terminal", "md", "json", "jsonl"],
        path_modes: &["absolute", "relative"],
        commands: vec![
            command(
                "size",
                "files strictly larger than N MiB",
                "dirrake size <MIB> [ARGS...]",
                true,
                true,
                true,
            ),
            command(
                "word",
                "filenames containing text, case-insensitive",
                "dirrake word <TEXT> [ARGS...]",
                true,
                true,
                true,
            ),
            command(
                "ext",
                "files with an extension, case-insensitive",
                "dirrake ext <EXT> [ARGS...]",
                true,
                true,
                true,
            ),
            command(
                "older",
                "files older than N days",
                "dirrake older <DAYS> [ARGS...]",
                true,
                true,
                true,
            ),
            command(
                "newer",
                "files modified within N days",
                "dirrake newer <DAYS> [ARGS...]",
                true,
                true,
                true,
            ),
            command(
                "empty",
                "zero-byte files",
                "dirrake empty [ARGS...]",
                true,
                true,
                true,
            ),
            command(
                "top",
                "N largest files",
                "dirrake top <N> [ARGS...]",
                false,
                false,
                true,
            ),
            command(
                "dirs",
                "recursive directory sizes",
                "dirrake dirs [N] [ARGS...]",
                false,
                true,
                true,
            ),
            command(
                "info",
                "one-pass directory-tree census",
                "dirrake info [ARGS...]",
                false,
                true,
                true,
            ),
            command(
                "capabilities",
                "machine/human discoverable interface contract",
                "dirrake capabilities [OUTPUT]",
                false,
                false,
                false,
            ),
        ],
        exit_codes: vec![
            ExitCodeCapability {
                code: 0,
                meaning: "success (including zero matches)",
            },
            ExitCodeCapability {
                code: 2,
                meaning: "invalid arguments or query",
            },
            ExitCodeCapability {
                code: 3,
                meaning: "invalid or inaccessible scan root",
            },
            ExitCodeCapability {
                code: 4,
                meaning: "output/report failure",
            },
            ExitCodeCapability {
                code: 5,
                meaning: "internal failure",
            },
        ],
    }
}

fn command(
    name: &'static str,
    purpose: &'static str,
    syntax: &'static str,
    supports_and: bool,
    supports_limit: bool,
    supports_depth: bool,
) -> CommandCapability {
    CommandCapability {
        name,
        purpose,
        syntax,
        supports_and,
        supports_limit,
        supports_depth,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_publish_stable_schema_and_exit_codes() {
        let report = report();
        assert_eq!(report.schema_version, 1);
        assert!(report.commands.iter().any(|command| command.name == "info"));
        assert!(report.exit_codes.iter().any(|code| code.code == 3));
        assert!(report.outputs.contains(&"jsonl"));
        assert!(report.broken_pipe_is_success);
    }
}
