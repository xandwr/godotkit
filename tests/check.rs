use std::{fs, process::Command};

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn checks_project_scripts_scenes_and_resources() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".tools")
        .join(format!("gdkit-check-{}", std::process::id()));
    fs::create_dir_all(directory.parent().unwrap()).unwrap();
    fs::create_dir(&directory).unwrap();
    fs::create_dir(directory.join(".git")).unwrap();
    fs::create_dir(directory.join("local only")).unwrap();
    fs::write(directory.join(".gitignore"), "/local only/\nignored.tres\n").unwrap();
    fs::write(directory.join("local only/unused.gd"), "extends Node\n").unwrap();
    fs::write(
        directory.join("ignored.tres"),
        "[gd_resource type=\"Resource\" format=3]\n[resource]\n",
    )
    .unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[application]\nconfig/name=\"gdkit check test\"\n",
    )
    .unwrap();

    let initialized = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .arg("init")
        .arg("--godot")
        .arg(&engine)
        .output()
        .unwrap();
    assert!(
        initialized.status.success(),
        "{}",
        String::from_utf8_lossy(&initialized.stderr)
    );
    let config = fs::read_to_string(directory.join("gdkit.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&config).unwrap();
    let configured = parsed["engine"]["executable"].as_str().unwrap();
    assert_eq!(
        fs::canonicalize(directory.join(configured)).unwrap(),
        fs::canonicalize(&engine).unwrap()
    );
    if pathdiff::diff_paths(
        fs::canonicalize(&engine).unwrap(),
        fs::canonicalize(&directory).unwrap(),
    )
    .is_some()
    {
        assert!(std::path::Path::new(configured).is_relative());
    }
    let repeated = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .args(["init", "--godot", "missing-engine"])
        .output()
        .unwrap();
    assert_eq!(repeated.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(directory.join("gdkit.toml")).unwrap(),
        config
    );
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
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap(), "--timings"])
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );
    let timing_output = String::from_utf8_lossy(&clean.stderr);
    assert!(
        timing_output.contains("timing: engine validation (cached)"),
        "{timing_output}"
    );
    for phase in ["file scan", "import", "resource loading", "total"] {
        assert!(
            timing_output.contains(&format!("timing: {phase} ")),
            "{timing_output}"
        );
    }
    assert_eq!(
        String::from_utf8(clean.stdout).unwrap(),
        "check passed: checked 1 scripts, 1 scenes, 1 resources\n"
    );
    let colored = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("GDKIT_GODOT")
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(colored.status.success());
    assert!(!String::from_utf8_lossy(&colored.stderr).contains("timing:"));
    assert_eq!(
        String::from_utf8(colored.stdout).unwrap(),
        "\x1b[32mcheck passed: checked 1 scripts, 1 scenes, 1 resources\x1b[0m\n"
    );

    fs::write(
		directory.join("broken.tscn"),
		"[gd_scene load_steps=2 format=3]\n\n[ext_resource type=\"Script\" path=\"res://missing.gd\" id=\"1\"]\n\n[node name=\"Broken\" type=\"Node\"]\nscript = ExtResource(\"1\")\n",
	)
	.unwrap();
    let broken = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(broken.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&broken.stdout).starts_with("check failed:"));
    assert!(String::from_utf8_lossy(&broken.stderr).contains("missing.gd"));

    fs::remove_file(directory.join("broken.tscn")).unwrap();
    fs::write(
        directory.join("broken.gd"),
        "extends Node\n\nvar health: int = \"full\"\n",
    )
    .unwrap();
    let invalid_script = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(invalid_script.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&invalid_script.stdout).starts_with("\x1b[31mcheck failed:"));
    assert!(String::from_utf8_lossy(&invalid_script.stdout).ends_with("\x1b[0m\n"));
    assert!(String::from_utf8_lossy(&invalid_script.stderr).contains("broken.gd"));
    fs::write(
        directory.join("broken.gd"),
        "extends Node\n\nvar missing = preload(\"uid://daaaaaaaaaaaa\")\n",
    )
    .unwrap();
    let unresolved = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .env("NO_COLOR", "1")
        .env_remove("GDKIT_GODOT")
        .args(["check", directory.to_str().unwrap(), "--verbose"])
        .output()
        .unwrap();
    assert_eq!(unresolved.status.code(), Some(1));
    let diagnostics = String::from_utf8_lossy(&unresolved.stderr);
    assert!(
        diagnostics.contains("Godot cannot resolve this resource ID to a file."),
        "{diagnostics}"
    );
    assert!(diagnostics.contains("res://broken.gd:3:"), "{diagnostics}");
    assert!(diagnostics.contains("Resource loading full Godot output:"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn engine_configuration_errors_and_precedence() {
    let directory = std::env::temp_dir().join(format!("gdkit-config-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    let run = |args: &[&str], environment: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gdkit"));
        command
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args(args);
        if let Some(value) = environment {
            command.env("GDKIT_GODOT", value);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        String::from_utf8(output.stderr).unwrap()
    };
    assert!(run(&["check"], None).contains("gdkit init --godot"));
    assert!(run(&["init", "--godot", "missing-init"], None).contains("missing-init"));
    assert!(!directory.join("gdkit.toml").exists());
    assert!(
        run(&["init", "--godot", env!("CARGO_BIN_EXE_gdkit")], None).contains("headless editor")
    );
    assert!(!directory.join("gdkit.toml").exists());
    fs::write(
        directory.join("gdkit.toml"),
        "[engine]\nexecutable = '../missing-config'\n",
    )
    .unwrap();
    assert!(run(&["check"], None).contains("missing-config"));
    assert!(run(&["check"], Some("missing-env")).contains("missing-env"));
    assert!(
        run(
            &["check", "--godot", "missing-explicit"],
            Some("missing-env")
        )
        .contains("missing-explicit")
    );
    fs::write(directory.join("gdkit.toml"), "invalid toml [").unwrap();
    assert!(run(&["check"], None).contains("gdkit.toml"));
    fs::remove_file(directory.join("project.godot")).unwrap();
    assert!(run(&["init", "--godot", "missing-init"], None).contains("project"));
    fs::remove_dir_all(directory).unwrap();
}
