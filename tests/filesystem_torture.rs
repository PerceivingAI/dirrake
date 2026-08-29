use std::{fs, path::Path, process::Command};

#[cfg(windows)]
use std::process::Stdio;

use serde_json::Value;
use tempfile::tempdir;

fn dirrake() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dirrake"))
}

fn json_output(command: &mut Command) -> Value {
    let output = command.output().expect("DirRake process should start");
    assert!(
        output.status.success(),
        "status={:?}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON")
}

fn relative_word(root: &Path, word: &str) -> Value {
    let mut command = dirrake();
    command
        .args(["word", word])
        .arg(root)
        .args(["relative", "json"]);
    json_output(&mut command)
}

#[test]
fn unicode_emoji_and_non_latin_names_round_trip_in_json() {
    let target = tempdir().unwrap();
    let filename = "Cámara-📷-東京.JSON";
    fs::write(target.path().join(filename), "x").unwrap();

    let value = relative_word(target.path(), "cámara");
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], filename);

    let mut extension = dirrake();
    extension
        .args(["ext", "json"])
        .arg(target.path())
        .args(["relative", "json"]);
    let value = json_output(&mut extension);
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], filename);
}

#[test]
fn long_paths_beyond_legacy_windows_limit_are_scanned() {
    let target = tempdir().unwrap();
    let mut current = target.path().to_path_buf();
    let mut depth = 0_u32;
    while current.as_os_str().len() < 320 {
        current.push(format!("segment-{depth:02}-abcdefghijklmnop"));
        fs::create_dir(&current).unwrap_or_else(|error| {
            panic!(
                "failed to create long-path fixture at length {}: {error}",
                current.as_os_str().len()
            )
        });
        depth += 1;
    }
    let file = current.join("camera-long-path.txt");
    fs::write(&file, "x").unwrap();

    let value = relative_word(target.path(), "camera-long-path");
    assert_eq!(value["stats"]["matches_total"], 1);
    let returned = value["results"][0]["path"].as_str().unwrap();
    assert!(returned.ends_with("camera-long-path.txt"));
    assert!(
        returned.len() > 260,
        "fixture did not exercise a long relative path"
    );
}

#[test]
fn multi_gib_sparse_file_uses_full_width_size_accounting() {
    let target = tempdir().unwrap();
    let huge = target.path().join("huge-sparse.bin");
    let size = 5_u64 * 1024 * 1024 * 1024;
    fs::File::create(&huge).unwrap().set_len(size).unwrap();

    let mut command = dirrake();
    command
        .args(["size", "4096"])
        .arg(target.path())
        .args(["relative", "json"]);
    let value = json_output(&mut command);

    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], "huge-sparse.bin");
    assert_eq!(value["results"][0]["size_bytes"], size);
    assert_eq!(value["stats"]["matched_bytes_total"], size);
}

#[cfg(unix)]
#[test]
fn non_utf8_filename_is_ascii_searchable_and_explicitly_escaped() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let target = tempdir().unwrap();
    let name = OsString::from_vec(b"CAMERA-\xFF.RS".to_vec());
    fs::write(target.path().join(&name), "x").unwrap();

    let value = relative_word(target.path(), "camera");
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], "<non-utf8>:CAMERA-\\xFF.RS");

    let mut extension = dirrake();
    extension
        .args(["ext", "rs"])
        .arg(target.path())
        .args(["relative", "json"]);
    let value = json_output(&mut extension);
    assert_eq!(value["stats"]["matches_total"], 1);
}

#[cfg(unix)]
#[test]
fn non_utf8_scan_root_is_accepted_without_lossy_replacement() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let parent = tempdir().unwrap();
    let root_name = OsString::from_vec(b"root-\xFE".to_vec());
    let root = parent.path().join(root_name);
    fs::create_dir(&root).unwrap();
    fs::write(root.join("camera.txt"), "x").unwrap();

    let value = relative_word(&root, "camera");
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], "camera.txt");
    let rendered_root = value["root"].as_str().unwrap();
    assert!(rendered_root.starts_with("<non-utf8>:"));
    assert!(rendered_root.contains("\\xFE"));
    assert!(!rendered_root.contains('\u{FFFD}'));
}

#[cfg(unix)]
#[test]
fn json_safely_round_trips_valid_control_characters_in_names() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let target = tempdir().unwrap();
    let name = OsString::from_vec(b"camera-line\nbreak.txt".to_vec());
    fs::write(target.path().join(&name), "x").unwrap();

    let value = relative_word(target.path(), "camera");
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], "camera-line\nbreak.txt");
}

#[cfg(unix)]
#[test]
fn terminal_output_escapes_filename_control_characters() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let target = tempdir().unwrap();
    let name = OsString::from_vec(b"camera-line\nbreak.txt".to_vec());
    fs::write(target.path().join(&name), "x").unwrap();

    let output = dirrake()
        .args(["word", "camera"])
        .arg(target.path())
        .arg("relative")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("camera-line\\nbreak.txt"));
    assert!(!stdout.contains("camera-line\nbreak.txt"));
}

#[cfg(unix)]
#[test]
fn unreadable_subdirectory_becomes_warning_and_readable_tree_continues() {
    use std::os::unix::fs::PermissionsExt;

    let target = tempdir().unwrap();
    fs::write(target.path().join("camera-readable.txt"), "x").unwrap();
    let locked = target.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("camera-secret.txt"), "x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let mut command = dirrake();
    command
        .args(["word", "camera"])
        .arg(target.path())
        .args(["relative", "json"]);
    let output = command.output().expect("DirRake process should start");

    // Restore access before assertions so tempfile cleanup remains reliable even on failure.
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], "camera-readable.txt");
    assert!(value["warnings"]["count"].as_u64().unwrap() >= 1);
}

#[cfg(unix)]
#[test]
fn symlink_scan_root_is_rejected_instead_of_followed() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("camera-external.txt"), "x").unwrap();
    let root_link = parent.path().join("root-link");
    symlink(outside.path(), &root_link).unwrap();

    let output = dirrake()
        .args(["word", "camera"])
        .arg(&root_link)
        .arg("json")
        .output()
        .expect("DirRake process should start");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not follow link targets"));
}

#[cfg(unix)]
#[test]
fn symlink_loop_broken_link_and_external_directory_are_not_followed() {
    use std::os::unix::fs::symlink;

    let target = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(target.path().join("camera-local.txt"), "x").unwrap();
    fs::write(outside.path().join("camera-external.txt"), "x").unwrap();

    symlink(outside.path(), target.path().join("external-link")).unwrap();
    symlink(target.path(), target.path().join("loop-link")).unwrap();
    symlink(
        target.path().join("does-not-exist"),
        target.path().join("broken-link"),
    )
    .unwrap();

    let value = relative_word(target.path(), "camera");
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], "camera-local.txt");
}

#[cfg(windows)]
#[test]
fn windows_junction_working_directory_remains_usable() {
    let parent = tempdir().unwrap();
    let target = tempdir().unwrap();
    let cwd_link = parent.path().join("cwd-junction");
    let status = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&cwd_link)
        .arg(target.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("cmd should be available on Windows");
    assert!(
        status.success(),
        "failed to create working-directory junction fixture"
    );

    let output = dirrake()
        .current_dir(&cwd_link)
        .args(["capabilities", "json"])
        .output()
        .expect("DirRake process should start");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "capabilities");
}

#[cfg(windows)]
#[test]
fn windows_junction_scan_root_is_rejected_instead_of_followed() {
    let parent = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("camera-external.txt"), "x").unwrap();

    let root_link = parent.path().join("root-junction");
    let status = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&root_link)
        .arg(outside.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("cmd should be available on Windows");
    assert!(
        status.success(),
        "failed to create directory junction fixture"
    );

    let output = dirrake()
        .args(["word", "camera"])
        .arg(&root_link)
        .arg("json")
        .output()
        .expect("DirRake process should start");

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not follow link targets"));
}

#[cfg(windows)]
#[test]
fn windows_directory_junction_is_not_followed() {
    let target = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(target.path().join("camera-local.txt"), "x").unwrap();
    fs::write(outside.path().join("camera-external.txt"), "x").unwrap();

    let link = target.path().join("external-junction");
    let status = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&link)
        .arg(outside.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("cmd should be available on Windows");
    assert!(
        status.success(),
        "failed to create directory junction fixture"
    );

    let value = relative_word(target.path(), "camera");
    assert_eq!(value["stats"]["matches_total"], 1);
    assert_eq!(value["results"][0]["path"], "camera-local.txt");
}
