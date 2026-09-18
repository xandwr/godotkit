use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use serde::Serialize;
use serde_json::Value;

use crate::cli::{AnimationArgs, AnimationCommand, AnimationListArgs, NetOutput};

const GLB_MAGIC: &[u8; 4] = b"glTF";
const GLB_VERSION: u32 = 2;
const JSON_CHUNK: u32 = 0x4e4f_534a;

#[derive(Debug, PartialEq, Serialize)]
struct AnimationReport {
    schema_version: u32,
    file: PathBuf,
    animations: Vec<PackedAnimation>,
}

#[derive(Debug, PartialEq, Serialize)]
struct PackedAnimation {
    index: usize,
    name: Option<String>,
    duration_seconds: Option<f64>,
    channels: usize,
    samplers: usize,
    target_nodes: usize,
    target_properties: Vec<String>,
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or("truncated GLB header or chunk")?;
    Ok(u32::from_le_bytes(value.try_into()?))
}

fn json_chunk(bytes: &[u8]) -> Result<&[u8], Box<dyn Error>> {
    if bytes.len() < 20 {
        return Err("file is too short to be a GLB".into());
    }
    if bytes.get(..4) != Some(GLB_MAGIC) {
        return Err("invalid GLB magic; expected binary glTF".into());
    }
    let version = read_u32(bytes, 4)?;
    if version != GLB_VERSION {
        return Err(format!("unsupported GLB version {version}; expected version 2").into());
    }
    let declared_length = usize::try_from(read_u32(bytes, 8)?)?;
    if declared_length != bytes.len() {
        return Err(format!(
            "GLB length mismatch: header declares {declared_length} bytes but file contains {}",
            bytes.len()
        )
        .into());
    }
    let chunk_length = usize::try_from(read_u32(bytes, 12)?)?;
    let chunk_type = read_u32(bytes, 16)?;
    if chunk_type != JSON_CHUNK {
        return Err("the first GLB chunk is not JSON".into());
    }
    let end = 20usize
        .checked_add(chunk_length)
        .ok_or("GLB JSON chunk length overflow")?;
    bytes
        .get(20..end)
        .ok_or_else(|| "truncated GLB JSON chunk".into())
}

fn number_at(value: &Value, field: &str) -> Option<f64> {
    value.get(field)?.as_array()?.first()?.as_f64()
}

fn animation_duration(
    animation: &Value,
    accessors: &[Value],
) -> Result<Option<f64>, Box<dyn Error>> {
    let Some(samplers) = animation.get("samplers").and_then(Value::as_array) else {
        return Ok(None);
    };
    let mut start: Option<f64> = None;
    let mut end: Option<f64> = None;
    for sampler in samplers {
        let input = sampler
            .get("input")
            .and_then(Value::as_u64)
            .ok_or("animation sampler is missing an input accessor")?;
        let input = usize::try_from(input)?;
        let accessor = accessors
            .get(input)
            .ok_or_else(|| format!("animation sampler references missing accessor {input}"))?;
        let (Some(minimum), Some(maximum)) =
            (number_at(accessor, "min"), number_at(accessor, "max"))
        else {
            continue;
        };
        start = Some(start.map_or(minimum, |current| current.min(minimum)));
        end = Some(end.map_or(maximum, |current| current.max(maximum)));
    }
    Ok(start.zip(end).map(|(start, end)| {
        let duration = (end - start).max(0.0);
        (duration * 1_000_000_000.0).round() / 1_000_000_000.0
    }))
}

fn parse(path: &Path, bytes: &[u8]) -> Result<AnimationReport, Box<dyn Error>> {
    let json = json_chunk(bytes)?;
    let document: Value = serde_json::from_slice(json)?;
    let accessors = document
        .get("accessors")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let animations = document
        .get("animations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut packed = Vec::with_capacity(animations.len());
    for (index, animation) in animations.iter().enumerate() {
        let channels = animation
            .get("channels")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let samplers = animation
            .get("samplers")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let mut target_nodes = BTreeSet::new();
        let mut target_properties = BTreeSet::new();
        for channel in channels {
            let Some(target) = channel.get("target") else {
                continue;
            };
            if let Some(node) = target.get("node").and_then(Value::as_u64) {
                target_nodes.insert(node);
            }
            if let Some(property) = target.get("path").and_then(Value::as_str) {
                target_properties.insert(property.to_owned());
            }
        }
        packed.push(PackedAnimation {
            index,
            name: animation
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            duration_seconds: animation_duration(animation, accessors)?,
            channels: channels.len(),
            samplers,
            target_nodes: target_nodes.len(),
            target_properties: target_properties.into_iter().collect(),
        });
    }
    Ok(AnimationReport {
        schema_version: 1,
        file: path.to_owned(),
        animations: packed,
    })
}

fn display_name(animation: &PackedAnimation) -> String {
    animation
        .name
        .clone()
        .unwrap_or_else(|| format!("<unnamed #{}>", animation.index + 1))
}

fn print_human(report: &AnimationReport, names_only: bool) {
    if names_only {
        for animation in &report.animations {
            println!("{}", display_name(animation));
        }
        return;
    }
    let label = report
        .file
        .file_name()
        .unwrap_or(report.file.as_os_str())
        .to_string_lossy();
    println!(
        "{label}\n{} animation{}",
        report.animations.len(),
        if report.animations.len() == 1 {
            ""
        } else {
            "s"
        }
    );
    if report.animations.is_empty() {
        return;
    }
    let width = report
        .animations
        .iter()
        .map(|animation| display_name(animation).chars().count())
        .chain([4])
        .max()
        .unwrap_or(4);
    println!(
        "\n{:<width$}  {:>8}  {:>8}  {:>7}",
        "NAME", "DURATION", "CHANNELS", "TARGETS"
    );
    for animation in &report.animations {
        let duration = animation
            .duration_seconds
            .map_or_else(|| "-".to_owned(), |value| format!("{value:.3}s"));
        println!(
            "{:<width$}  {:>8}  {:>8}  {:>7}",
            display_name(animation),
            duration,
            animation.channels,
            animation.target_nodes
        );
    }
}

fn list(args: AnimationListArgs) -> Result<ExitCode, Box<dyn Error>> {
    if !args
        .path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
    {
        return Err(format!("{} is not a .glb file", args.path.display()).into());
    }
    let bytes = fs::read(&args.path)
        .map_err(|error| format!("could not read {}: {error}", args.path.display()))?;
    let mut report = parse(&args.path, &bytes)
        .map_err(|error| format!("could not inspect {}: {error}", args.path.display()))?;
    if let Some(filter) = args.filter {
        let filter = filter.to_lowercase();
        report
            .animations
            .retain(|animation| display_name(animation).to_lowercase().contains(&filter));
    }
    match args.output {
        NetOutput::Human => print_human(&report, args.names),
        NetOutput::Json => println!("{}", serde_json::to_string_pretty(&report)?),
    }
    Ok(ExitCode::SUCCESS)
}

pub(crate) fn run(args: AnimationArgs) -> Result<ExitCode, Box<dyn Error>> {
    match args.command {
        AnimationCommand::List(args) => list(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glb(document: &str) -> Vec<u8> {
        let mut json = document.as_bytes().to_vec();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let length = 20 + json.len();
        let mut bytes = Vec::with_capacity(length);
        bytes.extend_from_slice(GLB_MAGIC);
        bytes.extend_from_slice(&GLB_VERSION.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(length).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(json.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&JSON_CHUNK.to_le_bytes());
        bytes.extend_from_slice(&json);
        bytes
    }

    #[test]
    fn reads_names_durations_and_target_counts() {
        let bytes = glb(
            r#"{"asset":{"version":"2.0"},"accessors":[{"min":[0.25],"max":[1.75]},{"min":[0.0],"max":[2.5]}],"animations":[{"name":"Run","samplers":[{"input":0},{"input":1}],"channels":[{"target":{"node":3,"path":"rotation"}},{"target":{"node":3,"path":"translation"}},{"target":{"node":4,"path":"rotation"}}]},{"samplers":[],"channels":[]}]}"#,
        );
        let report = parse(Path::new("model.glb"), &bytes).unwrap();
        assert_eq!(report.animations.len(), 2);
        assert_eq!(report.animations[0].name.as_deref(), Some("Run"));
        assert_eq!(report.animations[0].duration_seconds, Some(2.5));
        assert_eq!(report.animations[0].channels, 3);
        assert_eq!(report.animations[0].target_nodes, 2);
        assert_eq!(
            report.animations[0].target_properties,
            ["rotation", "translation"]
        );
        assert_eq!(display_name(&report.animations[1]), "<unnamed #2>");
    }

    #[test]
    fn rejects_invalid_containers_and_accessor_references() {
        assert!(json_chunk(b"not a glb").is_err());
        let bytes = glb(r#"{"asset":{"version":"2.0"},"animations":[{"samplers":[{"input":4}]}]}"#);
        assert!(parse(Path::new("broken.glb"), &bytes).is_err());
    }
}
