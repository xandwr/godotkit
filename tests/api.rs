use std::{fs, process::Command};

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn queries_inherited_native_api_and_suggests_typo_fixes() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!("gdkit-api-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("project.godot"),
        "config_version=5\n[application]\nconfig/name=\"gdkit api test\"\n",
    )
    .unwrap();
    fs::write(
        directory.join("domain.gd"),
        "class_name MatchState extends RefCounted\n\nsignal changed(peer_id: int)\nvar owner_peer_id: int = 0\n\nfunc serialize() -> Dictionary:\n\treturn {}\n",
    )
    .unwrap();
    let run = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .arg("api")
            .args(arguments)
            .arg("--godot")
            .arg(&engine)
            .output()
            .unwrap()
    };

    let method = run(&["CharacterBody3D", "move_and_slide"]);
    assert!(
        method.status.success(),
        "{}",
        String::from_utf8_lossy(&method.stderr)
    );
    let method = String::from_utf8(method.stdout).unwrap();
    assert!(method.contains("Class CharacterBody3D < PhysicsBody3D"));
    assert!(method.contains("func move_and_slide() -> bool"));
    assert!(method.contains("Engine "));

    let class = run(&["CharacterBody3D"]);
    assert!(class.status.success());
    let class = String::from_utf8(class.stdout).unwrap();
    assert!(class.contains("velocity: Vector3"));
    assert!(class.contains("signal ready() [from Node]"));
    assert!(class.contains("enum MotionMode { MOTION_MODE_GROUNDED = 0"));

    let inherited = run(&["CharacterBody3D", "move_and_collide"]);
    assert!(inherited.status.success());
    let inherited = String::from_utf8(inherited.stdout).unwrap();
    assert!(inherited.contains("-> KinematicCollision3D [from PhysicsBody3D]"));

    let search = run(&["search", "multiplayer"]);
    assert!(search.status.success());
    let search = String::from_utf8(search.stdout).unwrap();
    assert!(search.contains("class MultiplayerAPI < RefCounted"));
    assert!(search.contains("Node.multiplayer: MultiplayerAPI [property]"));

    let typo = run(&["CharacterBody3D", "move_and_slid"]);
    assert_eq!(typo.status.code(), Some(1));
    assert!(
        String::from_utf8(typo.stdout)
            .unwrap()
            .contains("Did you mean: move_and_slide")
    );

    let project_class = run(&["MatchState"]);
    assert!(project_class.status.success());
    let project_class = String::from_utf8(project_class.stdout).unwrap();
    assert!(project_class.contains("Project class MatchState < RefCounted"));
    assert!(project_class.contains("func serialize() -> Dictionary [res://domain.gd:6]"));
    assert!(project_class.contains("signal changed(peer_id: int) [res://domain.gd:3]"));

    let project_member = run(&["MatchState", "serialize"]);
    assert!(project_member.status.success());
    assert!(
        String::from_utf8(project_member.stdout)
            .unwrap()
            .contains("func serialize() -> Dictionary [res://domain.gd:6]")
    );

    let project_search = run(&["search", "serialize"]);
    assert!(project_search.status.success());
    assert!(
        String::from_utf8(project_search.stdout)
            .unwrap()
            .contains("MatchState.func serialize() -> Dictionary [res://domain.gd:6]")
    );

    fs::remove_dir_all(directory).unwrap();
}
