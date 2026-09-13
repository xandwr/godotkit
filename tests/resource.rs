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
        json!({"class":"StandardMaterial3D","properties":{"metallic":0.5,"resource_name":"Native","next_pass":null}}),
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
            "validate",
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

#[test]
fn schema_requires_exactly_one_type_selector() {
    for arguments in [
        vec![],
        vec!["--class", "Resource", "--script", "res://test.gd"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .args(["resource", "schema"])
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn discovers_instance_defaults_hints_and_creation_support() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-schema-test-{}-{}",
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
    fs::write(directory.join("fields.gd"), "extends \"res://base.gd\"\n@export_group(\"Details\")\n@export_enum(\"Off:0\", \"On:4\", \"Auto\") var mode: int = 4\n@export_enum(\"Small\", \"Large\") var size: String = \"Large\"\n@export_range(0.0, 1.0, 0.05) var ratio: float = 0.5\n@export var enabled: bool = true\n@export var offset: Vector3 = Vector3(1, 2, 3)\n@export var items: Array[int] = [1, 2]\n@export var child: Resource = Resource.new()\n@export var target: Resource\n@export var large: int = -9223372036854775808\nvar transient: int = 8\n@export var from_constructor: int = 0\nfunc _init():\n\tfrom_constructor = 42\n").unwrap();
    fs::write(
        directory.join("required.gd"),
        "extends Resource\nfunc _init(required: int):\n\tresource_name = str(required)\n",
    )
    .unwrap();
    let schema = |selector: &str, name: &str, format: &str| {
        Command::new(env!("CARGO_BIN_EXE_gdkit"))
            .current_dir(&directory)
            .env_remove("GDKIT_GODOT")
            .args([
                "resource", "schema", selector, name, "--output", format, "--godot",
            ])
            .arg(&engine)
            .output()
            .unwrap()
    };
    let native = schema("--class", "StandardMaterial3D", "json");
    assert!(
        native.status.success(),
        "{} {}",
        String::from_utf8_lossy(&native.stdout),
        String::from_utf8_lossy(&native.stderr)
    );
    let native: Value = serde_json::from_slice(&native.stdout).unwrap();
    assert_eq!(native["executes_constructors_and_getters"], true);
    let metallic = native["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "metallic")
        .unwrap();
    assert_eq!(metallic["default"], json!({"encoding":"json","value":0.0}));
    assert_eq!(metallic["create_supported"], true);
    let discovery = schema("--script", "res://fields.gd", "json");
    assert!(
        discovery.status.success(),
        "{} {}",
        String::from_utf8_lossy(&discovery.stdout),
        String::from_utf8_lossy(&discovery.stderr)
    );
    let discovery: Value = serde_json::from_slice(&discovery.stdout).unwrap();
    assert_eq!(discovery["status"], "schema");
    assert_eq!(discovery["executes_constructors_and_getters"], true);
    let fields = discovery["fields"].as_array().unwrap();
    let field = |name: &str| fields.iter().find(|field| field["name"] == name).unwrap();
    assert!(!fields.iter().any(|field| field["name"] == "Details"));
    assert_eq!(field("inherited")["default"]["value"], 7);
    assert_eq!(field("from_constructor")["default"]["value"], 42);
    assert_eq!(
        field("mode")["enum_choices"],
        json!([{"name":"Off","value":0},{"name":"On","value":4},{"name":"Auto","value":5}])
    );
    assert_eq!(
        field("size")["enum_choices"],
        json!([{"name":"Small","value":"Small"},{"name":"Large","value":"Large"}])
    );
    assert!(
        field("ratio")["hint_string"]
            .as_str()
            .unwrap()
            .contains("0.05")
    );
    assert_eq!(field("offset")["default"]["encoding"], "godot");
    assert_eq!(field("items")["default"]["encoding"], "godot");
    assert_eq!(field("large")["default"]["encoding"], "godot");
    assert_eq!(field("target")["default"]["value"], Value::Null);
    assert_eq!(field("target")["class_name"], "Resource");
    assert_eq!(field("target")["create_supported"], true);
    assert_eq!(
        field("target")["accepted_inputs"],
        json!(["null", "$ref", "$resource"])
    );
    assert_eq!(
        field("target")["resource_constraints"],
        json!([{"class":"Resource", "script":null}])
    );
    assert_eq!(
        field("child")["default"],
        json!({"encoding":"resource", "value":{"path":"", "type":"Resource", "script":null}})
    );
    assert_eq!(field("transient")["storage"], false);
    assert_eq!(field("inherited")["storage"], true);
    assert_eq!(field("inherited")["editor_visible"], true);
    for name in ["offset", "items", "script", "resource_path", "transient"] {
        assert_eq!(field(name)["create_supported"], false, "{name}");
        assert!(
            !field(name)["unsupported_reason"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }
    let properties: serde_json::Map<String, Value> = fields
        .iter()
        .filter(|field| field["create_supported"] == true && field["default"]["encoding"] == "json")
        .map(|field| {
            (
                field["name"].as_str().unwrap().to_owned(),
                field["default"]["value"].clone(),
            )
        })
        .collect();
    fs::write(
        directory.join("spec.json"),
        serde_json::to_vec(&json!({"script":"res://fields.gd","properties":properties})).unwrap(),
    )
    .unwrap();
    let created = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "create",
            "--spec",
            "spec.json",
            "--out",
            "res://generated.tres",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{} {}",
        String::from_utf8_lossy(&created.stdout),
        String::from_utf8_lossy(&created.stderr)
    );
    let human = schema("--script", "res://fields.gd", "human");
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("executes constructors and getters"));
    assert!(human.contains("inherited: int"));
    for (selector, name) in [
        ("--class", "Node"),
        ("--script", "res://required.gd"),
        ("--script", "res://../escape.gd"),
        ("--class", "MissingClass"),
    ] {
        let failed = schema(selector, name, "json");
        assert!(!failed.status.success());
        let failed: Value = serde_json::from_slice(&failed.stdout).unwrap();
        assert_eq!(failed["status"], "error");
    }
    assert!(!fs::read_dir(&directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gdkit-resource-")
    }));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires GDKIT_TEST_GODOT pointing to a Godot 4 editor"]
fn creates_nested_resources_and_verifies_external_references() {
    let engine = std::env::var_os("GDKIT_TEST_GODOT").expect("set GDKIT_TEST_GODOT");
    let directory = std::env::temp_dir().join(format!(
        "gdkit-nested-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
    fs::write(directory.join("stats.gd"), "class_name NestedStats extends Resource\n@export var damage: int = 3\n@export var child: Resource\n").unwrap();
    fs::write(
        directory.join("derived.gd"),
        "extends NestedStats\n@export var bonus: int = 1\n",
    )
    .unwrap();
    fs::write(directory.join("weapon.gd"), "extends Resource\n@export var stats: NestedStats\n@export var icon: Texture2D\n@export var any: Resource\n@export var effects: Array[NestedStats] = []\n@export var resources: Array[Resource] = []\n@export var textures: Array[Texture2D] = []\n").unwrap();
    fs::write(directory.join("mutator.gd"), "extends Resource\n@export var stats: NestedStats:\n\tset(value):\n\t\tstats = value\n\t\tif stats != null:\n\t\t\tstats.damage = 99\n").unwrap();
    fs::write(
        directory.join("cycle.gd"),
        "extends Resource\n@export var loop: Resource\nfunc _init():\n\tloop = self\n",
    )
    .unwrap();
    fs::write(directory.join("reload_child.gd"), "extends Resource\n@export var damage: int = 0:\n\tget:\n\t\treturn damage if resource_path.is_empty() else damage + 1\n").unwrap();
    fs::write(directory.join("external.tres"), "[gd_resource type=\"Resource\" script_class=\"NestedStats\" load_steps=2 format=3]\n[ext_resource type=\"Script\" path=\"res://stats.gd\" id=\"1\"]\n[resource]\nscript = ExtResource(\"1\")\ndamage = 15\n").unwrap();
    fs::write(directory.join("icon.svg"), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"><rect width=\"4\" height=\"4\" fill=\"red\"/></svg>").unwrap();
    fs::write(directory.join("anonymous.gd"), "extends Resource\nconst Stats = preload(\"res://derived.gd\")\n@export var effects: Array[Stats] = []\n").unwrap();
    let import = Command::new(&engine)
        .args(["--headless", "--editor", "--path"])
        .arg(&directory)
        .args(["--import"])
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    assert!(!String::from_utf8_lossy(&import.stderr).contains("SCRIPT ERROR:"));
    let run = |spec: Value, out: &str| {
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
                out,
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
    let external = fs::read(directory.join("external.tres")).unwrap();
    let icon = fs::read(directory.join("icon.svg")).unwrap();
    let (ok, native) = run(
        json!({"class":"StandardMaterial3D", "properties":{"next_pass":{"$resource":{"class":"StandardMaterial3D", "properties":{"metallic":0.5}}}}}),
        "res://material.tres",
    );
    assert!(ok, "{native}");
    let (ok, result) = run(
        json!({"script":"res://weapon.gd", "properties": {
            "stats":{"$resource":{"script":"res://derived.gd", "properties":{"damage":20,"bonus":4,"child":{"$resource":{"class":"Resource", "properties":{"resource_name":"Inner"}}}}}},
            "icon":{"$ref":"res://icon.svg"}, "any":{"$ref":"res://external.tres"}
        }}),
        "res://nested.tres",
    );
    assert!(ok, "{result}");
    assert_eq!(
        result["properties"]["stats"]["$resource"]["properties"]["damage"],
        20
    );
    let text = fs::read_to_string(directory.join("nested.tres")).unwrap();
    assert!(text.contains("[sub_resource"));
    assert!(text.contains("res://external.tres"));
    assert!(text.contains("res://icon.svg"));
    assert!(!text.contains(".gdkit-resource-"));
    let (ok, result) = run(
        json!({"script":"res://weapon.gd", "properties":{"stats":{"$ref":"res://external.tres"},"icon":null,"any":null}}),
        "res://references.tres",
    );
    assert!(ok, "{result}");
    for script in ["res://weapon.gd", "res://anonymous.gd"] {
        let (ok, result) = run(
            json!({"script":script,"properties":{"effects":[null,{"$ref":"res://external.tres"},{"$resource":{"script":"res://derived.gd","properties":{"damage":21}}},{"$resource":{"script":"res://derived.gd","properties":{"damage":22}}}]}}),
            if script.contains("anonymous") {
                "res://anonymous_array.tres"
            } else {
                "res://arrays.tres"
            },
        );
        if script.contains("anonymous") {
            assert!(!ok, "{result}");
            assert_eq!(result["field"], "properties.effects[1]");
        } else {
            assert!(ok, "{result}");
        }
        let (ok, result) = run(
            json!({"script":script,"properties":{"effects":[]}}),
            if script.contains("anonymous") {
                "res://anonymous_empty.tres"
            } else {
                "res://empty.tres"
            },
        );
        assert!(ok, "{result}");
    }
    let (ok, result) = run(
        json!({"script":"res://weapon.gd","properties":{"resources":[null,{"$resource":{"class":"Resource","properties":{}}},{"$ref":"res://external.tres"}],"textures":[{"$ref":"res://icon.svg"}]}}),
        "res://native_arrays.tres",
    );
    assert!(ok, "{result}");
    let (ok, result) = run(
        json!({"script":"res://anonymous.gd","properties":{"effects":[{"$resource":{"script":"res://derived.gd","properties":{"damage":23}}}]}}),
        "res://anonymous_valid.tres",
    );
    assert!(ok, "{result}");
    fs::write(directory.join("verify_arrays.gd"), "extends SceneTree\nfunc _initialize():\n\tvar graph = load(\"res://arrays.tres\")\n\tassert(graph.effects.size() == 4)\n\tassert(graph.effects[0] == null)\n\tassert(graph.effects[1].resource_path == \"res://external.tres\")\n\tassert(graph.effects[2].damage == 21 and graph.effects[3].damage == 22)\n\tassert(graph.effects[2] != graph.effects[3])\n\tvar empty = load(\"res://empty.tres\").effects\n\tassert(empty.is_empty() and empty.get_typed_script() == load(\"res://stats.gd\"))\n\tvar anonymous = load(\"res://anonymous_empty.tres\").effects\n\tassert(anonymous.is_empty() and anonymous.get_typed_script() == load(\"res://derived.gd\"))\n\tquit()\n").unwrap();
    let verification = Command::new(&engine)
        .args(["--headless", "--path"])
        .arg(&directory)
        .args(["--script", "res://verify_arrays.gd"])
        .output()
        .unwrap();
    assert!(verification.status.success());
    assert!(
        !String::from_utf8_lossy(&verification.stderr).contains("SCRIPT ERROR:"),
        "{}",
        String::from_utf8_lossy(&verification.stderr)
    );
    let schema = Command::new(env!("CARGO_BIN_EXE_gdkit"))
        .current_dir(&directory)
        .env_remove("GDKIT_GODOT")
        .args([
            "resource",
            "schema",
            "--script",
            "res://weapon.gd",
            "--output",
            "json",
            "--godot",
        ])
        .arg(&engine)
        .output()
        .unwrap();
    assert!(schema.status.success());
    let schema: Value = serde_json::from_slice(&schema.stdout).unwrap();
    let stats = schema["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "stats")
        .unwrap();
    assert_eq!(stats["create_supported"], true);
    let effects = schema["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "effects")
        .unwrap();
    assert_eq!(effects["create_supported"], true);
    assert_eq!(effects["accepted_inputs"], json!(["array"]));
    assert_eq!(
        effects["element_accepted_inputs"],
        json!(["null", "$ref", "$resource"])
    );
    assert_eq!(
        effects["element_constraints"][0]["script"],
        "res://stats.gd"
    );

    assert_eq!(
        stats["resource_constraints"],
        json!([{"class":"NestedStats", "script":"res://stats.gd"}])
    );
    for (spec, field, stage) in [
        (
            json!({"script":"res://weapon.gd","properties":{"resources":[{"$resource":{"script":"res://reload_child.gd","properties":{"damage":5}}}]}}),
            "properties.resources[0].properties.damage",
            "verify",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[{"$ref":"res://missing.tres"}]}}),
            "properties.effects[0]",
            "load",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[null,{"$resource":{"class":"Resource","properties":{}}}]}}),
            "properties.effects[1]",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[null,{"$resource":{"script":"res://stats.gd","properties":{"damage":"bad"}}}]}}),
            "properties.effects[1].properties.damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":[1]}}),
            "effects[0]",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"effects":null}}),
            "effects",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"stats":{"$resource":{"class":"Resource","properties":{}}}}}),
            "stats",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"icon":{"$ref":"res://external.tres"}}}),
            "icon",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$ref":"res://missing.tres"}}}),
            "any",
            "load",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$ref":"res://../escape.tres"}}}),
            "any",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$ref":"res://external.tres","extra":1}}}),
            "any",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"stats":{"$resource":{"script":"res://stats.gd","properties":{"damage":"bad"}}}}}),
            "properties.stats.properties.damage",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"stats":{"$resource":{"script":"res://stats.gd","properties":{"damage":9007199254740992_u64}}}}}),
            "properties.stats.properties.damage",
            "validate",
        ),
        (
            json!({"script":"res://mutator.gd","properties":{"stats":{"$ref":"res://external.tres"}}}),
            "properties.stats.properties.damage",
            "assign",
        ),
        (
            json!({"script":"res://mutator.gd","properties":{"stats":{"$resource":{"script":"res://stats.gd","properties":{"damage":20}}}}}),
            "properties.stats.properties.damage",
            "assign",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$resource":{"script":"res://reload_child.gd","properties":{"damage":5}}}}}),
            "properties.any.properties.damage",
            "verify",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":{"$resource":{"script":"res://cycle.gd","properties":{}}}}}),
            "properties.any.properties.loop",
            "validate",
        ),
        (
            json!({"script":"res://weapon.gd","properties":{"any":[]}}),
            "any",
            "validate",
        ),
    ] {
        let (ok, result) = run(spec, "res://failure.tres");
        assert!(!ok, "{result}");
        assert_eq!(result["field"], field, "{result}");
        assert_eq!(result["stage"], stage, "{result}");
        assert!(!directory.join("failure.tres").exists());
        assert_eq!(fs::read(directory.join("external.tres")).unwrap(), external);
        assert_eq!(fs::read(directory.join("icon.svg")).unwrap(), icon);
    }
    let mut deep = json!({"script":"res://stats.gd","properties":{}});
    for _ in 0..17 {
        deep = json!({"script":"res://stats.gd","properties":{"child":{"$resource":deep}}});
    }
    let (ok, result) = run(deep, "res://deep.tres");
    assert!(!ok, "{result}");
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("nesting exceeds")
    );
    assert!(!directory.join("deep.tres").exists());
    assert!(!fs::read_dir(&directory).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gdkit-resource-")
    }));
    fs::remove_dir_all(directory).unwrap();
}
