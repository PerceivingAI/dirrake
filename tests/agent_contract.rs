use std::{fs, process::Command};

use serde_json::Value;
use tempfile::tempdir;

fn dirrake() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dirrake"))
}

fn json_stdout(mut command: Command) -> (std::process::Output, Value) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = serde_json::from_slice(&output.stdout).unwrap();
    (output, value)
}

#[test]
fn capabilities_json_is_self_describing() {
    let mut command = dirrake();
    command.args(["capabilities", "json"]);
    let (output, value) = json_stdout(command);

    assert!(output.stderr.is_empty());
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["type"], "capabilities");
    assert_eq!(value["tool"], "dirrake");
    assert_eq!(value["read_only"], true);
    assert_eq!(value["parallel"], true);
    assert_eq!(value["broken_pipe_is_success"], true);
    assert!(
        value["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "jsonl")
    );
    assert!(
        value["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "info")
    );
    assert!(
        value["exit_codes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["code"] == 3)
    );
}

#[test]
fn bounded_json_reports_total_and_returned_matches_separately() {
    let target = tempdir().unwrap();
    for name in ["camera-c.txt", "camera-a.txt", "camera-b.txt"] {
        fs::write(target.path().join(name), "x").unwrap();
    }

    let mut command = dirrake();
    command
        .args(["word", "camera"])
        .arg(target.path())
        .args(["limit", "2", "relative", "json"]);
    let (output, value) = json_stdout(command);

    assert!(output.stderr.is_empty());
    assert_eq!(value["type"], "file_results");
    assert_eq!(value["controls"]["path_mode"], "relative");
    assert_eq!(value["stats"]["matches_total"], 3);
    assert_eq!(value["stats"]["matches_returned"], 2);
    assert_eq!(value["stats"]["truncated"], true);
    assert_eq!(value["results"][0]["path"], "camera-a.txt");
    assert_eq!(value["results"][1]["path"], "camera-b.txt");
}

#[test]
fn jsonl_has_meta_rows_and_final_summary() {
    let target = tempdir().unwrap();
    fs::write(target.path().join("camera-a.txt"), "x").unwrap();
    fs::write(target.path().join("camera-b.txt"), "x").unwrap();

    let output = dirrake()
        .args(["word", "camera"])
        .arg(target.path())
        .args(["relative", "jsonl"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let lines: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["type"], "meta");
    assert_eq!(lines[1]["type"], "match");
    assert_eq!(lines[2]["type"], "match");
    assert_eq!(lines[3]["type"], "summary");
    assert_eq!(lines[1]["result"]["path"], "camera-a.txt");
    assert_eq!(lines[3]["stats"]["matches_total"], 2);
}

#[test]
fn dirs_json_is_machine_readable() {
    let target = tempdir().unwrap();
    let nested = target.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("file.bin"), vec![0_u8; 12]).unwrap();

    let mut command = dirrake();
    command
        .arg("dirs")
        .arg(target.path())
        .args(["relative", "json"]);
    let (_, value) = json_stdout(command);

    assert_eq!(value["type"], "directory_results");
    assert_eq!(value["results"][0]["path"], "nested");
    assert_eq!(value["results"][0]["size_bytes"], 12);
    assert_eq!(value["results"][0]["file_count"], 1);
}

#[test]
fn info_json_exposes_scan_observability() {
    let target = tempdir().unwrap();
    fs::write(target.path().join("clip.mp4"), vec![0_u8; 20]).unwrap();
    fs::write(target.path().join("notes.txt"), vec![0_u8; 5]).unwrap();

    let mut command = dirrake();
    command
        .arg("info")
        .arg(target.path())
        .args(["relative", "json"]);
    let (_, value) = json_stdout(command);

    assert_eq!(value["type"], "info");
    assert_eq!(value["stats"]["files_seen"], 2);
    assert_eq!(value["stats"]["total_file_bytes"], 25);
    assert_eq!(value["warnings"]["count"], 0);
    assert!(value["stats"]["elapsed_ms"].is_number());
    assert_eq!(value["largest_file"]["path"], "clip.mp4");
}

#[test]
fn help_is_sufficient_for_cold_discovery() {
    let output = dirrake().arg("--help").output().unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for required in [
        "size",
        "word",
        "ext",
        "older",
        "newer",
        "empty",
        "top",
        "dirs",
        "info",
        "capabilities",
        "Common modifiers",
        "limit N",
        "depth N",
        "relative",
        "jsonl",
        "and",
    ] {
        assert!(help.contains(required), "missing help text: {required}");
    }
}

#[test]
fn subcommand_help_explains_agent_controls() {
    let size = dirrake().args(["size", "--help"]).output().unwrap();
    assert!(size.status.success());
    let size_help = String::from_utf8(size.stdout).unwrap();
    assert!(size_help.contains("and ext <EXT>"));
    assert!(size_help.contains("limit N"));
    assert!(size_help.contains("relative"));
    assert!(size_help.contains("jsonl"));

    let info = dirrake().args(["info", "--help"]).output().unwrap();
    assert!(info.status.success());
    let info_help = String::from_utf8(info.stdout).unwrap();
    assert!(info_help.contains("limit N"));
    assert!(info_help.contains("depth N"));
}

#[test]
fn version_is_available() {
    let output = dirrake().arg("--version").output().unwrap();
    assert!(output.status.success());
    let version = String::from_utf8(output.stdout).unwrap();
    assert!(version.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn invalid_root_uses_exit_code_three() {
    let target = tempdir().unwrap();
    let file = target.path().join("file.txt");
    fs::write(&file, "x").unwrap();

    let output = dirrake()
        .args(["word", "camera"])
        .arg(file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("is not a directory")
    );
}

#[test]
fn semantic_usage_error_uses_exit_code_two() {
    let output = dirrake()
        .args(["top", "10", "limit", "2"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("does not accept `limit`")
    );
}

#[test]
fn clap_usage_error_also_uses_exit_code_two() {
    let output = dirrake().args(["size", "not-a-number"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}
