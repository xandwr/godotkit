# godotkit

A Rust formatter targeting GDScript 4.7.2. Currently compacts bare-return `if`
guards using the owned lossless syntax tree; preserves other formatting. Inputs
with parser diagnostics are rejected before producing output or writing files.

```sh
cargo run -- format script.gd
cargo run -- format script.gd --write
cargo run -- format script.gd --check
cargo test
cargo test --test godot -- --ignored
GODOT_SOURCE=/path/to/godot cargo test --test corpus -- --ignored
```

Omit the path to read stdin. Output goes to stdout unless `--write` or `--check`
is used. Check exits 1 for changes; errors exit 2. Guard width defaults to 100
columns (`--line-width`); tabs count to the next multiple of 4. Guards with
comments or multiline conditions stay unchanged. The ignored test needs Godot 4.7.2.

`formatter::format_source` returns `Result<String, syntax::SyntaxError>`, reporting
the first parser diagnostic with its byte range. It expects a complete script;
statement fragments must be placed inside a function. Guard selection uses syntax
blocks and return statements, then replaces only the whitespace between the
header colon and bare return. No legacy lexer or fallback formatting path remains.

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
