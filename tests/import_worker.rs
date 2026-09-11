use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Project(PathBuf);

impl Project {
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .env("NO_COLOR", "1")
            .env_remove("GDKIT_GODOT")
            .arg("check")
            .arg(&self.0)
            .arg("--godot")
            .arg(std::env::var_os("GDKIT_TEST_GODOT").unwrap())
            .arg("--timings")
            .args(args)
            .output()
            .unwrap()
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn pass(&self) -> String {
        let output = self.run(&[]);
        let diagnostics = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(output.status.success(), "{diagnostics}");
        diagnostics
    }

    fn fail(&self, expected: &str) {
        let output = self.run(&[]);
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{diagnostics}");
        assert!(diagnostics.contains(expected), "{diagnostics}");
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = self.run(&["--stop-worker"]);
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn reuses_importer_without_reusing_script_validation() {
    let project = Project(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".tools")
            .join(format!("worker-test-{}", std::process::id())),
    );
    fs::create_dir(&project.0).unwrap();
    project.write("project.godot", "config_version=5\n");
    project.write(
        "base.gd",
        "class_name WorkerBase\nextends RefCounted\n\nstatic func value() -> int:\n\treturn 1\n",
    );
    project.write(
        "caller.gd",
        "extends Node\n\nvar value: int = WorkerBase.value()\n",
    );
    project.pass();
    project.pass();
    assert!(project.pass().contains("import (warm worker)"));

    project.write(
        "base.gd",
        "class_name WorkerBase\nextends RefCounted\n\nstatic func value() -> int:\n\treturn 2\n",
    );
    assert!(project.pass().contains("import (warm worker)"));
    project.write("base.gd", "class_name WorkerBase\nextends RefCounted\n\nstatic func value() -> String:\n\treturn \"two\"\n");
    project.fail("caller.gd");
    project.fail("caller.gd");
    project.write(
        "base.gd",
        "class_name WorkerBase\nextends RefCounted\n\nstatic func value() -> int:\n\treturn 3\n",
    );
    project.pass();

    project.write("base.gd", "class_name RenamedWorkerBase\nextends RefCounted\n\nstatic func value() -> int:\n\treturn 3\n");
    project.fail("WorkerBase");
    project.write(
        "caller.gd",
        "extends Node\n\nvar value: int = RenamedWorkerBase.value()\n",
    );
    project.pass();
    fs::remove_file(project.0.join("base.gd")).unwrap();
    project.fail("RenamedWorkerBase");
    project.write("base.gd", "class_name RenamedWorkerBase\nextends RefCounted\n\nstatic func value() -> int:\n\treturn 4\n");
    project.pass();

    project.write(
        "project.godot",
        "config_version=5\n[application]\nconfig/name=\"changed\"\n",
    );
    assert!(project.pass().contains("import (worker startup)"));
    let fresh = project.run(&["--fresh"]);
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stderr)
    );
    assert!(!project.0.join(".godot/gdkit/import-worker.json").exists());

    project.write("addons/broken/plugin.cfg", "[plugin]\nname=\"Check fixture\"\ndescription=\"Check fixture\"\nauthor=\"gdkit\"\nversion=\"1\"\nscript=\"plugin.gd\"\n");
    project.write("addons/broken/plugin.gd", "@tool\nextends EditorPlugin\n\nfunc _enter_tree() -> void:\n\tpush_error(\"worker fixture startup error\")\n");
    project.write("project.godot", "config_version=5\n[editor_plugins]\nenabled=PackedStringArray(\"res://addons/broken/plugin.cfg\")\n");
    project.fail("worker fixture startup error");
    project.fail("worker fixture startup error");
    project.write("addons/broken/plugin.gd", "@tool\nextends EditorPlugin\n");
    project.pass();
    project.pass();
    assert!(project.pass().contains("import (warm worker)"));
    assert!(project.run(&["--stop-worker"]).status.success());
    assert!(project.pass().contains("import (worker startup)"));

    project.write(
        "stats.tres",
        "[gd_resource type=\"Resource\" format=3]\n[resource]\nresource_name=\"new\"\n",
    );
    assert!(project.pass().contains("import (worker startup)"));
    project.write(
        "broken/invalid.gd",
        "extends Node\nvar value: int = \"wrong\"\n",
    );
    project.fail("invalid.gd");
    project.write("broken/.gdignore", "");
    project.pass();
    fs::remove_file(project.0.join("broken/.gdignore")).unwrap();
    project.fail("invalid.gd");
    project.write("broken/invalid.gd", "extends Node\n");
    project.pass();

    let record = project.0.join(".godot/gdkit/import-worker.json");
    let before: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    let first = std::thread::scope(|scope| {
        let first = scope.spawn(|| project.run(&[]));
        let second = scope.spawn(|| project.run(&[]));
        assert!(second.join().unwrap().status.success());
        first.join().unwrap()
    });
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let after: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    assert_eq!(before["token"], after["token"]);
}
