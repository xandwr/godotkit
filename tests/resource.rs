use serde_json::{Value, json};
use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn creates_verified_resources_and_preserves_destinations_on_failure() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-resource-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(
        directory.join("base.gd"),
        "extends Resource\n@export var inherited: int = 7\n",
    )
    .unwrap();
    fs::write(directory.join("weapon.gd"), "extends \"res://base.gd\"\n@export var damage: int = 3\n@export var enabled: bool = false\n@export var label: String = \"default\"\n@export var ratio: float = 0.5\nvar transient: int = 0\n@export var clamped: int = 0:\n\tset(value):\n\t\tclamped = clampi(value, 0, 10)\n").unwrap();
    fs::write(directory.join("node.gd"), "extends Node\n").unwrap();
    fs::write(directory.join("reload.gd"), "extends Resource\n@export var value: int = 0:\n\tget:\n\t\treturn value if resource_path.is_empty() else value + 1\n").unwrap();
    fs::write(
        directory.join("constructor.gd"),
        "extends Resource\nfunc _init(required: int):\n\tresource_name = str(required)\n",
    )
    .unwrap();
    let validation = Command::new(&engine)
        .args(["--headless", "--editor", "--path"])
        .arg(&directory)
        .arg("--script")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/src/resource.gd"))
        .arg("--check-only")
        .output()
        .unwrap();
    let diagnostics = String::from_utf8_lossy(&validation.stderr);
    assert!(validation.status.success(), "{diagnostics}");
    assert!(!diagnostics.contains("ERROR:"), "{diagnostics}");
    for line in diagnostics
        .lines()
        .filter(|line| line.starts_with("WARNING:"))
    {
        assert!(line == "WARNING: 1 RID of type \"Canvas\" was leaked."
            || (line.starts_with("WARNING: ") && line.ends_with(" ObjectDB instances were leaked at exit (run with `--verbose` for details).")), "{diagnostics}");
    }
    let run = |spec: Value, destination: &str| {
        fs::write(
            directory.join("spec.json"),
            serde_json::to_vec(&spec).unwrap(),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource",
                "create",
                "--spec",
                "spec.json",
                "--out",
                destination,
                "--output",
                "json",
                "--godot",
            ])
            .arg(&engine)
            .output()
            .unwrap();
        let result: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{error}: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        (output.status.success(), result)
    };
    let (ok, native) = run(
        json!({"class":"StandardMaterial3D","properties":{"metallic":0.5,"resource_name":"Native"}}),
        "res://native.tres",
    );
    assert!(ok, "{native}");
    assert_eq!(native["path"], "res://native.tres");
    let (ok, weapon) = run(
        json!({"script":"res://weapon.gd","properties":{"damage":15,"inherited":12,"enabled":true,"label":"Shotgun \"test\"\n","ratio":0.75}}),
        "res://weapon.tres",
    );
    assert!(ok, "{weapon}");
    assert_eq!(weapon["properties"]["damage"], 15);
    let serialized = fs::read_to_string(directory.join("weapon.tres")).unwrap();
    assert!(serialized.contains("res://weapon.gd"));
    assert!(!serialized.contains(".gdkit-resource-"));
    assert!(serialized.contains("inherited = 12"));
    assert!(!serialized.contains("clamped ="));
    let original = fs::read(directory.join("weapon.tres")).unwrap();
    let (ok, _) = run(
        json!({"class":"Resource","properties":{}}),
        "res://weapon.tres",
    );
    assert!(!ok);
    assert_eq!(fs::read(directory.join("weapon.tres")).unwrap(), original);
    for (spec, field, stage) in [
        (
            json!({"script":"res://weapon.gd","properties":{"damage":"bad"}}),
            "damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"damage":1.5}}),
            "damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"missing":1}}),
            "missing",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"transient":1}}),
            "transient",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"clamped":20}}),
            "clamped",
            "assign",
        ),
        (json!({"class":"Node","properties":{}}), "", "construct"),
        (
            json!({"script":"res://node.gd","properties":{}}),
            "",
            "construct",
        ),
        (
            json!({"class":"Resource","script":"res://weapon.gd","properties":{}}),
            "",
            "prepare",
        ),
        (
            json!({"class":"Resource","properties":{"nested":{}}}),
            "nested",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"damage":9007199254740992_u64}}),
            "damage",
            "validate",
        ),
        (
            json!({"script":"res://reload.gd","properties":{"value":5}}),
            "value",
            "verify",
        ),
        (
            json!({"script":"res://constructor.gd","properties":{}}),
            "",
            "construct",
        ),
        (
            json!({"class":"Resource","properties":{"resource_path":"res://other.tres"}}),
            "resource_path",
            "validate",
        ),
    ] {
        let (ok, result) = run(spec, "res://failure.tres");
        assert!(!ok, "{result}");
        assert_eq!(result["field"], field, "{result}");
        assert_eq!(result["stage"], stage, "{result}");
        assert!(!directory.join("failure.tres").exists());
    }
    let (ok, defaults) = run(
        json!({"script":"res://weapon.gd","properties":{}}),
        "res://defaults.tres",
    );
    assert!(ok, "{defaults}");
    let (ok, result) = run(
        json!({"class":"Resource","properties":{}}),
        "res://../escape.tres",
    );
    assert!(!ok, "{result}");
    assert!(!fs::read_dir(&directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gdkit-resource-")
    }));
    fs::remove_dir_all(directory).unwrap();
}
