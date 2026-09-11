use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Project(PathBuf);

impl Project {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("gdkit-cache-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("project.godot"), "config_version=5\n").unwrap();
        Self(path)
    }

    fn run(&self, command: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .args(command)
            .arg(&self.0)
            .env_remove("GDKIT_GODOT")
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot editor"]
fn refresh_persists_uid_mapping_after_move() {
    let project = Project::new("refresh");
    let engine = std::env::var("GDKIT_TEST_GODOT").unwrap();
    fs::write(
        project.0.join("actor.gd"),
        "class_name CacheActor\nextends RefCounted\n",
    )
    .unwrap();
    let first = project.run(&["cache", "refresh", "--godot", &engine]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let uid = fs::read_to_string(project.0.join("actor.gd.uid")).unwrap();
    fs::rename(project.0.join("actor.gd"), project.0.join("moved.gd")).unwrap();
    fs::rename(
        project.0.join("actor.gd.uid"),
        project.0.join("moved.gd.uid"),
    )
    .unwrap();
    let second = project.run(&["import", "--godot", &engine]);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        fs::read_to_string(project.0.join("moved.gd.uid")).unwrap(),
        uid
    );
    fs::write(project.0.join("verify.gd"), format!("extends SceneTree\nfunc _init():\n\tvar id = ResourceUID.text_to_id(\"{}\")\n\tquit(0 if ResourceUID.get_id_path(id) == \"res://moved.gd\" else 1)\n", uid.trim())).unwrap();
    let verified = Command::new(engine)
        .args(["--headless", "--path"])
        .arg(&project.0)
        .args(["--script", "res://verify.gd"])
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
}
