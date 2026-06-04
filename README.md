
# tsrust — TypeScript-ish syntax → Rust

> ⚠️ Early prototype. Expect sharp edges.

`tsrust` lets you write a tiny, TypeScript-flavored syntax and transpile it to Rust.  
It’s *not* JS/TS → Rust; it’s a TS-like language with Rust types & semantics that compiles to Rust code.

## Why?
- Familiar authoring experience for people coming from TS.
- Generates real Rust you can inspect, compile, and ship.
- Experiments in ergonomic error mapping (tsrust source ↔ rustc diagnostics).

## Workspace layout
- `crates/transpiler` — string-rewrite prototype (imports/exports, return-tail, etc.)
- `crates/build-driver` — `build.rs` helper to transpile `*.tsrust` files
- `crates/sourcemap` — simple line map format
- `tooling/error-post` — post-process `cargo check` errors back to `.tsrust` spans
- `tooling/tsrustc` — one-shot CLI compiler (transpile → mini cargo app → build)
- `examples/hello_tsrust` — sample project

## Quickstart
```bash
# Run error-post on the example (maps errors back to .tsrust)
cargo run -p tsrust-error-post --quiet

# One-shot compile:
cargo run -p tsrustc -- \
  -i examples/hello_tsrust/src \
  -o ./hello \
  --entry 'main::add_then_double(2,3)'
./hello
```

## Supported Today

`tsrust` currently supports a deliberately small surface area. The implementation is strongest when you stay inside this subset.

### Top-level items
- `import { A } from "./path"`
- `import { A as B } from "./path"`
- `import * as ns from "std.io"`
- nested import groups like `import { foo, bar::{Baz, qux as q} } from "x"`
- `export function name(...)`
- `export const name = (...) : T => ...`
- `export { Name }`
- `export { Name } from "./path"`
- `export * from "./path"`
- plain top-level `function name(...)`

### Function and body rewrites
- paren-tail returns: `(expr)`
- `return(expr)` and `return;`
- `for (x of xs)`
- `switch (...) { case ... }`
- local arrow functions inside bodies: `let f = (x: T): U => ...`
- `move` closures in exported and local arrows
- optional params: `b?: i32`
- default params: `a: i32 = 1`
- nullish coalescing: `a ?? b`
- optional chaining: `user?.name?.len()`

### Paths and naming
- relative paths like `./util` and `../models`
- dot-separated module paths like `std.collections`
- hyphenated module segments are rewritten to underscores in Rust output

### Diagnostics and tooling
- line-oriented mapping from generated Rust diagnostics back to `.tsrust`
- `build.rs`-driven transpilation for example projects
- one-shot CLI compilation via `tsrustc`
- VS Code syntax highlighting for `.tsrust`

## Explicit Non-Goals

These are not part of the current project contract:

- transpiling arbitrary JavaScript or TypeScript to Rust
- matching JavaScript runtime semantics
- full TypeScript syntax coverage
- classes, interfaces, enums, decorators, namespaces, JSX, or full module syntax
- generics, type inference, or a real type checker
- robust macro/interoperability design beyond simple Rust-shaped bodies
- precise char-level sourcemaps
- production-grade compiler guarantees

## Current Boundaries

The current architecture is a hybrid:

- top-level structure is parsed into AST items
- many body-level features are still implemented as targeted string rewrites
- unsupported syntax should be treated as outside the stable subset, even if some inputs happen to pass through

If you want predictable behavior, stay within the supported subset above and treat everything else as experimental.


## VS Code syntax highlighting

We ship a minimal TextMate grammar + snippets for `.tsrust` under `editors/vscode/tsrust/`.

**What you get**

* Keywords: `function`, `export`, `import`, `from`, `as`, `return`, `async`, `await`, `for`, `of`, `let`, `const`, plus Rust-y tokens (`pub`, `mod`, `use`, …).
* Types: `i32`, `u64`, `bool`, `f32`, `()`, `String`, etc.
* Macros: any `ident!(` with a special case for `println!`.
* Strings, numbers, `//` and `/* … */` comments.
* Snippets: `fn` (export function template), `pln` (`println!`), `imp` (import line).

**Repo layout**

```tree
editors/vscode/tsrust/
├─ package.json
├─ language-configuration.json
├─ syntaxes/
│  └─ tsrust.tmLanguage.json
└─ snippets/
   └─ tsrust.json
```

**Run locally (Extension Development Host)**

1. Open `editors/vscode/tsrust/` in VS Code.
2. Press **F5** (Run → Start Debugging) to launch an *Extension Development Host*.
3. Open a `.tsrust` file there to see highlighting.

**Install from a local VSIX (optional)**

```bash
npm i -g @vscode/vsce
cd editors/vscode/tsrust
vsce package
# then in VS Code: Extensions → … → Install from VSIX… → choose the .vsix
```

**Bonus: Markdown code blocks**
If you want `.tsrust` code blocks highlighted in Markdown previews, add this to your settings:

```json
"markdown.extension.syntax.highlighting": true
```



## Status

* ✅ import/export forms (`import {A as B}`, `import * as ns`, `export * from`, etc.)
* ✅ sugar: `for (x of xs)`, `(expr)` tail returns, `return;` → `return ();`
* ✅ error mapping back to `.tsrust` (file:line:col; caret under culprit)
* ✅ direct tests for lexer, parser, lowering, and rewrite layers
* 🚧 actual parser/AST, type-checking, full sourcemaps, macros/interops



## License: 

[Apache-2.0](./LICENSE)
