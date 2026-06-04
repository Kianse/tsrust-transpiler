pub fn version() -> &'static str {
    "0.1.0"
}
use tsrust_ast::*;

pub struct LowerOptions {
    pub rewrite_body: fn(&str) -> String,
    pub normalize_path: fn(&str) -> String,
}

pub fn lower_to_rust(src: &str, opts: &LowerOptions) -> String {
    let m = tsrust_parser::parse_module(src);
    let mut out = String::new();
    for it in m.items {
        match it {
            Item::Import(d) => lower_import(&mut out, d, opts),
            Item::ExportStar(e) => out.push_str(&format!("pub use {}::*;\n", (opts.normalize_path)(&e.from))),
            Item::ReExport(r)   => out.push_str(&format!("pub use {}::{{{}}};\n", (opts.normalize_path)(&r.from), r.items.join(", "))),
            Item::ReExportLocal(e) => out.push_str(&format!("pub use self::{{{}}};\n", e.items.join(", "))),
            Item::ExportedFn(f) => lower_fn(&mut out, f, true, opts),
            Item::Fn(f)         => lower_fn(&mut out, f, false, opts),
            Item::ExportedArrow(a) => lower_arrow(&mut out, a, true, opts),
            Item::VarArrow(v)      => lower_var_arrow(&mut out, v, opts),
        }
    }
    out
}

fn lower_import(out:&mut String, d:ImportDecl, opts:&LowerOptions){
    let path = (opts.normalize_path)(&d.from);
    for it in d.items {
        match it {
            ImportItem::Star => out.push_str(&format!("use {}::*;\n", path)),
            ImportItem::StarAs(alias) => out.push_str(&format!("use {} as {};\n", path, alias)),
            ImportItem::Simple(n) => out.push_str(&format!("use {}::{};\n", path, n)),
            ImportItem::Aliased{name,alias} => out.push_str(&format!("use {}::{} as {};\n", path, name, alias)),
            ImportItem::Nested{path:nested} => {
                fn prettify_nested_path(s: &str) -> String {
                    let mut out = String::new();
                    let mut in_braces = false;
                    let mut just_wrote_comma = false;
                    for ch in s.chars() {
                        match ch {
                            '{' => { in_braces = true; just_wrote_comma = false; out.push('{'); }
                            '}' => { in_braces = false; just_wrote_comma = false; out.push('}'); }
                            ',' if in_braces => { out.push(','); out.push(' '); just_wrote_comma = true; }
                            _ => {
                                if just_wrote_comma && ch.is_whitespace() {
                                    // skip extra spaces after we already inserted one
                                } else {
                                    out.push(ch);
                                    just_wrote_comma = false;
                                }
                            }
                        }
                    }
                    out
                }
                let pretty = prettify_nested_path(&nested);
                out.push_str(&format!("use {}::{};\n", path, pretty));
            }
        }
    }
}

fn lower_fn(out:&mut String, f:FnDecl, exported:bool, opts:&LowerOptions){
    let (ps, inits) = rewrite_params(f.params);
    let ret = f.ret.as_ref().map(|t| t.text.trim().to_string());
    // First rewrite the original tsrust body
    let mut body = (opts.rewrite_body)(&f.body_src);
    // Then prepend param-default initializers so rewrites don't touch them
    if !inits.is_empty() {
        let mut s = String::new();
        for i in inits { s.push_str(&i); s.push('\n'); }
        body = s + &body;
    }
    let pub_ = if exported { "pub " } else { "" };
    let ps_s = ps.join(", ");
    let b = body.trim();
    let single = !b.contains('\n');
    match ret {
        Some(r) if !r.is_empty() => {
            if single {
                out.push_str(&format!("{pub_}fn {}({}) -> {} {{ {} }}\n", f.name, ps_s, r, b));
            } else {
                out.push_str(&format!("{pub_}fn {}({}) -> {} {{\n{}\n}}\n", f.name, ps_s, r, b));
            }
        }
        _ => {
            if single {
                out.push_str(&format!("{pub_}fn {}({}) {{ {} }}\n", f.name, ps_s, b));
            } else {
                out.push_str(&format!("{pub_}fn {}({}) {{\n{}\n}}\n", f.name, ps_s, b));
            }
        }
    }
}

fn lower_arrow(out:&mut String, a:ArrowDecl, exported:bool, opts:&LowerOptions){
    let (ps, inits)=rewrite_params(a.params);
    let mut body = match a.body {
        ArrowBody::Block(b) => b,
        ArrowBody::Expr(e) => e,
    };
    // Rewrite original body first…
    body = (opts.rewrite_body)(&body);
    // …then prepend defaults to avoid coalesce canonicalization touching them
    if !inits.is_empty() {
        let mut s=String::new(); for i in inits { s.push_str(&i); s.push('\n'); }
        body = s + &body;
    }

    let pub_ = if exported { "pub " } else { "" };
    let ps_s = ps.join(", ");
    let r = a.ret.text.trim();
    let b = body.trim();
    if !b.contains('\n') {
        out.push_str(&format!("{pub_}fn {}({}) -> {} {{ {} }}\n", a.name, ps_s, r, b));
    } else {
        out.push_str(&format!("{pub_}fn {}({}) -> {} {{\n{}\n}}\n", a.name, ps_s, r, b));
    }
}


fn lower_var_arrow(out:&mut String, v:VarArrowDecl, opts:&LowerOptions){
    let (ps, inits)=rewrite_params(v.params);
    let mut body = match v.body { ArrowBody::Block(b)=>b, ArrowBody::Expr(e)=>e };
    // Rewrite first, then inject defaults
    body = (opts.rewrite_body)(&body);
    if !inits.is_empty(){
        let mut s=String::new(); for i in inits { s.push_str(&i); s.push('\n'); }
        body = s + &body;
    }
    out.push_str(&format!("let {} = {}|{}| -> {} {{\n{}\n}};\n",
        v.name, if v.is_move{"move "}else{""}, ps.join(", "), v.ret.text.trim(), body.trim()));
}

fn rewrite_params(params:Vec<Param>)->(Vec<String>,Vec<String>){
    let mut ps=Vec::new(); let mut init=Vec::new();
    for p in params {
        match p.kind {
            ParamKind::Optional => ps.push(format!("{}: Option<{}>", p.name, p.ty)),
            ParamKind::Defaulted(def) => { ps.push(format!("{}: Option<{}>", p.name, p.ty)); init.push(format!("let {} = {}.unwrap_or({});", p.name, p.name, def)); }
            ParamKind::Normal => ps.push(format!("{}: {}", p.name, p.ty)),
        }
    }
    (ps, init)
}

#[cfg(test)]
mod tests {
    use super::{lower_to_rust, LowerOptions};

    fn identity_body(body: &str) -> String {
        body.to_string()
    }

    fn rust_path(path: &str) -> String {
        match path {
            "./util.math" => "crate::util::math".into(),
            "../shared" => "crate::super::shared".into(),
            other => other.into(),
        }
    }

    #[test]
    fn lowers_imports_reexports_and_exported_functions() {
        let src = r#"
            import { foo, bar::{Baz, qux as q} } from "./util.math";
            export { Thing } from "../shared";
            export function add(a: i32, b?: i32): i32 { (a) }
        "#;

        let out = lower_to_rust(
            src,
            &LowerOptions {
                rewrite_body: identity_body,
                normalize_path: rust_path,
            },
        );

        assert!(out.contains("use crate::util::math::foo;"));
        assert!(out.contains("use crate::util::math::bar::{Baz, qux as q};"));
        assert!(out.contains("pub use crate::super::shared::{Thing};"));
        assert!(out.contains("pub fn add(a: i32, b: Option<i32>) -> i32 { (a) }"));
    }

    #[test]
    fn lowers_default_params_and_arrow_forms() {
        let src = r#"
            export function add(a: i32 = 1, b?: i32): i32 {
                return (a)
            }
            export const make = move (x: i32): i32 => (x);
        "#;

        let out = lower_to_rust(
            src,
            &LowerOptions {
                rewrite_body: identity_body,
                normalize_path: rust_path,
            },
        );

        assert!(out.contains("pub fn add(a: Option<i32>, b: Option<i32>) -> i32 {"));
        assert!(out.contains("let a = a.unwrap_or(1);"));
        assert!(out.contains("pub fn make(x: i32) -> i32 { (x); }"));
    }

    #[test]
    fn applies_body_rewrite_hook_before_emitting_rust() {
        fn rewrite_switch_like(_: &str) -> String {
            "match op {\n    0 => { 1 },\n    _ => { 2 },\n}".into()
        }

        let src = r#"
            function operate(op: i32): i32 {
                switch (op) {
                    default: { (2) }
                }
            }
        "#;

        let out = lower_to_rust(
            src,
            &LowerOptions {
                rewrite_body: rewrite_switch_like,
                normalize_path: rust_path,
            },
        );

        assert!(out.contains("fn operate(op: i32) -> i32 {"));
        assert!(out.contains("match op {"));
        assert!(out.contains("_ => { 2 },"));
    }
}
