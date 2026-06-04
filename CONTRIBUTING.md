
# Contributing to tsrust

Thanks for your interest in improving **tsrust**! 
This document explains how to set up your environment, make changes, and submit pull requests.

> tl;dr
> - Use stable Rust
> - `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace` must pass
> - Keep PRs small and well-tested
> - Stay inside the documented supported subset unless the PR explicitly expands it
> - Document any syntax or behavior changes in the README

---

## Code of Conduct

We follow the [Contributor Covenant](./CODE_OF_CONDUCT.md).  
By participating, you agree to uphold this code.

---

## Getting Started

### Prerequisites
- **Rust** (stable toolchain)  
  Install via <https://rustup.rs>.  
  Optional: `cargo install cargo-nextest` for faster tests.

### Repository layout (high level)
- `crates/transpiler` — string-rewrite prototype (imports/exports, return-tail, etc.)
- `crates/build-driver` — `build.rs` helper to transpile `*.tsrust` files
- `crates/sourcemap` — simple line-map format
- `tooling/error-post` — post-processes `cargo check` errors back to `.tsrust` spans
- `tooling/tsrustc` — one-shot CLI (transpile → mini cargo app → build)
- `examples/hello_tsrust` — sample project

### One-time setup
```bash
git clone https://github.com/Kianse/tsrust-transpiler
cd tsrust
cargo build --workspace
```

---

## Common Tasks

### Format, Lint, Test (what CI runs)

```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
cargo test --workspace --all-features --no-fail-fast
```

### Run the example with error mapping

```bash
cargo run -p tsrust-error-post --quiet
```

### Use the one-shot compiler

```bash
# Build to a binary, call an entry expression, and keep generated .rs
cargo run -p tsrustc -- \
  -i examples/hello_tsrust/src \
  -o ./hello \
  --entry 'main::add_then_double(2,3)' \
  --keep-gen
./hello
# Generated .rs lives in: examples/hello_tsrust/out/tsrust_gen
```


---

## Roadmap
- Parser/AST (ditch regex passes)
- Proper symbol & path resolution
- Type system MVP (subset of Rust types + inference)
- Full sourcemaps (char-level), multiple spans per diag
- Rich diagnostics (multi-span notes, suggests)
- Interop story (call Rust crates from tsrust, and vice versa)
- Language guide & reference

---

## Making Changes

### Small, focused PRs

Prefer incremental, reviewable changes over mega-PRs. If you’re unsure about design, open a **Draft PR** or start a **Discussion** first.

### Tests

* Put unit tests next to the code you change (e.g., `#[cfg(test)]` module in the same file).
* Add example-level tests under `examples/` if useful.
* For new syntax transforms, add focused tests in `crates/transpiler/src/lib.rs`’s test module.
* Ensure tests pass locally before opening a PR.

### Style

* Run `cargo fmt --all`.
* Keep the regex passes readable and comment intent. Prefer clarity over cleverness.
* Avoid panics in library code; bubble up `anyhow::Result` with context.

### Commit messages

* Clear and descriptive. Conventional commits are welcome but not required.
* Example: `transpiler: support 'export * from "./x"'` or `error-post: align carets with token`.

### Documentation

* Update the **README** when you add/change syntax or flags.
* Keep the **Supported Today**, **Explicit Non-Goals**, and **Current Boundaries** sections accurate.
* Add comments where behavior could surprise future contributors.

---

## Proposing Features

1. Search existing issues/discussions.
2. Open a **Feature Request** with:

   * Problem/use case
   * Proposed syntax/behavior
   * Examples
   * Alternatives considered

For larger changes, expect some iterative design before coding.

---

## Release Process (project maintainers)

* Ensure CI is green.
* Update README/CHANGELOG (if present).
* Tag semver release: `v0.1.x`.
* Announce in Discussions and community channels.

---

## Licensing

By contributing, you agree that your contributions will be licensed under the
[Apache-2.0 License](./LICENSE).

---

## Questions?

* Open a GitHub **Discussion** for Q&A/ideas.
* File an **Issue** for bugs (include a minimal `.tsrust` snippet + exact command).

Thanks again for helping make tsrust better!
