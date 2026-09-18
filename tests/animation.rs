use std::{fs, path::Path, process::Command};

fn glb(document: &str) -> Vec<u8> {
    let mut json = document.as_bytes().to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let length = 20 + json.len();
    let mut bytes = Vec::with_capacity(length);
    bytes.extend_from_slice(b"glTF");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    bytes.extend_from_slice(&json);
    bytes
}

fn run(path: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["animation", "list"])
        .arg(path)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn lists_and_filters_packed_animations() {
    let path = std::env::temp_dir().join(format!("gdkit-animations-{}.glb", std::process::id()));
    fs::write(
        &path,
        glb(
            r#"{"asset":{"version":"2.0"},"accessors":[{"min":[0],"max":[1.25]}],"animations":[{"name":"Standing","samplers":[{"input":0}],"channels":[{"target":{"node":1,"path":"rotation"}}]},{"name":"Running","samplers":[{"input":0}],"channels":[{"target":{"node":1,"path":"rotation"}},{"target":{"node":2,"path":"translation"}}]}]}"#,
        ),
    )
    .unwrap();

    let human = run(&path, &[]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("2 animations"));
    assert!(human.contains("Standing"));
    assert!(human.contains("Running"));
    assert!(human.contains("1.250s"));

    let names = run(&path, &["--names", "--filter", "run"]);
    assert!(names.status.success());
    assert_eq!(String::from_utf8(names.stdout).unwrap(), "Running\n");

    let json = run(&path, &["--output", "json", "--filter", "stand"]);
    assert!(json.status.success());
    let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["animations"].as_array().unwrap().len(), 1);
    assert_eq!(report["animations"][0]["name"], "Standing");
    assert_eq!(report["animations"][0]["duration_seconds"], 1.25);
    assert_eq!(report["animations"][0]["target_nodes"], 1);

    fs::remove_file(path).unwrap();
}

#[test]
fn reports_malformed_glb_files_as_tooling_errors() {
    let path = std::env::temp_dir().join(format!("gdkit-broken-{}.glb", std::process::id()));
    fs::write(&path, b"not glb").unwrap();
    let output = run(&path, &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("file is too short to be a GLB")
    );
    fs::remove_file(path).unwrap();
}
