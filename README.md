# gdkit

A Rust formatter targeting GDScript 4.7.2. Normalizes whitespace and orders fields using gdview's lossless syntax tree,
and compacts bare-return `if` guards. Inputs
with parser diagnostics are rejected before producing output or writing files.

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
cargo run -- check /path/to/project --isolated --script res://tests/contract.gd
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
engine version, selection source, compatibility-probe cache health, import-worker
state, effective check warning policy, and likely project-specific gotchas. Like
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
```

The command currently covers GDScript and `.tscn` files discovered through the
normal project ignore policy. It reports serialized replication node identity,
but arbitrary replication properties and live peer state remain future project
graph and runtime-inspection work. Engine selection follows the same precedence
as `api` and `check`.

`check` runs the selected editor headlessly, imports the project, and loads every
GDScript, scene, resource, and Godot shader outside ignored and hidden directories.
Importing can update the project's Godot caches. It exits 1 when Godot reports an
error or a resource fails to load, and exits 2 for tooling failures such as a
missing project, invalid configuration, or incompatible engine. This checks
resource loading, not gameplay execution or a complete C# build.

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
not edit `project.godot` or change the import worker's warning policy. Project
warning directory exclusions and explicit `@warning_ignore("unsafe_method_access")`
annotations still apply. Put a targeted annotation immediately before the call.
Other warning severities retain the project's policy, although the warning system
is enabled for strict checking even if the project disabled it globally.

A base-typed object can have a script or subclass with additional methods, so
strict checking can reject intentional dynamic calls too. Give the receiver its
actual custom type or explicitly suppress the diagnostic at the intentional call.
This is engine semantic validation, not a guarantee of runtime correctness.
Ordinary checks continue to use the project's warning policy. The coverage line
states which policy was checked and whether gameplay scenes were entered.

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

Use `gdkit check --isolated` to copy the project, excluding `.godot` and `.git`,
to a temporary directory and perform a fresh check there. Engine selection,
configuration, reports, and retained artifacts remain associated with the source
project. The temporary copy is removed after all processes finish. Symbolic links
are rejected rather than followed across the isolated boundary.

By default, the import editor stays alive for five minutes after its last request.
Subsequent checks ask it to scan again and update changed script classes, avoiding
editor startup after ordinary script edits. Every check still loads all selected
resources in a **fresh process**, sharing dependencies within that single pass;
script files load before scenes and resources to support cyclic preloads. There
is no cached pass result or persistent GDScript analyzer in the resource checker.

The worker restarts when the engine, worker implementation, non-script files,
addon scripts, or `@tool` scripts change. Changes after an import error also
restart it so errors from editor startup can be reevaluated. Unchanged import
errors remain visible and continue to fail checks. New assets, settings changes,
and cold startup can therefore still take seconds. Script contents are hashed
to detect edits even when modification timestamps are unchanged.

Use `gdkit check --fresh` for a complete import and resource check with new
processes, including import-editor shutdown diagnostics. This also stops any
existing worker. `gdkit check --stop-worker` stops the project's worker without
checking, releasing its memory and file handles immediately. Worker state and
logs live under `.godot/gdkit`; its authenticated socket listens only on localhost.
Concurrent import requests are serialized with a project lock. Unsupported
worker APIs or project symlinks fall back to fresh importing.

Use `gdkit check --timings` to print file scan, engine validation (cached or
probed), import (worker startup or warm worker), resource loading, and total
elapsed times to stderr. Project import and resource loading still run on every
check. On a local Pill Poppers copy with 45 scripts, 8 scenes, and 29 resources,
warm checks measured roughly 490 ms wall time; this is a sample, not a guarantee
for every project or cold/import-changing check. Successful ordinary script-edit
checks measured 489-499 ms with the existing broken Steam editor plugin disabled
only in the benchmark copy; all 82 selected files were still checked.

The final summary explicitly says `check passed` or `check failed`, colored green
or red in a terminal. Redirected output is plain text by default; `NO_COLOR`
disables color.

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
1 for changes; errors exit 2. Guard width defaults to 100 columns (`--line-width`);
tabs count to the next multiple of 4. Guards with comments or multiline conditions
stay unchanged. The ignored test needs Godot 4.7.2.

Run `format-project` from a directory containing `project.godot` to recursively
format every non-ignored `.gd` file in that project. Symlinked files and directories
are not followed. All selected scripts are validated before any files are changed. Its `--check`
mode exits 1 if any script would change without writing to the project.

`formatter::format_source` returns `Result<String, syntax::SyntaxError>`, reporting
the first parser diagnostic with its byte range. It expects a complete script;
statement fragments must be placed inside a function. Guard selection uses syntax
blocks and return statements. No legacy lexer or fallback formatting path remains.

Formatting uses tabs for indentation, removes trailing whitespace and outer blank
lines, normalizes spacing around operators, commas, type annotations, calls,
collections, and inline comments, and collapses extra blank lines inside
functions. Multiline arrays, dictionaries, and enums receive trailing commas;
single-line collections do not retain them. Functions have two blank lines
between them and after preceding fields. Field categories have one blank line
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
