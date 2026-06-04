# tsrust — VS Code syntax highlighting

Minimal TextMate grammar + snippets for `.tsrust` files (TypeScript-ish surface with Rust types/macros).

## Install (local dev)

1. Open this folder in VS Code:

```
tsrust/
└─ editors/
   └─ vscode/
      └─ tsrust/
         ├─ package.json
         ├─ language-configuration.json
         ├─ syntaxes/tsrust.tmLanguage.json
         └─ snippets/tsrust.json
```

2. Press **F5** (Run → Start Debugging) to launch an **Extension Development Host**.
   Open a `.tsrust` file there to see highlighting.

### Alternative: install from a local VSIX

* Install `vsce`:

  ```
  npm i -g @vscode/vsce
  ```
* From this folder:

  ```
  vsce package
  ```

  This creates something like `tsrust-syntax-0.1.0.vsix`.
* In VS Code: **Extensions → … → Install from VSIX…** and select the file.
* Before publishing publicly, replace the placeholder `publisher` field in `package.json`.

## What’s highlighted

* **Keywords:** `function`, `export`, `import`, `from`, `as`, `return`, `async`, `await`, `for`, `of`, `let`, `const`, plus a few Rusty ones (`pub`, `mod`, `use`, `match`, `while`, `break`, `continue`).
* **Types:** `i32`, `u64`, `bool`, `f32`, `()`, `String`, etc. (extend as the language grows).
* **Macros:** any `ident!(`; special-cased `println!`.
* **Literals:** single/double/backtick strings with escapes, basic numbers.
* **Comments:** `//` and `/* … */`.

## Snippets

* `fn` → export function boilerplate with paren-tail return.
* `pln` → `println!("…");`
* `imp` → `import { Name } from "./path";`

## File association

The extension registers the **.tsrust** extension and the `tsrust` language id automatically.
If you also want to colorize inline code blocks in Markdown, add to your VS Code settings:

```json
"markdown.extension.syntax.highlighting": true
```

## Tips for contributors

* Keep grammar changes minimal and test with `samples/*.tsrust` files (feel free to add one).
* When you add new language features, update:

  * `syntaxes/tsrust.tmLanguage.json` (keywords/types/macros)
  * `snippets/tsrust.json` (optional)
  * This README
* Validate the JSON grammars:

  ```
  jq . syntaxes/tsrust.tmLanguage.json > /dev/null
  ```
