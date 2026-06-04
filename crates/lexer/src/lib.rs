use tsrust_ast::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokKind {
    Ident(String), Number(String), Str(String),
    Import, Export, From, As, Function, Return, Let, Const, Pub, Move,
    LParen, RParen, LBrace, RBrace, Comma, Colon, Semicolon, Star, Eq, Dot, Question, FatArrow,
}

#[derive(Debug, Clone)]
pub struct Token { pub kind: TokKind, pub span: Span }

pub struct Lexer<'a> {
    src: &'a str, bytes: &'a [u8], i: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self { Self { src, bytes: src.as_bytes(), i: 0 } }
    fn peek(&self) -> Option<u8> { self.bytes.get(self.i).copied() }
    fn bump(&mut self) -> Option<u8> { let b = self.peek()?; self.i += 1; Some(b) }
    fn starts_with(&self, s: &str) -> bool { self.src[self.i..].starts_with(s) }

    fn skip_ws_comments(&mut self) {
        loop {
            let before = self.i;
            while let Some(b) = self.peek() {
                if b.is_ascii_whitespace() { self.i += 1; } else { break; }
            }
            if self.starts_with("//") {
                while let Some(b) = self.bump() { if b == b'\n' { break; } }
                continue;
            }
            if self.starts_with("/*") {
                self.i += 2;
                while self.i + 1 < self.bytes.len() && &self.src[self.i..self.i+2] != "*/" {
                    self.i += 1;
                }
                if self.i + 1 < self.bytes.len() { self.i += 2; }
                continue;
            }
            if self.i == before { break; }
        }
    }


    fn ident_or_kw(&mut self) -> Token {
        let start = self.i;
        while let Some(b) = self.peek() {
            if b.is_ascii_alphanumeric() || b == b'_' { self.i += 1; } else { break; }
        }
        let t = &self.src[start..self.i];
        let kind = match t {
            "import" => TokKind::Import,
            "export" => TokKind::Export,
            "from" => TokKind::From,
            "as" => TokKind::As,
            "function" => TokKind::Function,
            "return" => TokKind::Return,
            "let" => TokKind::Let,
            "const" => TokKind::Const,
            "pub" => TokKind::Pub,
            "move" => TokKind::Move,
            _ => TokKind::Ident(t.to_string()),
        };
        Token { kind, span: Span { start, end: self.i } }
    }

    fn string_lit(&mut self) -> Token {
        let q = self.bump().unwrap(); // " or '
        let start = self.i - 1;
        let mut s = String::new(); let mut esc = false;
        while let Some(b) = self.bump() {
            if esc { s.push(b as char); esc = false; continue; }
            if b == b'\\' { esc = true; continue; }
            if b == q { break; }
            s.push(b as char);
        }
        Token { kind: TokKind::Str(s), span: Span { start, end: self.i } }
    }

    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_ws_comments();
        let start = self.i;
        let b = self.peek()?;
        if b == b'"' || b == b'\'' { return Some(self.string_lit()); }
        if self.starts_with("=>") { self.i += 2; return Some(Token{kind:TokKind::FatArrow, span:Span{start,end:self.i}}); }
        let kind = match b {
            b'('=>{self.i+=1;TokKind::LParen}, b')'=>{self.i+=1;TokKind::RParen},
            b'{' =>{self.i+=1;TokKind::LBrace}, b'}'=>{self.i+=1;TokKind::RBrace},
            b',' =>{self.i+=1;TokKind::Comma},  b':' =>{self.i+=1;TokKind::Colon},
            b';' =>{self.i+=1;TokKind::Semicolon}, b'*'=>{self.i+=1;TokKind::Star},
            b'=' =>{self.i+=1;TokKind::Eq}, b'.' =>{self.i+=1;TokKind::Dot},
            b'?' =>{self.i+=1;TokKind::Question},
            _ => {
                if b.is_ascii_alphabetic() || b == b'_' { return Some(self.ident_or_kw()); }
                if b.is_ascii_digit() {
                    let s = self.i; self.i += 1;
                    while let Some(bb) = self.peek() { if bb.is_ascii_digit() || bb==b'_' { self.i+=1; } else {break;} }
                    return Some(Token{kind:TokKind::Number(self.src[s..self.i].to_string()), span:Span{start:s,end:self.i}});
                }
                self.i += 1; return self.next_token();
            }
        };
        Some(Token{kind, span: Span{start, end:self.i}})
    }

    pub fn tokenize(mut self) -> Vec<Token> {
        let mut v = Vec::new();
        while let Some(t) = self.next_token() { v.push(t); }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::{Lexer, TokKind};

    #[test]
    fn tokenizes_keywords_punctuation_and_skips_comments() {
        let src = r#"
            // leading comment
            export const add = move (x: i32, y?: i32) => "ok";
            /* trailing comment */
        "#;

        let kinds = Lexer::new(src)
            .tokenize()
            .into_iter()
            .map(|t| t.kind)
            .collect::<Vec<_>>();

        assert_eq!(
            kinds,
            vec![
                TokKind::Export,
                TokKind::Const,
                TokKind::Ident("add".into()),
                TokKind::Eq,
                TokKind::Move,
                TokKind::LParen,
                TokKind::Ident("x".into()),
                TokKind::Colon,
                TokKind::Ident("i32".into()),
                TokKind::Comma,
                TokKind::Ident("y".into()),
                TokKind::Question,
                TokKind::Colon,
                TokKind::Ident("i32".into()),
                TokKind::RParen,
                TokKind::FatArrow,
                TokKind::Str("ok".into()),
                TokKind::Semicolon,
            ]
        );
    }

    #[test]
    fn preserves_token_spans_for_strings_and_numbers() {
        let src = r#"let value = "hi"; 123_456"#;
        let tokens = Lexer::new(src).tokenize();

        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 3);
        assert_eq!(&src[tokens[3].span.start..tokens[3].span.end], "\"hi\"");
        assert_eq!(&src[tokens[5].span.start..tokens[5].span.end], "123_456");
    }
}
