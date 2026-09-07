use godotkit::{
    formatter::{Options, format_source},
    syntax::parse,
};

fn script(body: &str, indent: &str) -> String {
    if body.starts_with("func ") {
        return body.to_owned();
    }
    let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let mut source = format!("func f():{newline}");
    for line in body.split_inclusive('\n') {
        source.push_str(indent);
        source.push_str(line);
    }
    source
}

fn check(before: &str, after: &str) {
    let indent = if before.contains('\t') { "\t" } else { "    " };
    let before = script(before, indent);
    let after = script(after, indent);
    let before = before.as_str();
    let formatted = format_source(before, &Options::default()).unwrap();
    assert_eq!(formatted, after);
    assert_eq!(
        format_source(&formatted, &Options::default()).unwrap(),
        formatted
    );
    assert!(parse(&formatted).is_valid());
}

#[test]
fn compacts_guards_at_nested_indentation() {
    check(
        "func f():\n\tif ready:\n\t\treturn\n\twork()\n",
        "func f():\n\tif ready: return\n\twork()\n",
    );
    check(
        "func f():\n    if outer:\n        if inner:\n            return\n    work()\n",
        "func f():\n    if outer:\n        if inner: return\n    work()\n",
    );
}

#[test]
fn preserves_line_endings_and_missing_final_newline() {
    check("if ready:\r\n\treturn\r\n", "if ready: return\r\n");
    check("if ready:\n    return", "if ready: return");
}

#[test]
fn preserves_comments_and_other_statements() {
    for source in [
        "if ready: # reason\n    return\n",
        "if ready:\n    return # reason\n",
        "if ready:\n    # reason\n    return\n",
        "if ready:\n    return\n    # reason\n",
        "if ready:\n    return\n    work()\n",
        "if ready:\n    return value\n",
        "if ready:\n    break\n",
        "if ready:\n\n    return\n",
        "if ready: return\n",
        "if ready:\n    return;\n",
    ] {
        check(source, source);
    }
}

#[test]
fn protects_strings_and_continuations() {
    for source in [
        "var text = \"\"\"\nif ready:\n    return\n\"\"\"\n",
        "var text = '''\nif ready:\n    return\n'''\n",
        "var text = r\"escaped \\\" quote\"\n",
        "var text = \"日本語 😀 # []\"\n",
        "if (\n    ready\n):\n    return\n",
        "if ready \\\n    and waiting:\n    return\n",
    ] {
        check(source, source);
    }
    check(
        "if text == \"#:\":\n    return\n",
        "if text == \"#:\": return\n",
    );
}

#[test]
fn handles_adjacent_guards_and_else() {
    check(
        "if a:\n    return\nif b:\n    return\n",
        "if a: return\nif b: return\n",
    );
    check(
        "if a:\n    return\nelse:\n    work()\n",
        "if a: return\nelse:\n    work()\n",
    );
}

#[test]
fn respects_visual_width_with_tabs() {
    let source = "func f():\n\tif ready:\n\t\treturn\n";
    assert_eq!(
        format_source(
            source,
            &Options {
                line_width: 20,
                tab_width: 4
            }
        )
        .unwrap(),
        "func f():\n\tif ready: return\n"
    );
    assert_eq!(
        format_source(
            source,
            &Options {
                line_width: 19,
                tab_width: 4
            }
        )
        .unwrap(),
        source
    );
}

#[test]
fn reports_syntax_errors_without_panicking() {
    for source in ["var x = \"unterminated", "var x = [)", "var x = ("] {
        assert!(
            format_source(source, &Options::default()).is_err(),
            "{source}"
        );
    }
    for source in ["", "\n", "# trailing", "var π = 3.14\n"] {
        assert_eq!(format_source(source, &Options::default()).unwrap(), source);
    }
}

#[test]
fn preserves_newlines_inside_single_quoted_strings() {
    for source in [
        "var x = r\"first\nif ready:\n    return\nlast\"\n",
        "var x = \"first\nlast\"\n",
    ] {
        check(source, source);
    }
}

#[test]
fn dedented_comments_do_not_end_a_suite() {
    let source = "if ready:\n    return\n# explanation\n    work()\n";
    check(source, source);
    check(
        "if ready:\n    return\n# explanation\nwork()\n",
        "if ready: return\n# explanation\nwork()\n",
    );
}

#[test]
fn rejects_invalid_syntax_even_after_a_valid_guard() {
    for source in [
        "if ready:\n    return\n",
        "func f():\n    if :\n        return\n",
        "func f():\n    if ready:\n        return\n    var value =\n",
        "func f():\n    if ready:\n        return\n  pass\n",
        "var value = \"bad\\q\"\n",
    ] {
        let parsed = parse(source);
        let error = format_source(source, &Options::default()).unwrap_err();
        assert_eq!(&error, &parsed.errors()[0]);
    }
}

#[test]
fn preserves_branch_boundaries_and_statement_separators() {
    check(
        "if a:\n    return\nelif b:\n    return\nelse:\n    if c:\n        return\n",
        "if a: return\nelif b:\n    return\nelse:\n    if c: return\n",
    );
    for source in [
        "if a:\n    return; work()\n",
        "if a:\n    return\n    ;\n",
        "if a: return; work()\n",
        "if a:\n    return\n    ## explanation\n",
        "if a:\n    return\n    #region explanation\n",
    ] {
        check(source, source);
    }
    check("if a:  \n    return  \n", "if a: return  \n");
}

#[test]
fn formats_guards_in_lambdas_and_match_arms() {
    check(
        "var callback = func():\n    if ready:\n        return\ncallback.call()\n",
        "var callback = func():\n    if ready: return\ncallback.call()\n",
    );
    check(
        "match value:\n    1:\n        if ready:\n            return\n    _:\n        pass\n",
        "match value:\n    1:\n        if ready: return\n    _:\n        pass\n",
    );
}
