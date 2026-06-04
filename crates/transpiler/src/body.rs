use regex::{Captures, Regex};

// --- PATH NORMALIZATION (used by lowering too) ---
pub fn normalize_path(path: &str) -> String {
    if path.contains("::") { return path.to_string(); } // already rust
    if path.starts_with("./") || path.starts_with("../") {
        let mut rest = path;
        let mut prefix = String::from("crate");
        while rest.starts_with("../") { prefix.push_str("::super"); rest = &rest[3..]; }
        if rest.starts_with("./") { rest = &rest[2..]; }
        let parts: Vec<_> = rest.split(|c| c=='/' || c=='.')
            .filter(|s| !s.is_empty())
            .map(|s| s.replace('-', "_"))
            .collect();
        if parts.is_empty() { return prefix; }
        format!("{}::{}", prefix, parts.join("::"))
    } else {
        if path.contains('.') {
            path.split('.')
                .filter(|s| !s.is_empty())
                .map(|s| s.replace('-', "_"))
                .collect::<Vec<_>>()
                .join("::")
        } else {
            path.replace('-', "_")
        }
    }
}

// --- helpers used by rewrites ---
fn strip_return_wrapping_parens(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() && src[i..].starts_with("return") {
            let before_ok = i == 0 || {
                let b = bytes[i - 1];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            let after = i + "return".len();
            let after_ok = after >= bytes.len() || {
                let b = bytes[after];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            if before_ok && after_ok {
                out.push_str("return");
                i = after;
                let mut had_ws = false;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() { had_ws = true; i+=1; }
                if i < bytes.len() && bytes[i] == b'(' {
                    let mut depth = 0usize;
                    let mut j = i;
                    while j < bytes.len() {
                        match bytes[j] {
                            b'(' => depth += 1,
                            b')' => { if depth>0 { depth-=1; if depth==0 { break; } } }
                            _ => {}
                        }
                        j += 1;
                    }
                    if depth == 0 {
                        // detect tuple: top-level comma
                        let mut k = i + 1; let mut d2=0usize; let mut comma=false;
                        while k < j {
                            match bytes[k] {
                                b'(' => d2+=1,
                                b')' => d2=d2.saturating_sub(1),
                                b',' if d2==0 => { comma=true; break; }
                                _ => {}
                            } k+=1;
                        }
                        if !comma {
                            out.push(' ');
                            out.push_str(&src[i+1..j]);
                            i = j+1;
                            continue;
                        }
                    }
                }
                if had_ws { out.push(' '); }
            }
        }
        out.push(bytes[i] as char);
        i+=1;
    }
    out
}

fn rewrite_local_arrows(s: &str) -> String {
    // Block body: let f = (x: T, y: U): R => { ... };
    let re_block = Regex::new(
        r#"(?sx)
            \blet\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*
            (?: (move)\s+)?           # optional move
            \(\s*([^)]*?)\s*\)\s*:\s*([^=}{;]+?)\s*=>\s*
            \{\s*(.*?)\s*\}\s*;
        "#
    ).unwrap();

    // Expr body: let f = (x: T): R => (expr);
    let re_expr = Regex::new(
        r#"(?sx)
            \blet\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*
            (?: (move)\s+)?           # optional move
            \(\s*([^)]*?)\s*\)\s*:\s*([^=}{;]+?)\s*=>\s*
            \(\s*(.*?)\s*\)\s*;
        "#
    ).unwrap();

    let out = re_block.replace_all(s, |c: &Captures| {
        let name = c[1].trim();
        let mv = c.get(2).map(|m| m.as_str()).unwrap_or("").trim();
        let params = c[3].trim();
        let ret = c[4].trim();
        let body = c[5].trim();
        if mv.is_empty() {
            format!(r#"let {name} = |{params}| -> {ret} {{ {body} }};"#)
        } else {
            format!(r#"let {name} = move |{params}| -> {ret} {{ {body} }};"#)
        }
    }).into_owned();

    re_expr.replace_all(&out, |c: &Captures| {
        let name = c[1].trim();
        let mv = c.get(2).map(|m| m.as_str()).unwrap_or("").trim();
        let params = c[3].trim();
        let ret = c[4].trim();
        let expr = c[5].trim();
        if mv.is_empty() {
            format!(r#"let {name} = |{params}| -> {ret} {{ {expr} }};"#)
        } else {
            format!(r#"let {name} = move |{params}| -> {ret} {{ {expr} }};"#)
        }
    }).into_owned()
}


fn needs_block(body: &str) -> bool {
    // quick heuristic: statement-ish markers or a top-level ';'
    let b = body.trim();
    if b.starts_with("let ")
        || b.starts_with("return")
        || b.contains('\n')
    {
        return true;
    }
    // look for a top-level semicolon
    let mut depth = 0usize;
    for ch in b.chars() {
        match ch {
            '{' | '(' => depth += 1,
            '}' | ')' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn find_balanced(src: &str, start: usize, open: char, close: char) -> Option<usize> {
    let bytes = src.as_bytes();
    let mut i = start;
    let mut d: i32 = 0;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch == open { d += 1; }
        else if ch == close {
            d -= 1;
            if d == 0 { return Some(i); }
        }
        i += 1;
    }
    None
}

fn is_ident_boundary(bytes: &[u8], idx: usize) -> bool {
    if idx >= bytes.len() { return true; }
    let b = bytes[idx];
    !(b as char).is_ascii_alphanumeric() && b != b'_'
}

fn strip_top_level_breaks(s: String) -> String {
    // Remove `break;` that appear at top level of a case body.
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut depth_paren = 0i32;
    let mut depth_brace = 0i32;
    while i < bytes.len() {
        // detect `break;` at top level (not inside parens/braces)
        if depth_paren == 0 && depth_brace == 0 && s[i..].starts_with("break") {
            // word boundary before/after
            let before_ok = i == 0 || {
                let b = bytes[i - 1];
                !((b as char).is_ascii_alphanumeric() || b == b'_')
            };
            let after = i + "break".len();
            let after_ok = after >= bytes.len() || is_ident_boundary(bytes, after);
            if before_ok && after_ok {
                // optionally skip whitespace, then a single ';'
                let mut j = after;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() { j += 1; }
                if j < bytes.len() && bytes[j] == b';' {
                    j += 1;
                }
                // swallow it
                i = j;
                continue;
            }
        }
        match bytes[i] as char {
            '(' => { depth_paren += 1; out.push('('); }
            ')' => { depth_paren -= 1; out.push(')'); }
            '{' => { depth_brace += 1; out.push('{'); }
            '}' => { depth_brace -= 1; out.push('}'); }
            _ => out.push(bytes[i] as char),
        }
        i += 1;
    }
    out
}

fn rewrite_switches(original: &str) -> String {
    let mut s = original.to_string();
    loop {
        // Find next "switch"
        let bytes = s.as_bytes();
        let mut pos = match s.find("switch") {
            Some(p) => p,
            None => break,
        };
        // word boundary checks
        let before_ok = pos == 0 || {
            let b = bytes[pos - 1];
            !((b as char).is_ascii_alphanumeric() || b == b'_')
        };
        let after = pos + "switch".len();
        let after_ok = after >= bytes.len() || is_ident_boundary(bytes, after);
        if !(before_ok && after_ok) {
            // skip this occurrence
            pos = after;
            if pos >= s.len() { break; }
            // advance one char to avoid infinite loop
            let mut tmp = s.clone();
            tmp.replace_range(0..pos, "");
            // not worth it; just move search window forward fast:
            if let Some(next) = s[after..].find("switch") {
                pos = after + next;
            } else { break; }
        }

        // Expect: switch ( <expr> ) { <cases> }
        // find '('
        let mut i = after;
        while i < s.len() && s.as_bytes()[i].is_ascii_whitespace() { i += 1; }
        if i >= s.len() || s.as_bytes()[i] != b'(' { break; }
        let lparen = i;
        let rparen = match find_balanced(&s, lparen, '(', ')') { Some(p) => p, None => break };
        let discrim = s[lparen + 1..rparen].trim().to_string();

        // find '{'
        i = rparen + 1;
        while i < s.len() && s.as_bytes()[i].is_ascii_whitespace() { i += 1; }
        if i >= s.len() || s.as_bytes()[i] != b'{' { break; }
        let lbrace = i;
        let rbrace = match find_balanced(&s, lbrace, '{', '}') { Some(p) => p, None => break };

        let cases_src = &s[lbrace + 1..rbrace];

        // Parse cases: sequences of (case <pat> :)* body ... until next case/default or end
        #[derive(Default)]
        struct Arm { patterns: Vec<String>, body: String, is_default: bool }
        let mut arms: Vec<Arm> = Vec::new();

        let mut j = 0usize;
        let cb = cases_src.as_bytes();
        // helper to skip ws/comments
        let skip_ws = |k: &mut usize| {
            while *k < cb.len() {
                if cb[*k].is_ascii_whitespace() { *k += 1; continue; }
                // // comment
                if *k + 1 < cb.len() && &cases_src[*k..*k+2] == "//" {
                    *k += 2;
                    while *k < cb.len() && cb[*k] != b'\n' { *k += 1; }
                    continue;
                }
                // /* ... */
                if *k + 1 < cb.len() && &cases_src[*k..*k+2] == "/*" {
                    *k += 2;
                    while *k + 1 < cb.len() && &cases_src[*k..*k+2] != "*/" { *k += 1; }
                    if *k + 1 < cb.len() { *k += 2; }
                    continue;
                }
                break;
            }
        };

        while j < cb.len() {
            skip_ws(&mut j);
            if j >= cb.len() { break; }

            let rest = &cases_src[j..];
            let mut arm = Arm::default();

            // default:
            if rest.starts_with("default") && is_ident_boundary(cb, j + "default".len()) {
                // move past 'default' and ':'
                j += "default".len();
                skip_ws(&mut j);
                if j < cb.len() && cb[j] == b':' { j += 1; }
                arm.is_default = true;
                // collect body until next case/default or EOF (depth-aware)
                let start_body = j;
                let mut depth_paren = 0i32;
                let mut depth_brace = 0i32;
                while j < cb.len() {
                    // peek for 'case'/'default' only at top-level
                    if depth_paren == 0 && depth_brace == 0 {
                        if cases_src[j..].starts_with("case") && is_ident_boundary(cb, j + 4) { break; }
                        if cases_src[j..].starts_with("default") && is_ident_boundary(cb, j + 7) { break; }
                    }
                    match cb[j] as char {
                        '(' => depth_paren += 1,
                        ')' => depth_paren -= 1,
                        '{' => depth_brace += 1,
                        '}' => depth_brace -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                arm.body = strip_top_level_breaks(cases_src[start_body..j].to_string()).trim().to_string();
                arms.push(arm);
                continue;
            }

            // case <pat> :  (possibly multiple 'case's in a row for fallthrough labels)
            if rest.starts_with("case") && is_ident_boundary(cb, j + 4) {
                let mut labels: Vec<String> = Vec::new();
                loop {
                    if !(cases_src[j..].starts_with("case") && is_ident_boundary(cb, j + 4)) { break; }
                    j += "case".len();
                    skip_ws(&mut j);
                    // collect pattern until ':', depth-aware for parens
                    let pat_start = j;
                    let mut depth_paren = 0i32;
                    while j < cb.len() {
                        let ch = cb[j] as char;
                        if ch == '(' { depth_paren += 1; }
                        else if ch == ')' { depth_paren -= 1; }
                        if ch == ':' && depth_paren == 0 { break; }
                        j += 1;
                    }
                    let pat = cases_src[pat_start..j].trim().to_string();
                    labels.push(pat);
                    if j < cb.len() && cb[j] == b':' { j += 1; }
                    skip_ws(&mut j);
                    // if next token is another 'case', keep gathering labels (fallthrough)
                    if !(cases_src[j..].starts_with("case") && is_ident_boundary(cb, j + 4)) { break; }
                }

                // collect body until next case/default/EOF at top-level
                let start_body = j;
                let mut depth_paren = 0i32;
                let mut depth_brace = 0i32;
                while j < cb.len() {
                    if depth_paren == 0 && depth_brace == 0 {
                        if cases_src[j..].starts_with("case") && is_ident_boundary(cb, j + 4) { break; }
                        if cases_src[j..].starts_with("default") && is_ident_boundary(cb, j + 7) { break; }
                    }
                    match cb[j] as char {
                        '(' => depth_paren += 1,
                        ')' => depth_paren -= 1,
                        '{' => depth_brace += 1,
                        '}' => depth_brace -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                let body = strip_top_level_breaks(cases_src[start_body..j].to_string()).trim().to_string();
                arms.push(Arm { patterns: labels, body, is_default: false });
                continue;
            }

            // unknown token -> bail out (leave the original content)
            break;
        }

        // If we failed to parse anything, stop to avoid infinite loop.
        if arms.is_empty() {
            // move search window: skip this "switch"
            if let Some(next) = s[after..].find("switch") {
                let cut = after + next;
                // advance 1 char
                let mut t = s.clone();
                t.replace_range(0..cut+1, "");
                // fall out and try again; to keep things simple, just break:
            }
            break;
        }

        // Build Rust `match`.
        // Patterns are emitted as-is; the TSRust surface keeps Rust-like patterns.
        let mut match_out = String::new();
        match_out.push_str(&format!("match {} {{\n", discrim));
        for arm in &arms {
            let pat_join = if arm.is_default {
                "_".to_string()
            } else {
                arm.patterns
                    .iter()
                    .map(|p| p.trim().to_string())
                    .collect::<Vec<_>>()
                    .join(" | ")
            };

            let body = arm.body.trim();

            if body.starts_with('{') && body.ends_with('}') {
                match_out.push_str(&format!("    {} => {},\n", pat_join, body));
            } else if needs_block(body) {
                match_out.push_str(&format!("    {} => {{\n{}\n    }},\n", pat_join, body));
            } else {
                match_out.push_str(&format!("    {} => {{ {} }},\n", pat_join, body));
            }
        }

        match_out.push('}');


        // Splice into source
        let replace_start = pos;
        let replace_end = rbrace + 1;
        s.replace_range(replace_start..replace_end, &match_out);
    }
    s
}

// --- feature transforms (kept small & safe) ---
fn rewrite_for_of(s: &str) -> String {
    let re = Regex::new(r#"for\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s+of\s+([^)]+)\)"#).unwrap();
    re.replace_all(s, |caps: &Captures| {
        format!("for {} in {}", &caps[1], &caps[2])
    }).into_owned()
}

fn rewrite_paren_tail_inline(s: &str) -> String {
    // { ( expr ) } -> { expr }
    let re = Regex::new(r#"(?s)\{\s*\(\s*(.+?)\s*\)\s*\}"#).unwrap();
    re.replace_all(s, "{ $1 }").into_owned()
}

fn rewrite_paren_tail_line(s: &str) -> String {
    // line made only of ( expr ) -> expr
    let re = Regex::new(r#"(?ms)^\s*\(\s*(.+?)\s*\)\s*$"#).unwrap();
    re.replace_all(s, "$1").into_owned()
}

fn rewrite_paren_tail_before_brace(s: &str) -> String {
    // ( expr ) } -> expr }
    let re = Regex::new(r#"(?m)^\s*\(\s*(.+?)\s*\)\s*}"#).unwrap();
    re.replace_all(s, "$1 }").into_owned()
}

fn rewrite_return_unit(s: &str) -> String {
    let re = Regex::new(r#"\breturn\s*;"#).unwrap();
    // re.replace_all(s, "return ();").into_owned()
    re.replace_all(s, "return ;").into_owned()
}

fn rewrite_coalesce(mut out: String) -> String {
    // lhs ?? rhs  -> (lhs).unwrap_or(rhs)
    let re = regex::Regex::new(
        r#"(?x)
            (
              (?:[A-Za-z_][A-Za-z0-9_]*|\([^()]*\))
              (?:\.[A-Za-z_][A-Za-z0-9_]*(?:\([^()]*\))?)*
            )
            \s*\?\?\s*
            ([^?\n;\)\},]+)
        "#
    ).unwrap();

    // keep applying until stabilized (covers chains: a ?? 1 ?? 2)
    loop {
        let next = re.replace_all(&out, |caps: &regex::Captures| {
            let lhs = caps[1].trim();
            let rhs = caps[2].trim();
            format!("({}).unwrap_or({})", lhs, rhs)
        }).into_owned();
        if next == out { break; }
        out = next;
    }

    // Canonicalize to match tests exactly:

    // 1) Wrap a bare ident before `.unwrap_or` to force `(x).unwrap_or(..)`
    let re_wrap_ident = regex::Regex::new(
        r#"(?P<prefix>^|[^A-Za-z0-9_\.\(])(?P<name>[A-Za-z_][A-Za-z0-9_]*)\.unwrap_or"#
    ).unwrap();
    out = re_wrap_ident
        .replace_all(&out, "${prefix}(${name}).unwrap_or")
        .into_owned();

    // 2) Trim spaces inside the paren when it is a single ident: `( x ) .unwrap_or` -> `(x).unwrap_or`
    let re_space_in_paren = regex::Regex::new(
        r#"\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)\.unwrap_or"#
    ).unwrap();
    out = re_space_in_paren
        .replace_all(&out, "($1).unwrap_or")
        .into_owned();

    out
}



fn rewrite_opt_chain(mut out: String) -> String {
    // A) root?.member(args?) -> (root).as_ref().map(|__v| __v.member(args?))
    let re_a = Regex::new(
        r#"(?x)
        (
          (?:[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*|\([^()]*\))
        )
        \?\.([A-Za-z_][A-Za-z0-9_]*)
        (\([^()]*\))?
        "#
    ).unwrap();
    // B) continue chain after )?.member(args?) on prior result:
    let re_b = Regex::new(
        r#"(?x)
        \)\s*\?\.([A-Za-z_][A-Za-z0-9_]*)(\([^()]*\))?
        "#
    ).unwrap();

    loop {
        let step1 = re_a.replace_all(&out, |caps: &Captures|{
            let root=caps[1].trim(); let mem=caps[2].trim(); let call=caps.get(3).map(|m|m.as_str()).unwrap_or("");
            format!("({}).as_ref().map(|__v| __v.{}{})", root, mem, call)
        }).into_owned();
        let step2 = re_b.replace_all(&step1, |caps: &Captures|{
            let mem=caps[1].trim(); let call=caps.get(2).map(|m|m.as_str()).unwrap_or("");
            format!(").map(|__v| __v.{}{})", mem, call)
        }).into_owned();
        if step2 == out { break; }
        out = step2;
    }
    out
}


// Orchestrate all body transforms, in the same order you relied on:
pub fn rewrite_body_tsrust(original: &str) -> String {
    let mut s = original.to_string();

    s = rewrite_local_arrows(&s);
    
    // switch (...) { ... }  -> match (...) { ... };
    s = rewrite_switches(&s);

    // 1) for (x of xs) -> for x in xs
    s = rewrite_for_of(&s);

    // 2) Strip stray `);` after paren tails inside blocks
    let re_paren_tail_semi = Regex::new(r#"\)\s*;\s*}$"#).unwrap();
    s = re_paren_tail_semi.replace(&s, ")}").into_owned();

    // 3) Paren-tail inline & line and before-brace
    s = rewrite_paren_tail_inline(&s);
    s = rewrite_paren_tail_line(&s);
    s = rewrite_paren_tail_before_brace(&s);

    // 4) Nullish coalescing
    s = rewrite_coalesce(s);

    // 5) Optional chaining
    s = rewrite_opt_chain(s);

    // 6) `return (expr)` -> `return expr`
    s = strip_return_wrapping_parens(&s);

    // 7) return;  -> return ();
    s = rewrite_return_unit(&s);

    s
}
