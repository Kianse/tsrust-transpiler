use tsrust_ast::*;
use tsrust_lexer::{Lexer, TokKind, Token};

pub fn parse_module(src: &str) -> Module {
    let toks = Lexer::new(src).tokenize();
    let mut p = Parser { src, toks, i: 0 };
    p.parse_module()
}

struct Parser<'a> { src: &'a str, toks: Vec<Token>, i: usize }

// --- helpers that DO NOT borrow Parser (avoid &self while holding &mut self) ---
fn tok_text(t: &Token) -> String {
    match &t.kind {
        TokKind::Ident(s) | TokKind::Number(s) => s.clone(),
        TokKind::Str(s) => format!("\"{}\"", s),
        TokKind::Question => "?".into(),
        TokKind::FatArrow => "=>".into(),
        TokKind::Comma => ",".into(),
        TokKind::Colon => ":".into(),
        TokKind::Semicolon => ";".into(),
        TokKind::Dot => ".".into(),
        TokKind::Star => "*".into(),
        TokKind::Eq => "=".into(), 
        TokKind::As => " as ".into(),
        TokKind::LParen => "(".into(),
        TokKind::RParen => ")".into(),
        _ => "".into(),
    }
}

impl<'a> Parser<'a> {

    fn peek(&self) -> Option<&Token> { self.toks.get(self.i) }
    fn bump(&mut self) -> Option<&Token> { let t = self.toks.get(self.i); self.i+=1; t }
    fn eat(&mut self, k: &TokKind) -> bool {
        matches!(self.peek(), Some(t) if std::mem::discriminant(&t.kind)==std::mem::discriminant(k))
            .then(|| { self.i+=1; }).is_some()
    }
        // move the token cursor to the first token with span.end > raw_pos
    fn advance_tokens_to(&mut self, raw_pos: usize) {
        while let Some(t) = self.peek() {
            if t.span.end <= raw_pos {
                self.i += 1;
            } else {
                break;
            }
        }
    }
    fn collect_raw_brace_block_from(&mut self, start: usize) -> Collected {
        let bytes = self.src.as_bytes();
        let mut depth: i32 = 1;
        let mut pos = start;
        let begin = start;

        while pos < bytes.len() {
            match bytes[pos] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let text = &self.src[begin..pos];
                        let end = pos + 1; // include the closing brace
                        // sync token cursor
                        self.advance_tokens_to(end);
                        return Collected {
                            text: text.trim().to_string(),
                            end,
                        };
                    }
                }
                _ => {}
            }
            pos += 1;
        }
        Collected { text: String::new(), end: start }
    }

    fn collect_raw_until_semicolon_from(&mut self, start: usize) -> Collected {
        let s = &self.src[start..];

        // Find first ';' (include it), otherwise stop at first '\n' (exclude it),
        // otherwise go to EOF.
        let mut end_rel: Option<usize> = None;
        for (i, ch) in s.char_indices() {
            if ch == ';' {
                end_rel = Some(i + 1); // include ';'
                break;
            }
            if ch == '\n' {
                end_rel = Some(i);     // stop before newline
                break;
            }
        }

        let end = start + end_rel.unwrap_or(s.len());
        let text = &self.src[start..end];

        // sync token cursor past the end of what we just consumed
        self.advance_tokens_to(end);

        Collected { text: text.trim().to_string(), end }
    }


    fn parse_module(&mut self) -> Module {
        let mut items = Vec::new();
        while self.peek().is_some() {
            if self.is_kw(TokKind::Import) { if let Some(i)=self.parse_import(){ items.push(Item::Import(i)); continue; } }
            if self.is_kw(TokKind::Export) { if let Some(e)=self.parse_export(){ items.push(e); continue; } }
            if self.is_kw(TokKind::Function) { if let Some(f)=self.parse_fn(false){ items.push(Item::Fn(f)); continue; } }
            self.i += 1; // skip
        }
        Module{ items }
    }

    fn is_kw(&self, want: TokKind) -> bool {
        matches!(self.peek(), Some(Token{kind,..}) if std::mem::discriminant(kind)==std::mem::discriminant(&want))
    }

    fn parse_import(&mut self) -> Option<ImportDecl> {
         let start = self.peek()?.span.start; 
        self.bump(); // import

        // import * as alias from "path";
        if self.eat(&TokKind::Star) {
            if !self.is_kw(TokKind::As) { return None; }
            self.bump(); // as
            let alias = match self.bump() {
                Some(Token { kind: TokKind::Ident(s), .. }) => s.clone(),
                _ => return None,
            };
            if !self.is_kw(TokKind::From) { return None; }
            self.bump(); // from
            let path = match self.bump() {
                Some(Token { kind: TokKind::Str(s), .. }) => s.clone(),
                _ => return None,
            };
            return Some(ImportDecl {
                items: vec![ImportItem::StarAs(alias)],
                from: path,
                span: Span { start, end: self.peek().map(|t| t.span.end).unwrap_or(start) },
            });
        }
        if !self.eat(&TokKind::LBrace) { return None; }
        let mut raw = String::new(); let mut depth=1i32;
        let mut end=start;
        while let Some(t)=self.peek() {
            end=t.span.end;
            match t.kind {
                TokKind::LBrace => { raw.push('{'); self.i+=1; depth+=1; }
                TokKind::RBrace => { self.i+=1; depth-=1; if depth==0 { break; } raw.push('}'); }
                _ => { raw.push_str(&tok_text(t)); self.i+=1; }
            }
        }
        let _ = self.eat(&TokKind::RBrace);
        if !self.is_kw(TokKind::From) { return None; }
        self.bump();
        let path = match self.bump() {
            Some(Token{kind:TokKind::Str(s),..}) => s.clone(),
            _=>return None
        };
        let items = split_items(&raw).into_iter().map(parse_import_item).collect();
        Some(ImportDecl { items, from: path, span: Span{ start, end }})
    }

    fn parse_export(&mut self) -> Option<Item> {
        self.bump(); // export
        // handle variable/arrow first so it doesn't get pre-empted
        if self.is_kw(TokKind::Const) || self.is_kw(TokKind::Let) {
            if let Some(a) = self.parse_var_arrow(true) {
                return Some(Item::ExportedArrow(a));
            }
        }
        if self.is_kw(TokKind::Function) {
            let mut f = self.parse_fn(true)?;
            f.exported = true;
            return Some(Item::ExportedFn(f));
        }
        if self.eat(&TokKind::Star) && self.is_kw(TokKind::From) {
            self.bump();
            let s = match self.bump() { Some(Token{kind:TokKind::Str(s),..})=>s.clone(), _=>return None };
            return Some(Item::ExportStar(ExportStar{ from:s, span: Span{ start:0,end:0 }}));
        }
        if self.eat(&TokKind::LBrace) {
            let mut raw=String::new(); let mut depth=1i32;
            while let Some(t)=self.peek() {
                match t.kind {
                    TokKind::LBrace => { raw.push('{'); self.i+=1; depth+=1; }
                    TokKind::RBrace => { self.i+=1; depth-=1; if depth==0 { break; } raw.push('}'); }
                    _ => { raw.push_str(&tok_text(t)); self.i+=1; }
                }
            }
            let items = split_items(&raw);
            if self.is_kw(TokKind::From) {
                self.bump();
                let s = match self.bump() { Some(Token{kind:TokKind::Str(s),..})=>s.clone(), _=>return None };
                return Some(Item::ReExport(ReExportDecl{ items, from:s, span:Span{start:0,end:0}}));
            } else {
                return Some(Item::ReExportLocal(ExportLocal{ items, span: Span{start:0,end:0}}));
            }
        }
        if self.is_kw(TokKind::Const) || self.is_kw(TokKind::Let) {
            if let Some(a)=self.parse_var_arrow(true) { return Some(Item::ExportedArrow(a)); }
        }
        None
    }

    fn parse_fn(&mut self, exported: bool) -> Option<FnDecl> {
        let start = self.peek()?.span.start; self.bump(); // function
        let name = match self.bump(){ Some(Token{kind:TokKind::Ident(s),..})=>s.clone(), _=>return None };
        let _ = self.eat(&TokKind::LParen);
        let params_src = self.collect_until_paren_close();
        let params = parse_params(&params_src, start);
        let mut ret=None;
        if self.eat(&TokKind::Colon) {
            let ty = self.collect_until_brace_open().trim().to_string();
            ret = Some(TypeExpr{ text: ty });
        }

        // ⤵ copy the end-of-'{' to a plain usize so the token borrow ends before the next &mut self call
        let lbrace_end = {
            let t = self.bump()?;
            if !matches!(t.kind, TokKind::LBrace) { return None; }
            t.span.end
        };

        let body = self.collect_raw_brace_block_from(lbrace_end);
        Some(FnDecl{
            exported, name, params, ret,
            body_src: body.text, span: Span{ start, end: body.end },
        })
    }


    fn parse_var_arrow(&mut self, is_exported: bool) -> Option<ArrowDecl> {
        let start=self.peek()?.span.start;
        match self.bump()?.kind { TokKind::Const | TokKind::Let => {}, _=>return None }
        let name = match self.bump(){ Some(Token{kind:TokKind::Ident(s),..})=>s.clone(), _=>return None };
        // `const name = ...`
        if !self.eat(&TokKind::Eq) { return None; }
        // optional `move`
        let is_move = if self.is_kw(TokKind::Move) { self.bump(); true } else { false };
        // must start params with `(`
        if !self.eat(&TokKind::LParen) { return None; }
        let params_src = self.collect_until_paren_close();
        let params = parse_params(&params_src, start);
        if !self.eat(&TokKind::Colon) { return None; }
        let ret = TypeExpr{ text: self.collect_until_fat_arrow().trim().to_string() };
        let _ = self.eat(&TokKind::FatArrow);

        if self.is_kw(TokKind::LBrace) {
            // ⤵ copy out the end position, drop token borrow, then call the method
            let lbrace_end = {
                let t = self.bump()?;
                if !matches!(t.kind, TokKind::LBrace) { return None; }
                t.span.end
            };
            let body = self.collect_raw_brace_block_from(lbrace_end);
            Some(ArrowDecl{
                name, params, ret, is_exported, is_move,
                span: Span{ start, end: body.end }, body: ArrowBody::Block(body.text)
            })
        } else {
            let start_pos = self.peek().map(|t| t.span.start).unwrap_or(self.src.len());
            let expr = self.collect_raw_until_semicolon_from(start_pos);
            Some(ArrowDecl{
                name, params, ret, is_exported, is_move,
                span: Span{ start, end: expr.end }, body: ArrowBody::Expr(expr.text)
            })
        }
    }

    // collectors
    fn collect_until_paren_close(&mut self)->String{
        let mut d=1i32; let mut s=String::new();
        while let Some(t)=self.bump() {
            match t.kind {
                TokKind::LParen => { d+=1; s.push('('); }
                TokKind::RParen => { d-=1; if d==0 { break; } s.push(')'); }
                _=>s.push_str(&tok_text(t)),
            }
        } s
    }
    
    fn collect_until_brace_open(&mut self)->String{
        let mut s=String::new(); while let Some(t)=self.peek(){
            if matches!(t.kind, TokKind::LBrace) { break; }
            s.push_str(&tok_text(t)); self.i+=1;
        } s
    }
    
    fn collect_until_fat_arrow(&mut self)->String{
        let mut s=String::new(); while let Some(t)=self.peek(){
            if matches!(t.kind, TokKind::FatArrow) { break; }
            s.push_str(&tok_text(t)); self.i+=1;
        } s
    }
}
struct Collected{ text:String, end:usize }

fn split_items(raw:&str)->Vec<String>{
    let mut out=Vec::new(); let mut cur=String::new(); let mut d=0i32;
    for ch in raw.chars(){
        match ch {
            '{'=>{d+=1; cur.push(ch);}
            '}' =>{d-=1; cur.push(ch);}
            ',' if d==0 => { let s=cur.trim(); if !s.is_empty(){out.push(s.to_string());} cur.clear(); }
            _ => cur.push(ch),
        }
    }
    let s=cur.trim(); if !s.is_empty(){ out.push(s.to_string()); } out
}

fn parse_import_item(s:String)->ImportItem{
    let t=s.trim();
    if t=="*" { return ImportItem::Star; }
    if let Some(rest)=t.strip_prefix("* as "){ return ImportItem::StarAs(rest.trim().to_string()); }
    // treat `::{` with optional whitespace as nested
    if t.contains("::{") || t.contains(":: {") {
        // also normalize trivial spaces: `:: {` -> `::{`
        let p = t.replace(":: {", "::{");
        return ImportItem::Nested{ path: p };
    }
    if let Some((a,b))=t.split_once(" as "){ return ImportItem::Aliased{ name:a.trim().into(), alias:b.trim().into() }; }
    ImportItem::Simple(t.into())
}

fn parse_params(src:&str, base:usize)->Vec<Param>{
    let mut out=Vec::new(); let mut cur=String::new(); let mut d=0i32;
    for ch in src.chars(){
        match ch {
            '('=>{d+=1;cur.push(ch);} ')'=>{d-=1;cur.push(ch);}
            ',' if d==0 => { push_param(&mut out, cur.trim(), base); cur.clear(); }
            _=>cur.push(ch),
        }
    }
    push_param(&mut out, cur.trim(), base); out
}

fn push_param(out:&mut Vec<Param>, s:&str, base:usize){
    let s = s.trim();
    if s.is_empty(){ return; }
    let span = Span{ start:base, end:base+s.len() };

    // (1) name ?: Ty  (allow spaces around '?')
    if let Some(qpos) = s.find('?') {
        // only treat as optional if '?' appears before any '=' (i.e. not in default expr)
        let eq_pos = s.find('=');
        if eq_pos.map(|i| qpos < i).unwrap_or(true) {
            let (name_part, after_q) = s.split_at(qpos);
            if let Some(colon_pos) = after_q.find(':') {
                // normalize like the other branches: trim, and drop any trailing '?'
                let ty = after_q[colon_pos + 1..].trim().trim_end_matches('?').trim();
                out.push(Param {
                    name: name_part.trim().into(),
                    ty: ty.into(),
                    kind: ParamKind::Optional,
                    span
                });
                return;
            }
        }
    }

    // (2) name: Ty (= default)?
    if let Some((name, rest)) = s.split_once(':') {
        let rest = rest.trim();
        if let Some((ty, def)) = rest.split_once('=') {
            out.push(Param {
                name: name.trim().into(),
                ty: ty.trim().trim_end_matches('?').trim().into(),
                kind: ParamKind::Defaulted(def.trim().into()),
                span
            });
            return;
        }
        let is_opt = rest.trim_end().ends_with('?');
        let ty = rest.trim_end_matches('?').trim().to_string();
        out.push(Param {
            name: name.trim().into(),
            ty,
            kind: if is_opt { ParamKind::Optional } else { ParamKind::Normal },
            span
        });
        return;
    }

    // (3) bare name — treat as unit (fallback)
    out.push(Param{ name:s.into(), ty:"()".into(), kind:ParamKind::Normal, span });
}

#[cfg(test)]
mod tests {
    use super::parse_module;
    use tsrust_ast::{ArrowBody, ImportItem, Item, ParamKind};

    #[test]
    fn parses_import_export_and_function_shapes() {
        let src = r#"
            import { foo, bar::{Baz, qux as q} } from "./util.math";
            export { Foo } from "../models";
            export function add(a: i32 = 1, b?: i32): i32 { (a) }
        "#;

        let module = parse_module(src);
        assert_eq!(module.items.len(), 3);

        match &module.items[0] {
            Item::Import(import) => {
                assert_eq!(import.from, "./util.math");
                assert!(matches!(&import.items[0], ImportItem::Simple(name) if name == "foo"));
                assert!(matches!(&import.items[1], ImportItem::Nested { path } if path == "bar::{Baz,qux as q}"));
            }
            other => panic!("expected import, got {other:?}"),
        }

        match &module.items[1] {
            Item::ReExport(reexport) => {
                assert_eq!(reexport.items, vec!["Foo"]);
                assert_eq!(reexport.from, "../models");
            }
            other => panic!("expected re-export, got {other:?}"),
        }

        match &module.items[2] {
            Item::ExportedFn(function) => {
                assert_eq!(function.name, "add");
                assert_eq!(function.ret.as_ref().map(|t| t.text.as_str()), Some("i32"));
                assert_eq!(function.params.len(), 2);
                assert!(matches!(function.params[0].kind, ParamKind::Defaulted(ref def) if def == "1"));
                assert!(matches!(function.params[1].kind, ParamKind::Optional));
                assert_eq!(function.body_src, "(a)");
            }
            other => panic!("expected exported function, got {other:?}"),
        }
    }

    #[test]
    fn parses_arrow_forms_and_parameter_kinds() {
        let src = r#"
            export const build = move (name: String, count?: i32): i32 => {
                (count)
            }
        "#;

        let module = parse_module(src);
        assert_eq!(module.items.len(), 1);

        match &module.items[0] {
            Item::ExportedArrow(arrow) => {
                assert_eq!(arrow.name, "build");
                assert!(arrow.is_exported);
                assert!(arrow.is_move);
                assert!(matches!(arrow.params[0].kind, ParamKind::Normal));
                assert!(matches!(arrow.params[1].kind, ParamKind::Optional));
                assert!(matches!(arrow.body, ArrowBody::Block(ref body) if body.contains("(count)")));
            }
            other => panic!("expected exported arrow, got {other:?}"),
        }

    }
}
