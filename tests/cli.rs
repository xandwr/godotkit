use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(args: &[&str], source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_godotkit"))
        .arg("format")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdout_and_check_have_distinct_exit_statuses() {
    let source = "func f():\n    if ready:\n        return\n";
    let output = run(&[], source);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"func f():\n    if ready: return\n");
    assert!(output.stderr.is_empty());
    let check = run(&["--check"], source);
    assert_eq!(check.status.code(), Some(1));
    assert!(check.stdout.is_empty());
    assert!(
        run(&["--check"], "func f():\n    if ready: return\n")
            .status
            .success()
    );
    assert_eq!(run(&[], "var x = [").status.code(), Some(2));
    assert_eq!(run(&["--write"], source).status.code(), Some(2));
    assert_eq!(run(&["--line-width", "0"], source).status.code(), Some(2));
}

#[test]
fn writes_only_when_requested_and_preserves_failed_input() {
    let directory = std::env::temp_dir().join(format!("godotkit-test-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("guard.gd");
    let path_arg = path.to_str().unwrap();
    let source = "func f():\n    if ready:\n        return\n";
    fs::write(&path, source).unwrap();
    assert!(run(&[path_arg], "").status.success());
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    assert_eq!(run(&[path_arg, "--check"], "").status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    assert!(run(&[path_arg, "--write"], "").status.success());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "func f():\n    if ready: return\n"
    );
    for invalid in [
        "var x = [",
        "func f():\n    if ready:\n        return\n    var value =\n",
    ] {
        fs::write(&path, invalid).unwrap();
        for mode in ["--write", "--check"] {
            let output = run(&[path_arg, mode], "");
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("at byte"));
            assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        }
    }
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn writes_preserve_permissions_and_refuse_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory =
        std::env::temp_dir().join(format!("godotkit-permissions-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("guard.gd");
    let link = directory.join("link.gd");
    let source = "func f():\n    if ready:\n        return\n";
    fs::write(&path, source).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&path, &link).unwrap();
    assert_eq!(
        run(&[link.to_str().unwrap(), "--write"], "").status.code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), source);
    assert!(
        run(&[path.to_str().unwrap(), "--write"], "")
            .status
            .success()
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(directory).unwrap();
}
