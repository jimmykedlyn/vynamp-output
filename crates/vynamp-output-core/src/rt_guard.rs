//! Static guard: the real-time modules must not contain allocating, locking,
//! blocking or panicking constructs.
//!
//! The scanner is a small lexer, not a parser: it removes comments (line,
//! doc and nested block comments) and blanks string/char literal contents,
//! then cuts `#[cfg(test)] mod name { … }` blocks by brace matching. Any other
//! `#[cfg(test)]` item in an RT module is an error, so test-only code cannot
//! hide outside a test module. Tokens are matched on identifier boundaries
//! (`block()` does not match `lock()`, `pushed` does not match `push`).
//!
//! Panicking slice indexing (`x[i]`) is not banned by token (the bracket also
//! appears in types, arrays and patterns); RT modules are written with `get`,
//! iterators and `chunks` instead, which review checks.

/// RT module paths, relative to the crate root.
pub(crate) const RT_MODULES: &[&str] = &[
    "src/render.rs",
    "src/convert.rs",
    "src/silence.rs",
    "src/driver.rs",
    "src/rtflags.rs",
];

/// Tokens that must not appear in RT code.
pub(crate) const FORBIDDEN: &[&str] = &[
    "Vec::new",
    "Vec::with_capacity",
    "vec!",
    "to_vec",
    "to_string",
    "push",
    "push_str",
    "Box::new",
    "Arc::new",
    "String",
    "format!",
    "println!",
    "eprintln!",
    "panic!",
    "unreachable!",
    "todo!",
    "Mutex",
    "RwLock",
    "lock()",
    "unwrap()",
    "expect(",
    ".collect",
    "sleep",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// At `i` (`r` or `br`), returns `(hash count, index after the opening quote)`
/// when a raw string starts there.
fn raw_string_start(c: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i + if c.get(i) == Some(&'b') { 2 } else { 1 };
    let mut hashes = 0;
    while c.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    (c.get(j) == Some(&'"')).then_some((hashes, j + 1))
}

/// Skips an escaped literal body starting at `i` up to and including `quote`.
fn skip_quoted(c: &[char], mut i: usize, quote: char) -> usize {
    while let Some(&ch) = c.get(i) {
        if ch == quote {
            return i + 1;
        }
        i += if ch == '\\' { 2 } else { 1 };
    }
    i
}

/// Removes comments and blanks literal contents (`"…"` becomes `""`, `'x'`
/// becomes `''`). Raw strings (`r"…"`, `r#"…"#`, `br"…"`) are handled;
/// lifetimes (`'a`) are kept.
fn strip_comments_and_literals(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while let Some(&ch) = c.get(i) {
        let next = c.get(i + 1).copied();
        let prev_ident = i > 0 && c.get(i - 1).copied().is_some_and(is_ident);
        if ch == '/' && next == Some('/') {
            while c.get(i).is_some_and(|&x| x != '\n') {
                i += 1;
            }
        } else if ch == '/' && next == Some('*') {
            let mut depth = 0usize;
            while i < c.len() {
                if c.get(i) == Some(&'/') && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if c.get(i) == Some(&'*') && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(' ');
        } else if ch == '"' {
            i = skip_quoted(&c, i + 1, '"');
            out.push_str("\"\"");
        } else if let Some((hashes, body)) = (!prev_ident && (ch == 'r' || ch == 'b'))
            .then(|| raw_string_start(&c, i))
            .flatten()
            .filter(|_| ch == 'r' || next == Some('r'))
        {
            i = body;
            while i < c.len() {
                if c.get(i) == Some(&'"') && (1..=hashes).all(|k| c.get(i + k) == Some(&'#')) {
                    i += 1 + hashes;
                    break;
                }
                i += 1;
            }
            out.push_str("\"\"");
        } else if ch == '\'' && (next == Some('\\') || c.get(i + 2) == Some(&'\'')) {
            i = skip_quoted(&c, i + 1, '\'');
            out.push_str("''");
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// Index just past the bracket matching the opener at `open` (`[` or `{`).
fn matching(code: &[u8], open: usize) -> Option<usize> {
    let (o, cl) = match code.get(open)? {
        b'[' => (b'[', b']'),
        b'{' => (b'{', b'}'),
        _ => return None,
    };
    let mut depth = 0usize;
    for (k, &b) in code.iter().enumerate().skip(open) {
        if b == o {
            depth += 1;
        } else if b == cl {
            depth -= 1;
            if depth == 0 {
                return Some(k + 1);
            }
        }
    }
    None
}

fn skip_ws(code: &[u8], mut i: usize) -> usize {
    while code.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
        i += 1;
    }
    i
}

/// Returns the RT-relevant code of `src`: comments removed, literal contents
/// blanked, and every `#[cfg(test)] mod name { … }` block cut out. Any other
/// item behind `#[cfg(test)]` is an error.
pub(crate) fn rt_code(src: &str) -> Result<String, String> {
    const CFG: &str = "#[cfg(test)]";
    let mut code = strip_comments_and_literals(src);
    while let Some(start) = code.find(CFG) {
        let bytes = code.as_bytes();
        let mut i = skip_ws(bytes, start + CFG.len());
        // further attributes, e.g. `#[allow(..)]`
        while bytes.get(i) == Some(&b'#') && bytes.get(i + 1) == Some(&b'[') {
            i = skip_ws(bytes, matching(bytes, i + 1).ok_or("unclosed attribute")?);
        }
        let rest = code.get(i..).unwrap_or("");
        let is_mod = rest
            .strip_prefix("mod")
            .is_some_and(|r| r.starts_with(char::is_whitespace));
        if !is_mod {
            let line = rest.lines().next().unwrap_or("");
            return Err(format!(
                "#[cfg(test)] on an item other than an inline test module: {line:?}"
            ));
        }
        let open = rest
            .find(['{', ';'])
            .filter(|&k| rest.as_bytes().get(k) == Some(&b'{'))
            .ok_or("#[cfg(test)] module must be inline (`mod name { … }`)")?;
        let end = matching(bytes, i + open).ok_or("unclosed #[cfg(test)] module")?;
        code.replace_range(start..end, "");
    }
    Ok(code)
}

/// Forbidden tokens found in `code`, each matched on identifier boundaries.
pub(crate) fn violations(code: &str) -> Vec<&'static str> {
    FORBIDDEN
        .iter()
        .copied()
        .filter(|token| {
            let starts_ident = token.starts_with(is_ident);
            let ends_ident = token.ends_with(is_ident);
            code.match_indices(token).any(|(at, _)| {
                let before = code[..at].chars().next_back();
                let after = code[at + token.len()..].chars().next();
                !(starts_ident && before.is_some_and(is_ident))
                    && !(ends_ident && after.is_some_and(is_ident))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_strings_and_test_modules() {
        let src = r##"let a = 1; // push
/// String docs
let s = "x // y push"; let r = r#"lock() "quoted""#; let b = 2; /* lock() /* nested */ */
let q = '"'; let e = '\''; fn f<'a>(x: &'a str) {}
#[cfg(test)]
mod t { fn f() { let _ = "}"; push } }
let c = 3;
"##;
        let code = rt_code(src).expect("valid RT source");
        assert!(code.contains("let b = 2;"), "{code}");
        assert!(code.contains("fn f<'a>(x: &'a str)"), "{code}");
        assert!(
            code.contains("let c = 3;"),
            "code after the module kept: {code}"
        );
        assert!(!code.contains("mod t"), "{code}");
        assert!(
            violations(&code).is_empty(),
            "{:?} in {code}",
            violations(&code)
        );
    }

    #[test]
    fn detects_violations() {
        let src = r#"fn f(v: &mut Vec<u8>) { v.push(1); let x = m.lock().unwrap();
            let s = 'a'; let y: &'static str = "ok"; std::thread::sleep(d);
            let w = vec![0]; let z = it.collect::<Vec<_>>(); panic!("no");
            let a = Arc::new(1); let t = x.to_string(); let l = RwLock::new(0); }
        "#;
        let code = rt_code(src).expect("valid RT source");
        let found = violations(&code);
        for token in [
            "push",
            "lock()",
            "unwrap()",
            "sleep",
            "vec!",
            ".collect",
            "panic!",
            "Arc::new",
            "to_string",
            "RwLock",
        ] {
            assert!(found.contains(&token), "{token} not detected in {found:?}");
        }
    }

    #[test]
    fn matches_on_word_boundaries() {
        let code = "fn g() { block(); pushed += 1; unlocked(); my_sleeper(); ToString; }";
        assert!(violations(code).is_empty(), "{:?}", violations(code));
    }

    #[test]
    fn rejects_cfg_test_items_other_than_an_inline_module() {
        assert!(rt_code("fn a() {}\n#[cfg(test)]\nfn helper() { push }\n").is_err());
        assert!(rt_code("#[cfg(test)]\nuse std::vec::Vec;\n").is_err());
        assert!(rt_code("#[cfg(test)]\nmod tests;\n").is_err());
        assert!(rt_code("#[cfg(test)]\n#[allow(dead_code)]\nmod tests {}\n").is_ok());
    }

    #[test]
    fn rt_modules_have_no_forbidden_tokens() {
        let root = env!("CARGO_MANIFEST_DIR");
        for module in RT_MODULES {
            let path = std::path::Path::new(root).join(module);
            let src = std::fs::read_to_string(&path).expect("read RT module");
            let code = rt_code(&src).unwrap_or_else(|e| panic!("{module}: {e}"));
            assert!(!code.trim().is_empty(), "{module}: empty after stripping");
            let found = violations(&code);
            assert!(found.is_empty(), "{module}: forbidden tokens {found:?}");
        }
    }
}
