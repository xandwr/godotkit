use std::{fs, process::Command};

#[test]
#[ignore = "requires the provisioned Godot 4.7.2 editor"]
fn checks_project_scripts_scenes_and_resources() {
    let directory = std::env::temp_dir().join(format!("gdkit-check-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[application]\nconfig/name=\"gdkit check test\"\n",
    )
    .unwrap();
    fs::write(
        directory.join("player.gd"),
        "extends Node\n\nvar health: int = 100\n",
    )
    .unwrap();
    fs::write(
		directory.join("player.tscn"),
		"[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://player.gd\" id=\"1\"]\n\n[node name=\"Player\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
	)
	.unwrap();
    fs::write(
        directory.join("stats.tres"),
        "[gd_resource type=\"Resource\" format=3]\n\n[resource]\nresource_name = \"Stats\"\n",
    )
    .unwrap();

    let clean = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert_eq!(
        String::from_utf8(clean.stdout).unwrap(),
        "checked 1 scripts, 1 scenes, 1 resources\n"
    );

    fs::write(
		directory.join("broken.tscn"),
		"[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://missing.gd\" id=\"1\"]\n\n[node name=\"Broken\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
	)
	.unwrap();
    let broken = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(broken.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&broken.stderr).contains("missing.gd"));

    fs::remove_file(directory.join("broken.tscn")).unwrap();
    fs::write(
        directory.join("broken.gd"),
        "extends Node\n\nvar health: int = \"full\"\n",
    )
    .unwrap();
    let invalid_script = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(invalid_script.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid_script.stderr).contains("broken.gd"));
    fs::remove_dir_all(directory).unwrap();
}
