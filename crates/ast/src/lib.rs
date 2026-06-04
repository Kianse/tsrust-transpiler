use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Span { pub start: usize, pub end: usize }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module { pub items: Vec<Item> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Item {
    Import(ImportDecl),
    ExportStar(ExportStar),
    ReExport(ReExportDecl),
    ExportedFn(FnDecl),
    Fn(FnDecl),
    ExportedArrow(ArrowDecl),
    VarArrow(VarArrowDecl),
    ReExportLocal(ExportLocal),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportDecl {
    pub items: Vec<ImportItem>,
    pub from: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImportItem {
    Simple(String),
    Aliased { name: String, alias: String },
    Nested { path: String }, // bar::{Baz, qux as q}
    Star,
    StarAs(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportStar { pub from: String, pub span: Span }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReExportDecl {
    pub items: Vec<String>, pub from: String, pub span: Span
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportLocal {
    pub items: Vec<String>, pub span: Span
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FnDecl {
    pub exported: bool,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body_src: String, // body without outer { }
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArrowDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: ArrowBody,
    pub is_exported: bool,
    pub is_move: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarArrowDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: ArrowBody,
    pub is_move: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArrowBody {
    Block(String), // body without { }
    Expr(String),  // inline expr
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeExpr { pub text: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamKind { Normal, Optional, Defaulted(String) }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: String,     // inner type text (without Option<>)
    pub kind: ParamKind,
    pub span: Span,
}
