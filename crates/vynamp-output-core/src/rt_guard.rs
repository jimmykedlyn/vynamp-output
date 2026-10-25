//! Static guard: the real-time modules must not contain allocating, locking
//! or panicking constructs. Task 4 appends `driver.rs` and `rtflags.rs` to
//! [`RT_MODULES`].

/// RT module paths, relative to the crate root.
pub(crate) const RT_MODULES: &[&str] = &["src/render.rs", "src/convert.rs", "src/silence.rs"];

/// Tokens that must not appear in RT code.
pub(crate) const FORBIDDEN: &[&str] = &[
    "Vec::new", "push", "Box::new", "String", "format!", "println!", "Mutex", "lock()", "unwrap()",
    "expect(",
];

/// Returns the RT-relevant code of `src`: everything before the first
/// `#[cfg(test)]` line (test modules go last, by convention), with `//`
/// comments (including doc comments) removed. Block comments are not
/// supported; RT modules do not use them.
pub(crate) fn rt_code(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        out.push_str(line.split("//").next().unwrap_or(""));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_test_modules() {
        let src = "let a = 1; // push\n/// String docs\nlet b = 2;\n#[cfg(test)]\nmod t { push }\n";
        assert_eq!(rt_code(src), "let a = 1; \n\nlet b = 2;\n");
    }

    #[test]
    fn rt_modules_have_no_forbidden_tokens() {
        let root = env!("CARGO_MANIFEST_DIR");
        for module in RT_MODULES {
            let path = std::path::Path::new(root).join(module);
            let src = std::fs::read_to_string(&path).expect("read RT module");
            let code = rt_code(&src);
            assert!(!code.trim().is_empty(), "{module}: empty after stripping");
            for token in FORBIDDEN {
                assert!(!code.contains(token), "{module}: forbidden token {token:?}");
            }
        }
    }
}
