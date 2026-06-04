mod body;

use anyhow::{anyhow, Result};

fn last_meaningful_line(s: &str) -> Option<String> {
    for line in s.lines().rev() {
        let t = line.trim();
        if t.is_empty() { continue; }
        if t.starts_with("//") { continue; }
        return Some(t.to_string());
    }
    None
}

fn has_terminal_marker(body: &str) -> bool {
    if let Some(line) = last_meaningful_line(body) {
        // Accept `return`, `return(...)`, `return;`
        if line.starts_with("return") {
            let rest = &line["return".len()..];
            let mut chs = rest.chars();
            match chs.next() {
                None => return true,
                Some(c) if c.is_whitespace() || c == '(' || c == ';' => return true,
                _ => {}
            }
        }
        // Paren-tail: ( expr ) or (expr)
        let re = regex::Regex::new(r#"^\(\s*.+?\s*\)\s*;?$"#).unwrap();
        if re.is_match(&line) { return true; }

        // NEW: treat a trailing switch-block as terminal for non-unit fns.
        // (The actual rewrite turns it into a match expression.)
        // Heuristic: last non-ws char is '}', and there is a 'switch' earlier.
        let trimmed = body.trim_end();
        if trimmed.ends_with('}') && trimmed.contains("switch") {
            return true;
        }
    }
    false
}


pub fn transpile_str(input: &str, file_path_hint: &str) -> Result<String> {
    // Pre-validate non-unit functions for terminal marker (legacy behavior)
    let m = tsrust_parser::parse_module(input);
    for it in &m.items {
        if let tsrust_ast::Item::ExportedFn(f) | tsrust_ast::Item::Fn(f) = it {
            if let Some(ret) = &f.ret {
                let r = ret.text.trim();
                if !r.is_empty() && r != "()" && !has_terminal_marker(&f.body_src) {
                    // best-effort line: find 'function <name>'
                    let needle = format!("function {}", f.name);
                    let line = input.split('\n')
                        .take_while(|l| !l.contains(&needle))
                        .count() + 1;
                    return Err(anyhow!(
                        "{file}:{line}:1: Non-unit function '{}' must end with `return EXPR;` or `(EXPR)`",
                        f.name,
                        file = file_path_hint
                    ));
                }
            }
        }
    }

    // Lower (parser -> AST -> Rust)
    let opts = tsrust_lowering::LowerOptions {
        rewrite_body: body::rewrite_body_tsrust,
        normalize_path: body::normalize_path,
    };
    let out = tsrust_lowering::lower_to_rust(input, &opts);
    Ok(out)
}
