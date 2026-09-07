# godotkit

A Rust formatter targeting GDScript 4.7.2. Currently compacts bare-return `if`
guards; preserves other formatting. Full syntax validation is not implemented.

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
