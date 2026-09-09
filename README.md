# godotkit

A Rust formatter targeting GDScript 4.7.2. Normalizes whitespace and orders fields using the owned lossless syntax tree,
and compacts bare-return `if` guards. Inputs
with parser diagnostics are rejected before producing output or writing files.

```sh
cargo run -- format script.gd
cargo run -- format script.gd --check
cargo run -- format - < script.gd
cargo run -- format-project
cargo run -- format-project --check
cargo run -- scene-tree scene.tscn
cargo run -- scene-tree scene.tscn --expand
cargo run -- scene-tree scene.tscn --expand-depth 2
cargo test
cargo test --test godot -- --ignored
GODOT_SOURCE=/path/to/godot cargo test --test corpus -- --ignored
```

`scene-tree` reads a Godot text scene and prints its literal node hierarchy without
loading the project or running Godot. Native types, attached scripts, scene
instances, and owner-unique names are included while serialized properties and
resource contents are omitted. Nodes inherited from a base scene are identified
when their type is not present in the text scene. `--expand` recursively resolves
packed scene instances and inherited base scenes, while `--expand-depth` limits
resolution to a specific number of instance edges. Expanded nodes include their
origin scene. Expansion depths are capped at 64, cycles are rejected, and
`res://` paths are resolved from the nearest ancestor containing `project.godot`.

Use `--connections` to show outgoing signal connections beneath each source node,
and `--groups` to show saved group memberships. Both switches can be combined
with each other and with instance expansion:

```sh
godotkit scene-tree player.tscn --connections
godotkit scene-tree player.tscn --groups
godotkit scene-tree player.tscn --expand --connections --groups
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
format every `.gd` file in that project. Symlinked files and directories are not
followed. All scripts are validated before any files are changed. Its `--check`
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
# godotkit: off
var deliberately   =   spaced
# godotkit: on
```

## Syntax parser

`godotkit::syntax::parse` provides a local, lossless GDScript parser adapted from
`gdscript-syntax`. It returns a source-backed concrete syntax tree and byte-ranged
diagnostics, including on malformed input.

```rust
use godotkit::syntax::{ast::{AstNode, Function}, parse};

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

The implementation owns its lexer, indentation handling, recursive-descent/Pratt
parser, and arena tree. Declarative macros generate token metadata, operator
classification, typed node wrappers, and the `ast::Expression` enum. Expression
wrapping uses forward links instead of inserting into an event vector. Tree
traversal and destruction do not recurse through nested nodes. `unicode-ident`
is the only additional runtime dependency, supplying Unicode identifier tables.

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
