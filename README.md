# gdkit

A Rust formatter targeting GDScript 4.7.2. Normalizes whitespace, orders fields,
and compacts single-statement suites using gdview's lossless syntax tree. Inputs with
parser diagnostics are rejected before producing output or writing files.

```sh
cargo run -- format script.gd
cargo run -- format script.gd --check
cargo run -- format - < script.gd
cargo run -- format-project
cargo run -- format-project --check
cargo run -- autoloads
cargo run -- scene-tree scene.tscn
cargo run -- scene-tree scene.tscn --expand
cargo run -- scene-tree scene.tscn --expand-depth 2
cargo run -- init --godot /path/to/godot
cargo run -- api CharacterBody3D move_and_slide
cargo run -- api search multiplayer
cargo run -- check /path/to/project
cargo run -- check /path/to/project --script res://tests/contract.gd
cargo run -- run --name server --headless -- --server
cargo run -- sessions
cargo run -- logs server
cargo run -- stop server
cargo test
cargo test --test godot -- --ignored
GODOT_SOURCE=/path/to/godot cargo test --test corpus -- --ignored
```

## Project engine

Run `gdkit init --godot <editor-executable>` from the directory containing
`project.godot`. It validates the project directory and probes the engine before
creating `gdkit.toml`. Existing configuration is never overwritten. For a monorepo
with sibling `game` and `engine` directories, the committed config can look like:

```toml
[engine]
executable = "../engine/bin/windows-x86_64/godot.windows.editor.x86_64.console.exe"
```

Paths in this file resolve relative to the file, regardless of the shell's current
directory. Initialization writes a relative path when possible, or an absolute
path when the engine is on another Windows drive. Edit `engine.executable` to
change the association.

Engine selection uses `--godot`, then `GDKIT_GODOT`, then the project's config.
Explicit and environment paths resolve relative to the current directory. There
is no bundled-engine or PATH fallback; a missing or invalid selection is an error.
`init` also accepts `GDKIT_GODOT` when `--godot` is omitted.

Both commands probe the selected executable in an isolated temporary project.
They require Godot 4 editor command-line support, working GDScript and resource
APIs, and acceptance of the checker harness. Exact versions and release suffixes
are not restricted, so compatible custom builds and engine updates are accepted.
The selected executable and reported version are printed to stderr. Compatibility
has been tested with official 4.7.2 and a custom 4.7.3 RC build; the probe permits
other Godot 4 editors without claiming that every release has been tested.

Successful compatibility probes are cached per project in
`.godot/gdkit/engine-probe.json`. Repeated commands reuse the reported version
without launching the three compatibility-check processes. The cache is
invalidated by changes to the engine path, file size, or modification time, or to
gdkit's probe implementation and scripts. Windows console launchers also track
their companion engine executable when present. Failed probes are never cached;
missing, unreadable, or malformed caches trigger a fresh probe. Cache write
failures do not prevent checking. Delete the cache file to force a fresh probe.

Run `gdkit doctor [project]` to explain the resolved project and engine, exact
engine version, selection source, compatibility-probe cache health, legacy
import-worker cleanup state, effective check warning policy, and likely project-specific gotchas. Like
`check`, it accepts `--godot`; otherwise the normal selection precedence applies.

`gdkit api <class> [member]` reflects the configured engine's native `ClassDB`.
Class queries print signatures, properties, signals, enum values, constants, and
the full inherited surface with each inherited member's declaring class. Member
queries walk that same inheritance chain and suggest nearby names for typos.
`gdkit api search <term>` searches class and member names across the native API.

Project scripts declaring `class_name` participate in the same lookup. Class
queries show their declared methods, properties, signals, enums, constants,
script inheritance, and `res://` source locations. Member queries follow project
base classes and then the configured engine's native base class. API search
combines native and project results. This source index uses gdview's syntax tree,
respects the normal project file-discovery policy, and does not execute project
scripts.
Negative queries exit 1. Anonymous scripts, inner classes, computed members, and
runtime state remain outside this static project index.

Every result identifies the actual engine executable and version. The index runs
inside the selected project so registered GDExtension classes are included, but
it does not enter a gameplay scene. Native metadata is cached at
`.godot/gdkit/api-index.json` and invalidated by engine replacement, gdkit's
reflection implementation, project feature settings, enabled extension lists,
extension descriptors, and their referenced native library metadata. As with
`check`, `--godot` overrides `GDKIT_GODOT`, which overrides `gdkit.toml`.

`gdkit resource create --spec weapon.json --out res://weapons/shotgun.tres`
creates a new Resource using the configured engine. The destination's parent
directory must already exist. A spec selects exactly one native `class` or
project-local GDScript `script`, and supplies a `properties` object:

```json
{
  "script": "res://resources/weapon_definition.gd",
  "properties": {
    "damage": 15,
    "display_name": "Shotgun"
  }
}
```

Native types use `"class": "StandardMaterial3D"` instead of `script`. Scripts
must instantiate a Resource without required constructor arguments. This
operation executes project constructors, getters, and setters without entering
the main scene. Existing project dependencies must be importable; refresh stale
script-class or asset caches with `gdkit cache refresh` when necessary.

Properties must be writable and serialized. Supported values are booleans,
integers, floats, strings, tagged `StringName`, `NodePath`, vectors, integer
vectors, `Color`, rectangles, and spatial values, Resource fields, and typed
arrays of Resources, packed arrays, and typed scalar arrays and dictionaries.
Unknown fields, transient fields, and unsupported compound values fail explicitly.
Plain JSON integer input is limited to the exact transport range of plus or minus
9007199254740991. Tagged integer strings support the full signed 64-bit range.
Values are checked after assignment and again
after Godot saves and reloads the staged `.tres` without its instance cache.
Setter transformations and serialization precision changes fail verification.
Unspecified fields retain engine or script defaults.

The version 1 tagged Variant codec uses exactly one `$variant` key containing
exactly `type` and `value`. The `int`, `StringName`, and `NodePath` tags require
a string payload:

```json
{
  "integer": {"$variant": {"type": "int", "value": "9223372036854775807"}},
  "symbol": {"$variant": {"type": "StringName", "value": "Shotgun"}},
  "node_path": {"$variant": {"type": "NodePath", "value": "Root/Child:position"}}
}
```

The tag must match the declared property type. Integer text must be canonical
signed decimal from `-9223372036854775808` to `9223372036854775807`; leading zeros,
a plus sign, whitespace, and `-0` are rejected. `StringName` and `NodePath`
require explicit tags, including for empty values. Any engine normalization of
the payload is rejected. Tagged values remain tagged in creation results,
including nested specs, so large integers never pass through a JSON number.
No expression evaluation or arbitrary GDScript input is supported.

`Vector2`, `Vector3`, and `Vector4` tags use numeric component arrays in `x`, `y`,
`z`, `w` order, with exactly the appropriate number of entries. `Vector2i`,
`Vector3i`, and `Vector4i` use signed 32-bit integer components. `Color` requires
four numeric components in `r`, `g`, `b`, `a` order, including explicit alpha:

```json
{
  "offset": {"$variant": {"type": "Vector3", "value": [1.5, -2.25, 3.125]}},
  "grid": {"$variant": {"type": "Vector2i", "value": [-2, 4]}},
  "albedo_color": {"$variant": {"type": "Color", "value": [0.5, 0.25, 0.75, 1.0]}}
}
```

Components must be finite and exactly representable by the configured engine.
For example, a standard single-precision engine rejects an authored `0.1`
component rather than silently rounding it; `0.125` is exact. Color components
are not clamped to the 0 to 1 range. Ordinary JSON arrays do not imply a vector
or color. Component errors identify paths such as
`offset.$variant.value[2]`. Schema defaults and creation results use full-precision
JSON output so engine-authored components can be reused exactly.

Rectangles and spatial types use the same flat, numeric component arrays.
`variant_contract.components` gives their exact order:

| Tag | Component order |
| --- | --- |
| `Rect2`, `Rect2i` | position x/y, size x/y |
| `Transform2D` | x column x/y, y column x/y, origin x/y |
| `Basis` | x column x/y/z, y column x/y/z, z column x/y/z |
| `Transform3D` | basis columns in the order above, then origin x/y/z |
| `Quaternion` | x, y, z, w |
| `Plane` | normal x/y/z, d |
| `AABB` | position x/y/z, size x/y/z |

`Rect2i` components follow the same signed 32-bit rules as integer vectors.
Sizes, bases, quaternions, and plane normals retain the supplied values; the
codec does not take absolute sizes, orthonormalize matrices, or normalize
rotations and normals. Setters and serialization still must preserve every
requested value. For example:

```json
{
  "region": {"$variant": {"type": "Rect2", "value": [1.5, 2.25, 8.0, 4.0]}},
  "transform": {"$variant": {"type": "Transform3D", "value": [1, 0, 0, 0, 1, 0, 0, 0, 1, 2, 3, 4]}}
}
```


Packed array tags are `PackedByteArray`, `PackedInt32Array`, `PackedInt64Array`,
`PackedFloat32Array`, `PackedFloat64Array`, `PackedStringArray`,
`PackedVector2Array`, `PackedVector3Array`, `PackedVector4Array`, and
`PackedColorArray`. Their `value` is an ordered JSON array. Vector/color entries
use their existing tags. Byte entries must be 0 to 255, and Int32 entries must
fit signed 32-bit storage. Int64 entries beyond the safe JSON range use the
`int` string tag. Float and vector entries must survive storage and reload
exactly; for example Float32 rejects an authored `0.1`, while Float64 accepts it.

Scalar arrays and dictionaries declare their types explicitly:

```json
{
  "scores": {"$variant": {"type": "Array", "value": {
    "element_type": "int",
    "items": [10, {"$variant": {"type": "int", "value": "9223372036854775807"}}]
  }}},
  "scores_by_name": {"$variant": {"type": "Dictionary", "value": {
    "key_type": "StringName",
    "value_type": "int",
    "entries": [[{"$variant": {"type": "StringName", "value": "Alice"}}, 10]]
  }}}
}
```

Type names must match the property's actual Godot container metadata. Scalar
means `bool`, `int`, `float`, `String`, or any supported non-container Variant
such as `NodePath`, `Vector3`, or `Transform3D`. Container entries use the same
scalar and tagged value rules as individual properties. Dictionaries use
`[key, value]` pairs to preserve non-string keys, and reject duplicate keys after
decoding. Untyped containers, nested containers, and Resource-valued dictionaries
are not creation inputs. Existing Resource arrays keep their earlier JSON-array
syntax. Empty arrays and dictionaries preserve all declared types after reload.

Schema container contracts include `element_contract`, or `key_contract` and
`value_contract`, with their scalar codecs and allowed inputs. Non-empty defaults
and verified creation results are reusable specs. Safe integers in container
results use JSON numbers; larger integers always use decimal string tags.
Indexed errors identify entries such as
`scores.$variant.value.items[2]` or
`scores_by_name.$variant.value.entries[1][0]`. Variant payloads exceeding 64 nested
levels are rejected before engine execution.

The verified file is published without overwriting an existing destination;
failed operations remove their staging directory. Publication uses a hard link
within the destination filesystem and fails if that filesystem does not support
hard links. `--project` and `--godot` use the normal engine selection rules.
`--output json` returns `status`, `path`, native `type`, optional `script`, and
verified `properties`; errors return `status`, `stage`, `field`, and `message`,
with engine diagnostics when available. Creation failures exit 1. The returned
`res://` path is a persistent reference, not a live editor object handle.

Resource-valued fields accept null, an existing project-local reference, or an
inline Resource spec:

```json
{
  "script": "res://resources/weapon_definition.gd",
  "properties": {
    "icon": { "$ref": "res://textures/shotgun.svg" },
    "stats": {
      "$resource": {
        "script": "res://resources/weapon_stats.gd",
        "properties": { "damage": 15 }
      }
    }
  }
}
```

Each directive contains exactly one key. Inline specs use the same class/script
and property rules as the root. Godot saves inline objects as subresources and
existing references as external resources; gdkit does not save referenced files.
Native and registered script constraints are checked before assignment, including
inherited script types. Godot also checks assignment types, including anonymous
script types that reflection may describe only by their native or global base.

Verification checks referenced paths and snapshots serialized fields throughout
the graph, including defaults, after assignment, saving, and a reload that
bypasses dependency caches. A setter that mutates a supplied nested or referenced
object fails creation. Nested errors identify fields such as
`properties.stats.properties.damage`; root field errors retain their existing
names. Cyclic graphs, more than 16 nested Resources, and serialized container
graphs deeper than 16 are rejected. Compound serialized dictionary keys and
non-Resource serialized objects are also unsupported. Typed arrays of Resources accept JSON arrays containing null, `$ref`, and
`$resource` entries, for example `"effects": [null, {"$ref": "res://effect.tres"}]`.
Native and custom script element types, order, and empty-array typing survive
reload. Element errors include indices such as
`properties.effects[2].properties.duration`. Each inline entry creates a separate
object; named shared inline-object identities remain outside the input format.

`gdkit resource schema --class StandardMaterial3D --output json` discovers
instance fields using the configured engine. For a custom Resource, use
`gdkit resource schema --script res://resources/weapon_definition.gd --output json`.
Exactly one type selector is required. Schema shares creation's construction and
property rules, accepts `--project` and `--godot`, and includes inherited fields.
It instantiates the selected type and executes constructors and getters,
including project scripts; it does not save a resource or enter the main scene.

JSON results have `status: "schema"`, native `type`, optional `script`,
`executes_constructors_and_getters: true`, `integer_min`, `integer_max`, and
`fields`. Each field contains `name`, `type`, `type_id`, `class_name`, instance
`default`, raw `hint` and `hint_string`, decoded `enum_choices`, raw `usage`,
`storage`, `read_only`, `editor_visible`, `create_supported`, an
`unsupported_reason`, `accepted_inputs`, and `resource_constraints`. Resource
constraints contain native/global `class` and a registered `script` path when
available. Supported Resource fields list `null`, `$ref`, and `$resource` as
accepted inputs. Supported Resource arrays list `array`, expose
`element_constraints`, and list their allowed `element_accepted_inputs`.
Tagged scalar fields expose `variant_contract` with the tag, type, payload
encoding, component order/count, exactness requirements, and integer bounds
where applicable. Results also include
`variant_codec_version` and `max_resource_depth`. Inspector
group/category entries are omitted. Enum choices
contain `name` and `value`, including explicit integer enum values. Hints describe
editor controls; setters and reload verification still determine creation success.

Defaults carry an `encoding` and `value`: `json` for transportable scalar values
and null; `tagged` for reusable `$variant` inputs, including large integers,
`StringName`, `NodePath`, vectors, and `Color`; `resource` for a descriptive path/type/script
reference; `godot` for Godot text describing other values, including unsupported
compound defaults.
An empty resource path denotes an unsaved resource. Descriptive defaults are not
creation inputs; use `$ref` or `$resource` explicitly for a Resource default.
`create_supported` describes the field's type and usage, so
agents must also check a default's encoding and the integer limits before using
it in a spec. Defaults reflect a newly constructed instance, including constructor
changes. Failures use creation's error format and exit 1; selector errors exit 2.

`gdkit net [project]` prints the project's authored multiplayer topology without
entering its main scene. It asks the selected engine for effective GDScript RPC
configuration, then combines that result with gdview source locations for RPC
calls, peer construction and assignment, connection lifecycle signals, authority
and peer-identity checks, networked autoloads, and text-scene
`MultiplayerSpawner` or `MultiplayerSynchronizer` nodes. Use `--output json` for
the versioned typed report. Results are observations rather than lint failures;
dynamic targets and unsupported source surfaces are reported as unknowns.

```text
gdkit net
gdkit net game --output json
gdkit net --godot /path/to/godot
gdkit net explain LobbyCoordinator._request_start_match --project game
```

The command currently covers GDScript and `.tscn` files discovered through the
normal project ignore policy. `net explain` relates each statically resolvable RPC
call to endpoints on the same receiver, including the receiver's multiplayer
subtree, recipient, effective RPC permission and delivery configuration, sender
identity reads, and the node path that must agree across peers. Autoload receivers
resolve to `/root/<name>`; scene receivers retain their scene-local path and source
scene. Calls with dynamic receivers stay explicitly unresolved.

Serialized `MultiplayerSpawner` and `MultiplayerSynchronizer` contracts include
spawn and synchronization roots, spawnable scenes and limits,
`SceneReplicationConfig` property modes, related replication nodes in the same
scene, and authority assignments made by scripts attached in that scene. Source
calls to `SceneTree.set_multiplayer` identify additional `MultiplayerAPI` subtree
roots; `/root` is always reported as the default context. Live peer state and
runtime-created nodes remain outside static inspection. Engine selection follows
the same precedence as `api` and `check`.

Run `gdkit cache refresh [PROJECT]` (or `gdkit import [PROJECT]`) after adding,
moving, or deleting files outside Godot. It stops any legacy gdkit background importer,
runs a headless editor import to completion, and persists Godot's UID,
script-class, and import caches without the resource-validation pass of `check`.
Engine selection follows `check`, including `--godot`. Move a script's `.uid`
sidecar with the script to preserve its identity. Refresh cannot recover an old
UID whose source metadata has been lost or repair arbitrary broken path references.
The editor may execute plugins and `@tool` scripts and create source sidecars.
Exit codes are 0 for a successful import, 1 for engine/import errors, and 2 for
setup failures. Refresh reports raw engine diagnostics, including errors that
`check.ignore_import_errors` would suppress during validation.

Use `gdkit cache rebuild [PROJECT]` when indexes are stale or corrupt. It removes
the UID, global script-class, scene-group, and editor filesystem indexes, then
imports with the configured engine. Imported assets and shaders remain available.
Use `gdkit cache clean [PROJECT] --dry-run` to preview a broader cleanup;
`gdkit cache clean [PROJECT]` also removes `.godot/imported` and
`.godot/shader_cache`, without importing afterward. Clean needs no engine.
Both operations preserve source `.uid` and `.import` sidecars, editor layouts,
and `.godot/gdkit` records, logs, and API/engine-probe caches. Every removed target
is printed. Deleted derived caches can be regenerated with `gdkit cache refresh`.
Close external editors before rebuilding or cleaning. Running gdkit sessions
block these operations; a legacy gdkit import worker is stopped automatically.
Cache mutations and checks share a project lock outside `.godot`. Cleanup
rejects cache symlinks and Windows reparse points instead of following them.
`check` does not use or delete the source project's derived caches.

`gdkit cache status [PROJECT]` reports cache presence and sizes without starting
Godot or requiring an engine configuration. Add `--output json` for a versioned
machine-readable inventory. Presence does not prove freshness.
`gdkit cache stop [PROJECT]` stops a background import editor left by an older gdkit;
`gdkit check --stop-worker` remains supported for compatibility.

Typical workflow after moving files:

```powershell
gdkit cache refresh game
gdkit check game
```

For a stale UID index, use `gdkit cache rebuild game`. Reserve `cache clean` for
discarding imported assets and shader caches as well. Rebuild preserves existing
resource identities; it does not assign fresh UIDs to every resource.

`check` copies the project to a disposable directory without `.godot` or `.git`,
uses the selected editor's dedicated import mode to populate the clean copy's
cache, then performs a complete editor filesystem scan, collects its GDScript
diagnostics, and loads every GDScript, scene, resource, and Godot shader outside
ignored and hidden directories. Reports and
artifacts remain associated with the source project, whose Godot caches are not
read or changed. It exits 1 when Godot reports an error or a resource fails to
load, and exits 2 for tooling failures such as a missing project, invalid
configuration, or incompatible engine. This checks resource loading, not gameplay
execution or a complete C# build.

Editor shutdown allocation messages from the disposable scan are retained in the
raw import artifacts but are not treated as project diagnostics. The scan records
an explicit completion boundary; GDScript, addon, and import errors emitted before
it remain validation failures, while later editor teardown output does not.

After import, `check` compares project-owned `class_name` declarations with
Godot's global script-class cache. A missing class, a class mapped to the wrong
script, a cache entry whose script was deleted, or an unreadable cache produces
a targeted diagnostic with the declaration location when available. Run
`gdkit cache refresh` first; use `gdkit cache rebuild` if the mismatch persists.

Use `gdkit check game --output json` to emit a versioned report on stdout, with
progress and human diagnostics on stderr. The report includes the outcome,
engine identity, project content fingerprint, phase coverage, structured
diagnostics with occurrence counts, failures, and artifact paths. Exit codes
remain 0 for success, 1 for validation failure or incomplete checks, and 2 for
tooling failures. Each check retains `report.json`, original phase stdout/stderr,
and ordered output events under `.godot/gdkit/checks/`.

Use `gdkit check game --strict-methods` to reject calls whose methods are not
guaranteed by the receiver's declared or inferred type, including calls in code
that never executes. To enable this for every check, set:

```toml
[check]
strict_methods = true
```

This enables Godot's `unsafe_method_access` diagnostic as an error in the fresh
resource-checking process, before autoloads and their dependencies load. It does
not edit `project.godot` or change the editor import's warning policy. Project
warning directory exclusions and explicit `@warning_ignore("unsafe_method_access")`
annotations still apply. Put a targeted annotation immediately before the call.
Other warning severities retain the project's policy, although the warning system
is enabled for strict checking even if the project disabled it globally.

A base-typed object can have a script or subclass with additional methods, so
strict checking can reject intentional dynamic calls too. Give the receiver its
actual custom type or explicitly suppress the diagnostic at the intentional call.
This is engine semantic validation, not a guarantee of runtime correctness.
Ordinary checks continue to use the project's warning policy. Human output gives
resource validation and runtime execution separate summary lines, then states the
validation policy. A default check reports `runtime execution: none requested`;
it does not imply that loaded scripts ran as tests or that gameplay scenes started.

Use `gdkit check game --scene res://scenes/match_lobby/match_lobby.tscn` to
add a headless runtime smoke check after resource validation succeeds. Repeat
`--scene` for multiple scenes. Paths resolve relative to the project and must
identify a scene inside it. Each scene starts in a fresh game process with the
project's autoloads and normal runtime warning policy. This executes gameplay,
including `_ready()`, and can perform whatever I/O the project normally performs.
Use a project-owned fixture scene when a lobby needs identity or network setup.

`--smoke-frames 2` sets the number of process iterations before Godot quits
(default 2). `--smoke-timeout 30` sets the wall-clock limit in seconds per scene
(default 30, maximum 3600), including startup. A timeout, unsuccessful exit, or
`ERROR:` / `SCRIPT ERROR:` on either stream fails the check, even when Godot
otherwise exits 0. On Windows the timeout terminates the launched process tree,
including the console launcher's engine process. Output is captured to temporary
files so a verbose or blocked scene cannot fill a pipe and deadlock the runner.
The frame budget only covers startup; smoke success does not validate later
interactions, multiplayer behavior, or rendering correctness. Resource-validation
failures skip scene execution, and the coverage line reports the number run.

Use `--script res://tests/contract.gd` to run a project-owned `SceneTree` script
after resource validation. Repeat the option to run multiple scripts. Each script
runs in a fresh headless process and must terminate itself with an appropriate
exit status. `--script-timeout 30` controls the per-script deadline. Failures,
timeouts, engine diagnostics, and original output use the same report and
artifact contract as the rest of the check.

The disposable copy is removed after every check. Symbolic links are rejected
rather than followed across the copy boundary. All selected resources load in a
fresh process after the editor scan, sharing dependencies within that single pass;
script files load before scenes and resources to support cyclic preloads. There
is no cached pass result or persistent GDScript analyzer.

`gdkit check --fresh` and `gdkit check --isolated` remain accepted as hidden
compatibility options, but no longer change behavior. `gdkit check --stop-worker`
and `gdkit cache stop` can release an import editor left by an older gdkit version.

Use `gdkit check --timings` to print file scan, engine validation (cached or
probed), clean editor import, resource loading, and total elapsed times to stderr.
Project import and resource loading run on every check.

The final summary explicitly says `check passed` or `check failed`, colored green
or red in a terminal, and labels the scripts, scenes, and resources as loaded by
resource validation. A separate runtime line reports requested and executed
project scripts and gameplay scene smoke checks, including skips caused by an
earlier failure. Redirected output is plain text by default; `NO_COLOR` disables
color.

Diagnostics are grouped by import and resource loading, with shutdown allocation
reports collected separately after other messages. Cleanup `ERROR` entries still
fail the check; grouping does not change exit status. Unrecognized resource UIDs
include an explanation and matching references with line numbers from checked
text files and `project.godot`. Matches are repair candidates, not proof of which
reference caused the error. Engine-internal `at:` stack frames are omitted from
the default display; project source locations remain visible. Use `gdkit check
--verbose` to also print the full captured Godot output.

Engine selection does not change the syntax supported by the gdview formatter.

## Named runtime sessions

`gdkit run --name client` launches the configured engine and returns immediately.
Windowed play is the default; `--headless` is explicit. Supply `--scene
res://path/to/scene.tscn` to override the main scene, and place project arguments
after `--`, for example `gdkit run --name server --headless -- --server`.

Each launch creates an immutable generation record under
`.godot/gdkit/sessions`, containing the project, engine and version, scene,
arguments, launch mode, PID, process creation identity, and combined log path.
The creation identity prevents `stop` from targeting an unrelated process if an
old PID has been reused. Session selectors accept either the latest generation
by name or an exact `name@generation` printed by `run`.

Use `gdkit sessions` to list the latest generation of each name and its current
status. `--all` includes superseded generations. `gdkit logs <session>` prints
the retained combined log, `gdkit stop <session>` stops a matching live process,
and `gdkit restart <session>` launches the recorded engine, scene, arguments,
and mode as a new generation. These commands only operate on records belonging
to the selected project.

`gdkit inspect server --net` takes a fresh, read-only multiplayer observation
from a live named session. An exact generation selector such as `server@abc`
is also accepted, and `--output json` emits the typed response. Each sample
identifies the immutable session generation, wall-clock collection time,
process and physics ticks, concrete peer and connection state, SceneMultiplayer
roots and settings, node authority, live spawners and synchronizers, and the
recent connection and RPC event ring. RPC counts and byte totals come from the
engine's built-in multiplayer profiler through a loopback debugger relay. The
loopback probe accepts only the versioned network-observation request, checks
its generation, token, deadline,
and response bound, and does not expose expression evaluation or mutation.

Named runs use a generated `SceneTree` launch wrapper under
`.godot/gdkit/sessions/probe` so observation begins before the main scene loads.
The wrapper preserves project arguments and loads the selected or configured
main scene after its loopback endpoint is ready. Sessions created by an older
gdkit build must be restarted before they can be inspected.

Projects can declare the script that gives domain meaning to live checkpoints:

```toml
[inspect]
checkpoint_adapter = "res://tools/gdkit_checkpoints.gd"
```

The GDScript must define `collect_checkpoints(tree: SceneTree) -> Dictionary`.
Its top-level keys name stable checkpoints and each value must be a dictionary,
for example `network_session`, `lobby_state`, `round_state`, and
`local_player_state`. The project owns the names and values in this adapter.
Declaring an adapter does not execute it during `check`, `doctor`, or normal
game startup.

`gdkit inspect server --checkpoints` invokes the adapter inside that exact live
session and prints the returned dictionaries. It records the session generation,
adapter path, collection time, process and physics ticks, and adapter duration.
`--net --checkpoints` collects both generic network state and project checkpoints.
JSON output retains the dictionary values without assigning them gdkit-defined
meaning.

Checkpoint values may contain only JSON scalars, arrays, and string-keyed
dictionaries. Collection accepts at most 32 checkpoints, 2,048 aggregate
dictionary or array entries, eight levels of nesting, and 16 KiB per string or
key, inside the existing 1 MiB response limit. Contract and limit failures are
reported as a bounded checkpoint error and exit with status 1.

`gdkit inspect server --checkpoints --compare client` collects both live
generations sequentially and compares their declared values. Results identify
both session generations and capture ticks, include the wall-clock capture skew,
and report recursive differences by JSON Pointer path. Matching checkpoints exit
0; differences or collection errors exit 1. A comparison emits at most 2,048
differences, and JSON output includes both original bounded captures.

Named multiplayer scenarios build on durable sessions and project checkpoints.
Declare them in `gdkit.toml` with an explicit transport, named ports, participant
roles, launch arguments, and a JSON Pointer readiness condition:

```toml
[scenarios.late_join]
transport = "dedicated_enet"
timeout_seconds = 20
ports = { game = { checkpoint = "/network_session/port" } }

[[scenarios.late_join.participants]]
name = "server"
role = "server"
arguments = ["--server", "--port={port.game}"]
readiness = { path = "/network_session/listening", equals = true }

[[scenarios.late_join.participants]]
name = "client-1"
role = "client"
arguments = ["--enet-client", "--port={port.game}"]
readiness = { path = "/network_session/connected", equals = true }

[[scenarios.late_join.participants]]
name = "client-2"
role = "client"
arguments = ["--enet-client", "--port={port.game}"]
readiness = { path = "/network_session/connected", equals = true }

[[scenarios.late_join.participants]]
name = "late-client"
role = "late_client"
arguments = ["--enet-client", "--port={port.game}"]
readiness = { path = "/network_session/connected", equals = true }
```

Run it with `gdkit scenario start late_join`. The server starts and becomes ready
first, regular clients start as a group, and late clients start only after all
regular clients are ready. Dynamic ports declare the server checkpoint that
reports the bound port. gdkit passes `0` to the server, waits for readiness,
reads the OS-selected port from that checkpoint, and only then expands client
arguments. A fixed nonzero port may use the shorthand
`ports = { game = 7000 }`. Arguments may use
`{port.NAME}`, `{participant}`, and `{user_data_dir}`. Every participant receives
its own user-data roots, durable log, session generation, and the environment
variables `GDKIT_SCENARIO`, `GDKIT_SCENARIO_PARTICIPANT`,
`GDKIT_SCENARIO_ROLE`, `GDKIT_SCENARIO_TRANSPORT`, and
`GDKIT_SCENARIO_PORT_NAME`.

The only accepted transports are `dedicated_enet` and `steam_p2p`; transport is
never inferred from arguments and one is never substituted for the other.
Dedicated ENet scenarios require exactly one server and at least one named port.
Steam P2P scenarios reject dedicated-server participants. Both require an
explicit late client. Use `gdkit scenario status NAME`, `disconnect NAME
PARTICIPANT`, `crash NAME PARTICIPANT`, and `stop NAME` to control the latest
run. Run records are created before the first participant launches and retain
the first startup failure, resolved ports, and each launched participant's
readiness state after cleanup. Disconnect requests orderly engine shutdown and
reports failure without falling back to a forced crash; crash is the explicit
force-termination command.

`autoloads` finds the nearest enclosing Godot project and prints its saved
autoload initialization order with zero-based indices, names, singleton status,
and decoded resource paths. It reads `project.godot` through gdview without
loading the project or checking whether referenced resources exist. An optional
path can select another project or a location inside one:

```sh
gdkit autoloads /path/to/project
```

To tolerate a known third-party editor plugin error, add a narrowly scoped import
exception to the project's `gdkit.toml`:

```toml
[[check.ignore_import_errors]]
message = "ERROR: Script inherits from native type 'MarginContainer', so it can't be assigned to an object of type 'FoldableContainer'."
source = "res://addons/godotsteam_server/godotsteam_plugin.gd"
```

The entire trimmed error message must match, and its attached Godot stack trace
must contain the exact source path. Only import diagnostics are eligible; resource
loading failures, other errors, and unsuccessful process exits still fail checks.
Matches are counted in normal output; `--verbose` preserves the original output.
Rules apply to both fresh imports and cached worker diagnostics, and are reread
on every check. This does not disable the plugin or exclude its files from Godot.

`format-project` and the resource-loading phase of `check` use gdview's shared
file enumeration with gdkit's Git-aware filtering policy. Extensions are matched
case-insensitively. Parent `.gitignore` files (including monorepo roots), nested ignore
files, `!` exceptions, `.git/info/exclude`, and global Git ignore rules are
respected. `.gitignore` rules also work without an initialized Git repository.
Matching is path-based, including for tracked files; gdkit does not consult the
Git index. Hidden paths, directories containing `.gdignore`, and symlinks are
skipped. Ignored directories are pruned before scanning their contents.

Explicit `format <file>` and `scene-tree <file>` requests still inspect the named
file; scene expansion still follows its dependencies. Godot's own import pass
and dependency loading do not obey Git ignore rules. An ignored resource needed
by the project can therefore still be loaded by Godot, and engine errors still
fail the check. Use `.gdignore` for directories Godot itself should exclude.

## Test engine

The [poweruser roadmap](docs/poweruser-roadmap.md) documents the researched
direction for diagnostics, runtime sessions, API queries, scene intelligence,
and repeatable multiplayer scenarios. Proposed commands there are not yet APIs.

Godot 4.7.2 is pinned in `godot.lock.json` only as a reproducible test dependency.
Normal `init` and `check` operation never reads this lock. On Windows,
provision the official editor into the repository-local `.tools` directory with:

```powershell
pwsh -File scripts/provision-godot.ps1
```

The provisioner verifies the archive checksum and engine version, enables Godot's
self-contained mode, and prints the path to the console executable. Re-running it
reuses an installation that still matches the lock.

Run the engine association and checker integration fixtures against any selected
editor with:

```powershell
$env:GDKIT_TEST_GODOT = 'P:/path/to/godot.console.exe'
cargo test --test check -- --include-ignored
cargo test --test import_worker -- --include-ignored
```

`scene-tree` reads a Godot text scene and prints its literal node hierarchy without
loading the project or running Godot. Scene parsing, expansion, and tree rendering
are provided by `gdview`, re-exported as `gdkit::scene` for library compatibility.
The structured `gdkit::scene::expand(path, depth)` API also exposes the expanded
hierarchy without rendering it. CLI arguments and output handling remain in gdkit.
Native types, attached scripts, scene
instances, and owner-unique names are included while serialized properties and
resource contents are omitted. Nodes inherited from a base scene are identified
when their type is not present in the text scene. `--expand` recursively resolves
packed scene instances and inherited base scenes, while `--expand-depth` limits
resolution to a specific number of instance edges. Expanded nodes include their
origin scene. Expansion depths are capped at 64, cycles are rejected, and
`res://` paths are resolved from the nearest ancestor containing `project.godot`.
Expansion reports invalid project markers and project-discovery I/O failures;
relative scene references still work without an enclosing project.
Resource paths and optional enclosing-project discovery use gdview's shared APIs.
UID-only references cannot be resolved without a UID registry; runtime-specific
schemes such as `user://` are rejected. Engine commands retain explicit-root
validation rather than searching parent directories.

Use `--connections` to show outgoing signal connections beneath each source node,
and `--groups` to show saved group memberships. Both switches can be combined
with each other and with instance expansion:

```sh
gdkit scene-tree player.tscn --connections
gdkit scene-tree player.tscn --groups
gdkit scene-tree player.tscn --expand --connections --groups
```

Connection targets use the root name, `%Name` for known owner-unique nodes, or
the scene-relative node path. Saved flags, binds, and unbinds appear when present.
Signal argument names are not stored in scene connection records, so signatures
are not inferred from scripts. Connections and groups created at runtime are not
included. With expansion, saved memberships are merged and connection targets
are resolved within their instance. Sources absent from the literal hierarchy
are shown as inherited placeholders when connections are requested.

File inputs are replaced atomically in place. Omit the path or use `-` to read
stdin and write the formatted source to stdout. `--check` does not write and exits
1 for changes; errors exit 2. Line width defaults to 100 columns (`--line-width`);
tabs count to the next multiple of 4. Calls that exceed the limit expand to one
argument per line with a trailing comma. Single simple statements compact onto
their suite header when the result fits. Comments, semicolons, multiline bodies,
and multi-statement suites stay expanded. The ignored test needs Godot 4.7.2.

Run `format-project` from a directory containing `project.godot` to recursively
format every non-ignored `.gd` file in that project. Symlinked files and directories
are not followed. All selected scripts are validated before any files are changed. Its `--check`
mode exits 1 if any script would change without writing to the project.

`formatter::format_source` returns `Result<String, syntax::SyntaxError>`, reporting
the first parser diagnostic with its byte range. It expects a complete script;
statement fragments must be placed inside a function. Suite selection uses syntax
blocks and statement nodes. No legacy lexer or fallback formatting path remains.

Formatting uses tabs for indentation, removes trailing whitespace and outer blank
lines, normalizes spacing around operators, commas, type annotations, calls,
collections, and inline comments, and collapses extra blank lines inside
functions. Long calls and any nested calls that remain over width are wrapped
recursively as the terminal formatting phase, after suite compaction and other
rewrites. Multiline arrays, dictionaries, and enums receive trailing commas;
single-line collections do not retain them. Functions have two blank lines between
them and after preceding fields. Field categories have one blank line
between them, including public/private and static/instance boundaries. Existing
single blank lines within a category preserve semantic groups.

Consecutive fields are stably ordered: constants, static variables, exports,
regular variables, then onready variables, with public names before private names.
Attached comments and annotations move with fields. Script and inner-class
documentation remains attached to its class.
Functions, other declarations, and export group/category annotations act as sorting
boundaries. Ordering can change initializer execution order across categories;
within each category, declaration order is preserved. String contents and existing
line endings are preserved. Missing final newlines remain missing unless field
reordering requires a line separator. Invalid input is rejected, including mixed
indentation diagnosed by the parser.

Formatting can be disabled for a region without weakening syntax validation:

```gdscript
# gdkit: off
var deliberately   =   spaced
# gdkit: on
```

## Syntax parser

`gdkit::syntax` re-exports the lossless GDScript frontend from
[gdview](https://github.com/xandwr/gdview), pinned to a Git revision in `Cargo.toml`.
`gdkit::syntax::parse` returns a source-backed concrete syntax tree and byte-ranged
diagnostics, including on malformed input.

```rust
use gdkit::syntax::{ast::{AstNode, Function}, parse};

let source = "func greet(name: String): return name\n";
let parsed = parse(source);
assert!(parsed.is_valid());
assert_eq!(parsed.root().text(), source);

for function in parsed.root().children().filter_map(Function::cast) {
    println!("{:?}", function.name());
}
```

The source must outlive its parse result. Node and typed AST views borrow the
result, so they cannot outlive the tree. Tokens reference UTF-8 byte spans in the
original source; comments, whitespace, spelling, and line endings are retained.
`root().tokens()` walks the actual tree in source order, including zero-width
layout markers. `root().descendants()` includes the root. `debug_tree()` renders
node kinds, spans, and token text.

The gdview implementation provides its lexer, indentation handling, recursive-descent/Pratt
parser, and arena tree. Declarative macros generate token metadata, operator
classification, typed node wrappers, and the `ast::Expression` enum. Expression
wrapping uses forward links instead of inserting into an event vector. Tree
traversal and destruction do not recurse through nested nodes. Project formatting
also uses gdview to validate the current project root and enumerate files.
Formatting rules, file-discovery policy, and file writes remain in gdkit;
gdview owns traversal for both project commands.

Coverage includes declarations, abstract signatures, typed variadic parameters,
property accessors, nested classes, annotations, match patterns and guards,
typed containers, lambdas, node paths, and expressions. Tests verify acceptance
of all 368 feature/warning scripts (including helper scripts) in the parser,
analyzer, and runtime directories of the local Godot `4.7.2-stable` corpus.
Error and LSP fixtures are also checked for lossless reconstruction; completion
fixtures are excluded because they use special cursor markers.

```sh
cargo test --test syntax
GODOT_SOURCE=/path/to/godot cargo test --test syntax_corpus -- --ignored --nocapture
```

This is syntax tooling, not Godot's semantic analyzer. An empty diagnostic list
does not establish engine validity: name resolution, types, annotation targets,
and other contextual restrictions are not fully validated. Unicode lexical edge
cases and exact diagnostic parity also need further conformance work. Recursive
parsing is limited to 128 nested parser calls; exceeding that limit produces a
diagnostic while retaining the input. Corpus acceptance does not prove full
language coverage. The formatter uses this parser as its only syntax frontend.

Upstream revision and attribution are recorded in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
