use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use serde::{Deserialize, Serialize};

use crate::{cli::ApiArgs, engine, project_files};

const RESULT_PREFIX: &str = "GDKIT_API_RESULT:";
const SEARCH_LIMIT: usize = 50;

#[derive(Deserialize, Serialize)]
struct ApiIndex {
    version: String,
    classes: Vec<ApiClass>,
    method_argument_names: bool,
    method_default_values: bool,
}

#[derive(Deserialize, Serialize)]
struct ApiClass {
    name: String,
    parent: String,
    methods: Vec<ApiMethod>,
    properties: Vec<ApiProperty>,
    signals: Vec<ApiSignal>,
    enums: Vec<ApiEnum>,
    constants: Vec<ApiConstant>,
}

#[derive(Deserialize, Serialize)]
struct ApiMethod {
    name: String,
    return_type: ApiType,
    arguments: Vec<ApiArgument>,
    defaults: Vec<String>,
    flags: u32,
}

#[derive(Deserialize, Serialize)]
struct ApiProperty {
    name: String,
    #[serde(rename = "type")]
    value_type: ApiType,
}

#[derive(Deserialize, Serialize)]
struct ApiSignal {
    name: String,
    arguments: Vec<ApiArgument>,
}

#[derive(Deserialize, Serialize)]
struct ApiEnum {
    name: String,
    values: Vec<ApiConstant>,
}

#[derive(Deserialize, Serialize)]
struct ApiConstant {
    name: String,
    value: i64,
}

#[derive(Deserialize, Serialize)]
struct ApiArgument {
    name: String,
    #[serde(rename = "type")]
    value_type: ApiType,
}

#[derive(Deserialize, Serialize)]
struct ApiType {
    #[serde(rename = "type")]
    kind: u32,
    class_name: String,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
struct CacheKey {
    engine: String,
    project_extensions: String,
    implementation: u64,
}

#[derive(Deserialize, Serialize)]
struct CachedIndex {
    key: CacheKey,
    index: ApiIndex,
}

struct TemporaryScript(PathBuf);

impl Drop for TemporaryScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn project_extension_fingerprint(project: &Path) -> Result<String, Box<dyn Error>> {
    let mut hasher = blake3::Hasher::new();
    let project_config = project.join("project.godot");
    hasher.update(&fs::read(&project_config)?);
    let extension_list = project.join(".godot/extension_list.cfg");
    let extension_list_text = fs::read_to_string(&extension_list).unwrap_or_default();
    hasher.update(extension_list_text.as_bytes());

    let mut descriptors: HashSet<PathBuf> = project_files::collect(project, &["gdextension"])?
        .into_iter()
        .collect();
    for resource_path in resource_paths(&extension_list_text, "gdextension") {
        descriptors.insert(project.join(resource_path));
    }

    let mut descriptors: Vec<_> = descriptors.into_iter().collect();
    descriptors.sort();
    let mut libraries = HashSet::new();
    for path in descriptors {
        hasher.update(path.to_string_lossy().as_bytes());
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        hasher.update(text.as_bytes());
        for extension in ["dll", "so", "dylib", "framework"] {
            for resource_path in resource_paths(&text, extension) {
                libraries.insert(project.join(resource_path));
            }
        }
    }
    let mut libraries: Vec<_> = libraries.into_iter().collect();
    libraries.sort();
    for path in libraries {
        hasher.update(path.to_string_lossy().as_bytes());
        match fs::metadata(path) {
            Ok(metadata) => {
                hasher.update(&metadata.len().to_le_bytes());
                if let Ok(modified) = metadata.modified()
                    && let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH)
                {
                    hasher.update(&duration.as_nanos().to_le_bytes());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hasher.update(b"missing");
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn resource_paths(text: &str, extension: &str) -> Vec<PathBuf> {
    text.match_indices("res://")
        .map(|(start, _)| &text[start + "res://".len()..])
        .map(|path| {
            let end = path
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '\"' | '\'' | ',' | ']' | '}')
                })
                .unwrap_or(path.len());
            &path[..end]
        })
        .filter(|path| {
            path.rsplit_once('.')
                .is_some_and(|(_, value)| value.eq_ignore_ascii_case(extension))
        })
        .map(|path| PathBuf::from(path.replace('/', std::path::MAIN_SEPARATOR_STR)))
        .collect()
}

fn cache_key(engine_path: &Path, project: &Path) -> Result<CacheKey, Box<dyn Error>> {
    let mut implementation = DefaultHasher::new();
    include_str!("api.rs").hash(&mut implementation);
    include_str!("api.gd").hash(&mut implementation);
    Ok(CacheKey {
        engine: engine::fingerprint(engine_path)?,
        project_extensions: project_extension_fingerprint(project)?,
        implementation: implementation.finish(),
    })
}

fn query_engine(engine_path: &Path, project: &Path) -> Result<ApiIndex, Box<dyn Error>> {
    let mut script = None;
    for attempt in 0..100 {
        let path =
            std::env::temp_dir().join(format!("gdkit-api-{}-{attempt}.gd", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(include_bytes!("api.gd"))?;
                script = Some(TemporaryScript(path));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let script = script.ok_or("could not create API query script")?;
    let output = Command::new(engine_path)
        .args(["--headless", "--no-header", "--path"])
        .arg(project)
        .arg("--script")
        .arg(&script.0)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let index = stdout
        .lines()
        .find_map(|line| line.strip_prefix(RESULT_PREFIX))
        .map(serde_json::from_str::<ApiIndex>)
        .transpose()?;
    match index {
        Some(index) if output.status.success() => Ok(index),
        _ => Err(format!(
            "configured engine could not enumerate its native API\n{}{}",
            stdout,
            String::from_utf8_lossy(&output.stderr)
        )
        .into()),
    }
}

fn load_index(engine_path: &Path, project: &Path) -> Result<ApiIndex, Box<dyn Error>> {
    let cache_path = project.join(".godot/gdkit/api-index.json");
    let key = cache_key(engine_path, project)?;
    if let Ok(bytes) = fs::read(&cache_path)
        && let Ok(cached) = serde_json::from_slice::<CachedIndex>(&bytes)
        && cached.key == key
    {
        return Ok(cached.index);
    }
    let index = query_engine(engine_path, project)?;
    let cached = CachedIndex { key, index };
    if let Some(parent) = cache_path.parent()
        && fs::create_dir_all(parent).is_ok()
        && let Ok(bytes) = serde_json::to_vec(&cached)
    {
        let _ = fs::write(&cache_path, bytes);
    }
    Ok(cached.index)
}

fn type_name(value: &ApiType, return_type: bool) -> String {
    if !value.class_name.is_empty() {
        return value.class_name.clone();
    }
    let name = match value.kind {
        0 if return_type => "void",
        0 => "Variant",
        1 => "bool",
        2 => "int",
        3 => "float",
        4 => "String",
        5 => "Vector2",
        6 => "Vector2i",
        7 => "Rect2",
        8 => "Rect2i",
        9 => "Vector3",
        10 => "Vector3i",
        11 => "Transform2D",
        12 => "Vector4",
        13 => "Vector4i",
        14 => "Plane",
        15 => "Quaternion",
        16 => "AABB",
        17 => "Basis",
        18 => "Transform3D",
        19 => "Projection",
        20 => "Color",
        21 => "StringName",
        22 => "NodePath",
        23 => "RID",
        24 => "Object",
        25 => "Callable",
        26 => "Signal",
        27 => "Dictionary",
        28 => "Array",
        29 => "PackedByteArray",
        30 => "PackedInt32Array",
        31 => "PackedInt64Array",
        32 => "PackedFloat32Array",
        33 => "PackedFloat64Array",
        34 => "PackedStringArray",
        35 => "PackedVector2Array",
        36 => "PackedVector3Array",
        37 => "PackedColorArray",
        38 => "PackedVector4Array",
        _ => return format!("Variant({})", value.kind),
    };
    name.to_owned()
}

fn arguments(values: &[ApiArgument], defaults: &[String]) -> String {
    let default_start = values.len().saturating_sub(defaults.len());
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let mut text = format!("{}: {}", value.name, type_name(&value.value_type, false));
            if index >= default_start {
                text.push_str(" = ");
                text.push_str(&defaults[index - default_start]);
            }
            text
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn method_signature(method: &ApiMethod) -> String {
    let mut qualifiers = Vec::new();
    if method.flags & 32 != 0 {
        qualifiers.push("static");
    }
    if method.flags & 16 != 0 {
        qualifiers.push("vararg");
    }
    let qualifier = if qualifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", qualifiers.join(" "))
    };
    format!(
        "{qualifier}func {}({}) -> {}",
        method.name,
        arguments(&method.arguments, &method.defaults),
        type_name(&method.return_type, true)
    )
}

fn signal_signature(signal: &ApiSignal) -> String {
    format!(
        "signal {}({})",
        signal.name,
        arguments(&signal.arguments, &[])
    )
}

fn class_map(index: &ApiIndex) -> HashMap<&str, &ApiClass> {
    index
        .classes
        .iter()
        .map(|class| (class.name.as_str(), class))
        .collect()
}

fn find_class<'a>(index: &'a ApiIndex, name: &str) -> Option<&'a ApiClass> {
    index
        .classes
        .iter()
        .find(|class| class.name.eq_ignore_ascii_case(name))
}

fn lineage<'a>(index: &'a ApiIndex, class: &'a ApiClass) -> Vec<&'a ApiClass> {
    let classes = class_map(index);
    let mut result = vec![class];
    let mut parent = class.parent.as_str();
    while let Some(class) = classes.get(parent).copied() {
        result.push(class);
        parent = class.parent.as_str();
    }
    result
}

fn engine_header(index: &ApiIndex, engine_path: &Path) {
    println!(
        "Engine {} ({})",
        index.version,
        engine::display_path(engine_path)
    );
    println!(
        "Metadata: argument names {}, default values {}",
        if index.method_argument_names {
            "available"
        } else {
            "unavailable"
        },
        if index.method_default_values {
            "available"
        } else {
            "unavailable"
        }
    );
}

fn inheritance_text(chain: &[&ApiClass]) -> String {
    chain
        .iter()
        .map(|class| class.name.as_str())
        .collect::<Vec<_>>()
        .join(" < ")
}

fn owner_suffix(owner: &ApiClass, requested: &ApiClass) -> String {
    if owner.name == requested.name {
        String::new()
    } else {
        format!(" [from {}]", owner.name)
    }
}

fn print_class(index: &ApiIndex, engine_path: &Path, class: &ApiClass) {
    let chain = lineage(index, class);
    engine_header(index, engine_path);
    println!("\nClass {}", inheritance_text(&chain));

    println!("\nMethods:");
    for owner in &chain {
        for method in &owner.methods {
            println!(
                "  {}{}",
                method_signature(method),
                owner_suffix(owner, class)
            );
        }
    }
    println!("\nProperties:");
    for owner in &chain {
        for property in &owner.properties {
            println!(
                "  {}: {}{}",
                property.name,
                type_name(&property.value_type, false),
                owner_suffix(owner, class)
            );
        }
    }
    println!("\nSignals:");
    for owner in &chain {
        for signal in &owner.signals {
            println!(
                "  {}{}",
                signal_signature(signal),
                owner_suffix(owner, class)
            );
        }
    }
    println!("\nEnums:");
    for owner in &chain {
        for enum_info in &owner.enums {
            let values = enum_info
                .values
                .iter()
                .map(|value| format!("{} = {}", value.name, value.value))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "  enum {} {{ {} }}{}",
                enum_info.name,
                values,
                owner_suffix(owner, class)
            );
        }
    }
    println!("\nConstants:");
    for owner in &chain {
        for constant in &owner.constants {
            println!(
                "  {} = {}{}",
                constant.name,
                constant.value,
                owner_suffix(owner, class)
            );
        }
    }
}

fn member_lines(index: &ApiIndex, class: &ApiClass, member: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for owner in lineage(index, class) {
        let suffix = owner_suffix(owner, class);
        for method in &owner.methods {
            if method.name.eq_ignore_ascii_case(member) {
                lines.push(format!("{}{}", method_signature(method), suffix));
            }
        }
        for property in &owner.properties {
            if property.name.eq_ignore_ascii_case(member) {
                lines.push(format!(
                    "property {}: {}{}",
                    property.name,
                    type_name(&property.value_type, false),
                    suffix
                ));
            }
        }
        for signal in &owner.signals {
            if signal.name.eq_ignore_ascii_case(member) {
                lines.push(format!("{}{}", signal_signature(signal), suffix));
            }
        }
        for enum_info in &owner.enums {
            if enum_info.name.eq_ignore_ascii_case(member) {
                let values = enum_info
                    .values
                    .iter()
                    .map(|value| format!("{} = {}", value.name, value.value))
                    .collect::<Vec<_>>()
                    .join(", ");
                lines.push(format!(
                    "enum {} {{ {} }}{}",
                    enum_info.name, values, suffix
                ));
            }
            for value in &enum_info.values {
                if value.name.eq_ignore_ascii_case(member) {
                    lines.push(format!(
                        "{}.{} = {}{}",
                        enum_info.name, value.name, value.value, suffix
                    ));
                }
            }
        }
        for constant in &owner.constants {
            if constant.name.eq_ignore_ascii_case(member) {
                lines.push(format!(
                    "const {} = {}{}",
                    constant.name, constant.value, suffix
                ));
            }
        }
    }
    lines
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = vec![left_index + 1];
        for (right_index, right_char) in right.iter().enumerate() {
            current.push(
                (previous[right_index + 1] + 1)
                    .min(current[right_index] + 1)
                    .min(previous[right_index] + usize::from(left_char != *right_char)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn suggestions<'a>(names: impl Iterator<Item = &'a str>, query: &str) -> Vec<&'a str> {
    let query = query.to_ascii_lowercase();
    let mut scored: Vec<_> = names
        .map(|name| (edit_distance(&name.to_ascii_lowercase(), &query), name))
        .collect();
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)));
    scored.dedup_by(|left, right| left.1 == right.1);
    scored.into_iter().take(5).map(|(_, name)| name).collect()
}

fn print_member(index: &ApiIndex, engine_path: &Path, class: &ApiClass, member: &str) -> ExitCode {
    engine_header(index, engine_path);
    let chain = lineage(index, class);
    println!("\nClass {}", inheritance_text(&chain));
    let lines = member_lines(index, class, member);
    if !lines.is_empty() {
        for line in lines {
            println!("\n{line}");
        }
        return ExitCode::SUCCESS;
    }
    println!("\nNo native member named '{member}' on {}.", class.name);
    let names = chain.iter().flat_map(|owner| {
        owner
            .methods
            .iter()
            .map(|value| value.name.as_str())
            .chain(owner.properties.iter().map(|value| value.name.as_str()))
            .chain(owner.signals.iter().map(|value| value.name.as_str()))
            .chain(owner.enums.iter().map(|value| value.name.as_str()))
            .chain(owner.constants.iter().map(|value| value.name.as_str()))
    });
    let nearby = suggestions(names, member);
    if !nearby.is_empty() {
        println!("Did you mean: {}?", nearby.join(", "));
    }
    println!("Attached scripts and subclasses may add non-native members.");
    ExitCode::from(1)
}

fn search_lines(index: &ApiIndex, term: &str) -> Vec<(u8, String)> {
    let needle = term.to_ascii_lowercase();
    let score = |name: &str| {
        let name = name.to_ascii_lowercase();
        if name == needle {
            Some(0)
        } else if name.starts_with(&needle) {
            Some(1)
        } else if name.contains(&needle) {
            Some(2)
        } else {
            None
        }
    };
    let mut lines = Vec::new();
    for class in &index.classes {
        if let Some(score) = score(&class.name) {
            let parent = if class.parent.is_empty() {
                String::new()
            } else {
                format!(" < {}", class.parent)
            };
            lines.push((score, format!("class {}{}", class.name, parent)));
        }
        for method in &class.methods {
            if let Some(score) = score(&method.name) {
                lines.push((
                    score,
                    format!("{}.{}", class.name, method_signature(method)),
                ));
            }
        }
        for property in &class.properties {
            if let Some(score) = score(&property.name) {
                lines.push((
                    score,
                    format!(
                        "{}.{}: {} [property]",
                        class.name,
                        property.name,
                        type_name(&property.value_type, false)
                    ),
                ));
            }
        }
        for signal in &class.signals {
            if let Some(score) = score(&signal.name) {
                lines.push((
                    score,
                    format!("{}.{}", class.name, signal_signature(signal)),
                ));
            }
        }
        for enum_info in &class.enums {
            if let Some(score) = score(&enum_info.name) {
                lines.push((score, format!("{}.{} [enum]", class.name, enum_info.name)));
            }
            for value in &enum_info.values {
                if let Some(score) = score(&value.name) {
                    lines.push((
                        score,
                        format!(
                            "{}.{}.{} = {}",
                            class.name, enum_info.name, value.name, value.value
                        ),
                    ));
                }
            }
        }
        for constant in &class.constants {
            if let Some(score) = score(&constant.name) {
                lines.push((
                    score,
                    format!(
                        "{}.{} = {} [constant]",
                        class.name, constant.name, constant.value
                    ),
                ));
            }
        }
    }
    lines.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    lines
}

fn print_search(index: &ApiIndex, engine_path: &Path, term: &str) -> ExitCode {
    engine_header(index, engine_path);
    let lines = search_lines(index, term);
    println!("\nNative API search for '{term}':");
    if lines.is_empty() {
        println!("  no matches");
        let nearby = suggestions(index.classes.iter().map(|class| class.name.as_str()), term);
        if !nearby.is_empty() {
            println!("Did you mean: {}?", nearby.join(", "));
        }
        return ExitCode::from(1);
    }
    for (_, line) in lines.iter().take(SEARCH_LIMIT) {
        println!("  {line}");
    }
    if lines.len() > SEARCH_LIMIT {
        println!("  ... {} more matches", lines.len() - SEARCH_LIMIT);
    }
    ExitCode::SUCCESS
}

pub(crate) fn run(args: ApiArgs) -> Result<ExitCode, Box<dyn Error>> {
    if args.query == "search" && args.member.is_none() {
        return Err("api search requires a search term".into());
    }
    let project = engine::project_root(&args.project)?;
    let engine_path = engine::resolve(&project, args.godot.as_deref())?;
    let index = load_index(&engine_path, &project)?;
    if args.query == "search" {
        let term = args.member.as_deref().unwrap();
        return Ok(print_search(&index, &engine_path, term));
    }
    let Some(class) = find_class(&index, &args.query) else {
        engine_header(&index, &engine_path);
        println!("\nNo native class named '{}'.", args.query);
        let nearby = suggestions(
            index.classes.iter().map(|class| class.name.as_str()),
            &args.query,
        );
        if !nearby.is_empty() {
            println!("Did you mean: {}?", nearby.join(", "));
        }
        return Ok(ExitCode::from(1));
    };
    Ok(match args.member.as_deref() {
        Some(member) => print_member(&index, &engine_path, class, member),
        None => {
            print_class(&index, &engine_path, class);
            ExitCode::SUCCESS
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_type(kind: u32, class_name: &str) -> ApiType {
        ApiType {
            kind,
            class_name: class_name.to_owned(),
        }
    }

    #[test]
    fn formats_signatures_and_default_arguments() {
        let method = ApiMethod {
            name: "configure".into(),
            return_type: value_type(1, ""),
            arguments: vec![
                ApiArgument {
                    name: "peer".into(),
                    value_type: value_type(24, "MultiplayerPeer"),
                },
                ApiArgument {
                    name: "channel".into(),
                    value_type: value_type(2, ""),
                },
            ],
            defaults: vec!["0".into()],
            flags: 32,
        };
        assert_eq!(
            method_signature(&method),
            "static func configure(peer: MultiplayerPeer, channel: int = 0) -> bool"
        );
        assert_eq!(
            type_name(&value_type(2, "PhysicsServer3D.BodyAxis"), false),
            "PhysicsServer3D.BodyAxis"
        );
    }

    #[test]
    fn ranks_exact_prefix_and_substring_searches() {
        assert_eq!(edit_distance("move_and_slide", "move_and_slid"), 1);
        let names = ["slide", "move_and_slide", "move_and_collide"];
        assert_eq!(
            suggestions(names.into_iter(), "move_and_slid")[0],
            "move_and_slide"
        );
    }

    #[test]
    fn extension_fingerprint_tracks_enabled_ignored_libraries() {
        let directory = std::env::temp_dir().join(format!("gdkit-api-key-{}", std::process::id()));
        fs::create_dir_all(directory.join(".godot")).unwrap();
        fs::create_dir_all(directory.join("addons/native/bin")).unwrap();
        fs::write(directory.join("project.godot"), "config_version=5\n").unwrap();
        fs::write(directory.join(".gitignore"), "addons/\n").unwrap();
        fs::write(
            directory.join(".godot/extension_list.cfg"),
            "res://addons/native/native.gdextension\n",
        )
        .unwrap();
        fs::write(
            directory.join("addons/native/native.gdextension"),
            "[libraries]\nwindows = \"res://addons/native/bin/native.dll\"\n",
        )
        .unwrap();
        let library = directory.join("addons/native/bin/native.dll");
        fs::write(&library, "first").unwrap();
        let before = project_extension_fingerprint(&directory).unwrap();
        fs::write(&library, "second version").unwrap();
        let after = project_extension_fingerprint(&directory).unwrap();
        assert_ne!(before, after);
        fs::remove_dir_all(directory).unwrap();
    }
}
