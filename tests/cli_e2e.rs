use std::{
    fs,
    io::Read,
    process::{Command, Stdio},
};

use tempfile::tempdir;

fn dirrake() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dirrake"))
}

fn assert_closed_stdout_is_success(target: &std::path::Path, output_mode: Option<&str>) {
    let mut command = dirrake();
    command.args(["word", "camera"]).arg(target).arg("relative");
    if let Some(output_mode) = output_mode {
        command.arg(output_mode);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut first_byte = [0_u8; 1];
    stdout.read_exact(&mut first_byte).unwrap();
    drop(stdout);

    let status = child.wait().unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(
        status.success(),
        "closed stdout should be success for {:?}; stderr: {stderr}",
        output_mode.unwrap_or("terminal")
    );
}

#[test]
fn closed_stdout_pipe_is_success_for_terminal_json_and_jsonl() {
    let target = tempdir().unwrap();
    for index in 0..4_000 {
        fs::write(target.path().join(format!("camera-{index:05}.txt")), []).unwrap();
    }

    for output_mode in [None, Some("json"), Some("jsonl")] {
        assert_closed_stdout_is_success(target.path(), output_mode);
    }
}

#[test]
fn original_word_command_scans_explicit_directory() {
    let cwd = tempdir().unwrap();
    let target = tempdir().unwrap();
    fs::create_dir(target.path().join("nested")).unwrap();
    fs::write(target.path().join("nested").join("FrontCamera.txt"), "x").unwrap();
    fs::write(target.path().join("other.txt"), "camera only in contents").unwrap();

    let output = dirrake()
        .current_dir(cwd.path())
        .args(["word", "camera"])
        .arg(target.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("FrontCamera.txt"));
    assert!(!stdout.contains("other.txt"));
    assert!(stdout.contains("1 file"));
}

#[test]
fn original_size_command_uses_strict_mib_threshold() {
    let target = tempdir().unwrap();
    let exact = target.path().join("exact.bin");
    let over = target.path().join("over.bin");
    fs::File::create(&exact)
        .unwrap()
        .set_len(1024 * 1024)
        .unwrap();
    fs::File::create(&over)
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();

    let output = dirrake()
        .args(["size", "1"])
        .arg(target.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("over.bin"));
    assert!(!stdout.contains("exact.bin"));
}

#[test]
fn ext_is_case_insensitive() {
    let target = tempdir().unwrap();
    fs::write(target.path().join("clip.MP4"), "x").unwrap();
    fs::write(target.path().join("clip.mp3"), "x").unwrap();

    let output = dirrake()
        .args(["ext", ".mp4"])
        .arg(target.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("clip.MP4"));
    assert!(!stdout.contains("clip.mp3"));
}

#[test]
fn empty_lists_only_zero_byte_files() {
    let target = tempdir().unwrap();
    fs::write(target.path().join("empty.txt"), []).unwrap();
    fs::write(target.path().join("full.txt"), "x").unwrap();

    let output = dirrake().arg("empty").arg(target.path()).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("empty.txt"));
    assert!(!stdout.contains("full.txt"));
}

#[test]
fn top_returns_largest_files_in_order() {
    let target = tempdir().unwrap();
    for (name, size) in [("small.bin", 10), ("large.bin", 30), ("mid.bin", 20)] {
        fs::File::create(target.path().join(name))
            .unwrap()
            .set_len(size)
            .unwrap();
    }

    let output = dirrake()
        .args(["top", "2"])
        .arg(target.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let large = stdout.find("large.bin").unwrap();
    let mid = stdout.find("mid.bin").unwrap();
    assert!(large < mid);
    assert!(!stdout.contains("small.bin"));
    assert!(stdout.contains("2 of 3 matches returned"));
}

#[test]
fn dirs_reports_recursive_directory_sizes() {
    let target = tempdir().unwrap();
    let media = target.path().join("media");
    let nested = media.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("clip.bin"), vec![0_u8; 64]).unwrap();

    let output = dirrake()
        .args(["dirs", "5"])
        .arg(target.path())
        .arg("relative")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("media"));
    assert!(stdout.contains("nested"));
    assert!(stdout.contains("64 B"));
}

#[test]
fn info_produces_a_directory_census() {
    let target = tempdir().unwrap();
    fs::write(target.path().join("a.txt"), "abc").unwrap();
    fs::write(target.path().join("b.txt"), "defg").unwrap();

    let output = dirrake().arg("info").arg(target.path()).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Files seen: 2"));
    assert!(stdout.contains("Total size: 7 B"));
    assert!(stdout.contains(".txt"));
}

#[test]
fn compound_and_filters_are_applied_together() {
    let target = tempdir().unwrap();
    fs::File::create(target.path().join("camera.mp4"))
        .unwrap()
        .set_len(2 * 1024 * 1024)
        .unwrap();
    fs::File::create(target.path().join("camera.jpg"))
        .unwrap()
        .set_len(2 * 1024 * 1024)
        .unwrap();
    fs::write(target.path().join("tiny-camera.mp4"), "x").unwrap();

    let output = dirrake()
        .args(["word", "camera", "and", "ext", "mp4", "and", "size", "1"])
        .arg(target.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("camera.mp4"));
    assert!(!stdout.contains("camera.jpg"));
    assert!(!stdout.contains("tiny-camera.mp4"));
}

#[test]
fn depth_limits_recursive_search() {
    let target = tempdir().unwrap();
    fs::write(target.path().join("camera-root.txt"), "x").unwrap();
    let nested = target.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("camera-deep.txt"), "x").unwrap();

    let output = dirrake()
        .args(["word", "camera"])
        .arg(target.path())
        .args(["depth", "1"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("camera-root.txt"));
    assert!(!stdout.contains("camera-deep.txt"));
}

#[test]
fn newer_finds_a_fresh_file_and_older_does_not() {
    let target = tempdir().unwrap();
    fs::write(target.path().join("fresh.txt"), "x").unwrap();

    let newer = dirrake()
        .args(["newer", "1"])
        .arg(target.path())
        .output()
        .unwrap();
    assert!(newer.status.success());
    assert!(
        String::from_utf8(newer.stdout)
            .unwrap()
            .contains("fresh.txt")
    );

    let older = dirrake()
        .args(["older", "1"])
        .arg(target.path())
        .output()
        .unwrap();
    assert!(older.status.success());
    assert!(
        !String::from_utf8(older.stdout)
            .unwrap()
            .contains("fresh.txt")
    );
}

#[test]
fn markdown_report_is_written_to_launch_directory_not_scan_root() {
    let cwd = tempdir().unwrap();
    let target = tempdir().unwrap();
    fs::write(target.path().join("camera.txt"), "x").unwrap();

    let output = dirrake()
        .current_dir(cwd.path())
        .args(["word", "camera"])
        .arg(target.path())
        .arg("md")
        .output()
        .unwrap();

    assert!(output.status.success());
    let reports: Vec<_> = fs::read_dir(cwd.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("dirrake_") && name.ends_with(".md"))
        })
        .collect();
    assert_eq!(reports.len(), 1);
    assert!(fs::read_dir(target.path()).unwrap().all(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            != Some("md")
    }));
    let report = fs::read_to_string(&reports[0]).unwrap();
    assert!(report.contains("# DirRake File Results"));
    assert!(report.contains("camera.txt"));
}

#[test]
fn omitted_path_uses_process_working_directory() {
    let cwd = tempdir().unwrap();
    fs::write(cwd.path().join("camera-local.txt"), "x").unwrap();

    let output = dirrake()
        .current_dir(cwd.path())
        .args(["word", "camera"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("camera-local.txt")
    );
}

#[test]
fn no_matches_is_a_successful_empty_result() {
    let target = tempdir().unwrap();
    let output = dirrake()
        .args(["word", "does-not-exist"])
        .arg(target.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("No files matched."));
    assert!(stdout.contains("0 files | 0 B total"));
}
