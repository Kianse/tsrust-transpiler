use anyhow::{Context, Result};
use regex::Regex;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Default, Debug)]
struct ExportIndex {
    // module key = normalized relative path from src_root, e.g. "util.tsrust" or "dir/foo.tsrust"
    per_module: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    files: Vec<(String /*stem*/, PathBuf /*abs path in OUT_DIR*/)>,
}



fn normalize_rel_path(from_dir: &Path, spec: &str) -> Option<String> {
    // only handle relative specs: "./..." or "../..."
    if !(spec.starts_with("./") || spec.starts_with("../")) {
        return None;
    }
    // support dot-separated segments like "./util.math" used in tests
    let mut steps = Vec::<&str>::new();
    let mut rest = spec;

    // peel ../
    let mut base = from_dir.to_path_buf();
    while rest.starts_with("../") {
        base = base.parent().unwrap_or(from_dir).to_path_buf();
        rest = &rest[3..];
    }
    if rest.starts_with("./") {
        rest = &rest[2..];
    }
    // split both '/' and '.'
    for seg in rest.split(|c| c == '/' || c == '.').filter(|s| !s.is_empty()) {
        steps.push(seg);
    }
    let mut p = base;
    for s in steps {
        p = p.join(s);
    }
    p.set_extension("tsrust");

    // make path relative to src root later; here just stringify
    Some(p.to_string_lossy().replace('\\', "/"))
}

// very small export scanner (local exports only)
fn scan_exports(src_text: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();

    // export function Foo(
    let re_fn = Regex::new(r#"(?m)^\s*export\s+function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("#).unwrap();
    for c in re_fn.captures_iter(src_text) {
        set.insert(c[1].to_string());
    }

    // export const Foo = ..., export let Foo = ...
    let re_const = Regex::new(
        r#"(?m)^\s*export\s+(?:const|let)\s+([A-Za-z_][A-Za-z0-9_]*)\s*="#
    ).unwrap();
    for c in re_const.captures_iter(src_text) {
        set.insert(c[1].to_string());
    }

    // export { A, B as C };
    let re_list = Regex::new(r#"(?m)^\s*export\s*\{([^}]+)\}\s*;?\s*$"#).unwrap();
    for c in re_list.captures_iter(src_text) {
        for item in c[1].split(',') {
            let item = item.trim();
            if item.is_empty() { continue; }
            // keep alias (right side) visible: "A as B" exports name B
            if let Some((_, rhs)) = item.split_once(" as ") {
                set.insert(rhs.trim().to_string());
            } else {
                set.insert(item.to_string());
            }
        }
    }

    set
}

fn build_export_index(src_root: &Path, files: &[PathBuf]) -> ExportIndex {
    let mut idx = ExportIndex::default();
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else { continue; };
        let exports = scan_exports(&text);

        // key relative to src_root
        let rel = f.strip_prefix(src_root).unwrap_or(f);
        let key = rel.to_string_lossy().replace('\\', "/");
        idx.per_module.insert(key, exports);
    }
    idx
}

fn import_items_of_line(line: &str) -> Option<(Vec<String>, String)> {
    // matches: import { A, B as C } from "path";
    let re = Regex::new(r#"^\s*import\s*\{([^}]*)\}\s*from\s*"([^"]+)"\s*;?\s*$"#).unwrap();
    let caps = re.captures(line)?;
    let items = caps[1].split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            // keep only the **imported binding name in this file**
            // e.g. "Foo as Bar" -> "Bar"
            if let Some((_, rhs)) = s.split_once(" as ") { rhs.trim().to_string() }
            else { s.to_string() }
        })
        .collect::<Vec<_>>();
    let spec = caps[2].to_string();
    Some((items, spec))
}

fn find_col(line: &str, needle: &str) -> usize {
    // 1-based visual column of the first occurrence (fallback 1)
    if let Some(pos) = line.find(needle) {
        line[..pos].chars().count() + 1
    } else {
        1
    }
}

fn validate_imports(
    src_root: &Path,
    files: &[PathBuf],
    idx: &ExportIndex,
) -> anyhow::Result<()> {
    // Build quick reverse map: symbol -> modules exporting it (for hinting)
    let mut who_has: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (module, set) in &idx.per_module {
        for sym in set {
            who_has.entry(sym.clone()).or_default().push(module.clone());
        }
    }

    let mut errors = Vec::<anyhow::Error>::new();

    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else { continue; };
        let from_dir = file.parent().unwrap_or(src_root);

        for (i, raw_line) in text.lines().enumerate() {
            if let Some((items, spec)) = import_items_of_line(raw_line) {
                // only validate relative targets ("./" or "../")
                if !(spec.starts_with("./") || spec.starts_with("../")) {
                    continue;
                }

                // resolve to a src path string, then normalize relative-to-root key
                let Some(abs_str) = normalize_rel_path(from_dir, &spec) else { continue; };
                // key relative to src_root
                let target_key = Path::new(&abs_str)
                    .strip_prefix(src_root)
                    .unwrap_or(Path::new(&abs_str))
                    .to_string_lossy()
                    .replace('\\', "/");

                let exported = idx.per_module.get(&target_key);

                for it in items {
                    // namespace "*" and aliased "*" already skipped by our parser
                    if let Some(set) = exported {
                        if set.contains(&it) { continue; }
                    }
                    // not found → build message
                    let line_no = i + 1;
                    let col = find_col(raw_line, &it);
                    let mut msg = format!(
                        "{}:{}:{}: `{}`
is not exported by \"{}\"",
                        file.display(),
                        line_no,
                        col,
                        it,
                        spec
                    );
                    if let Some(cands) = who_has.get(&it) {
                        // show short hints with ./-style paths if possible
                        if !cands.is_empty() {
                            let hint = cands
                                .iter()
                                .map(|k| format!("./{}", Path::new(k).with_extension("").to_string_lossy()))
                                .collect::<Vec<_>>()
                                .join(", ");
                            msg.push_str(&format!("\n        hint: found `{}` in {}", it, hint));
                        }
                    }
                    errors.push(anyhow::anyhow!(msg));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        // chain them together
        let mut agg = String::new();
        for (n, e) in errors.iter().enumerate() {
            if n > 0 { agg.push_str("\n\n"); }
            agg.push_str(&format!("{e}"));
        }
        Err(anyhow::anyhow!(agg))
    }
}



fn find_fn_starts_src(src_text: &str) -> Vec<(String, usize)> {
    // Support both `function foo(` and `export function foo(`
    let re = regex::Regex::new(r#"(?m)^\s*(?:export\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("#)
        .unwrap();

    let mut out = Vec::new();
    for caps in re.captures_iter(src_text) {
        let name = caps.get(1).unwrap().as_str().to_string();
        let line = src_text[..caps.get(0).unwrap().start()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1; // 1-based
        out.push((name, line));
    }
    out
}

fn find_fn_starts_gen(gen_text: &str) -> Vec<(String, usize)> {
    // fn NAME(...) -> ... {   (or no ->)
    let re = Regex::new(r#"(?m)^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("#).unwrap();
    let mut out = Vec::new();
    for caps in re.captures_iter(gen_text) {
        let name = caps.get(1).unwrap().as_str().to_string();
        let line = gen_text[..caps.get(0).unwrap().start()]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1;
        out.push((name, line));
    }
    out
}

/// Compile all `.tsrust` files under `src_dir` into OUT_DIR/tsrust_gen,
/// run a friendly import/export validation, and emit a module file
/// OUT_DIR/tsrust_mods.rs that includes them.
pub fn compile_project(src_dir: &str) -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").context("OUT_DIR not set")?);
    let gen_dir = out_dir.join("tsrust_gen");
    if !gen_dir.exists() {
        fs::create_dir_all(&gen_dir)?;
    }

    let src_root = PathBuf::from(src_dir);

    // 1) Collect all .tsrust files under src_root (and tell cargo to watch them)
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(&src_root) {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("tsrust") {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        files.push(path.to_path_buf());
    }

    if files.is_empty() {
        anyhow::bail!("No .tsrust files found under {}", src_root.display());
    }

    // 2) Validate imports BEFORE generating any Rust files
    let export_index = build_export_index(&src_root, &files);
    validate_imports(&src_root, &files, &export_index)?;

    // 3) Transpile each file and write its mirrored .rs into OUT_DIR/tsrust_gen
    let mut generated: Vec<(String /*module name*/, PathBuf /*abs path in OUT_DIR*/)> = Vec::new();

    for path in &files {
        let src = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let rs = tsrust_transpiler::transpile_str(&src, &path.display().to_string())?;

        // Mirror the directory layout under gen_dir
        let rel = path.strip_prefix(&src_root).unwrap();
        let mut out_rs = gen_dir.join(rel);
        out_rs.set_extension("rs");
        if let Some(parent) = out_rs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_rs, rs)
            .with_context(|| format!("writing {}", out_rs.display()))?;

        // Module name is the file stem
        let module_name = rel
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mod")
            .to_string();

        generated.push((module_name, out_rs));
    }

    // 4) Build a folder tree mirroring src/ and emit module glue
    fn insert(tree: &mut Node, rel: &Path, out_rs: PathBuf) {
        let comps: Vec<_> = rel.components().collect();
        if comps.is_empty() {
            return;
        }

        let mut cur = tree;
        for (i, comp) in comps.iter().enumerate() {
            use std::path::Component::*;
            let is_last = i == comps.len() - 1;
            match comp {
                Normal(os) if !is_last => {
                    let key = os.to_string_lossy().to_string();
                    cur = cur.children.entry(key).or_default();
                }
                Normal(os) => {
                    let stem = Path::new(os)
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    cur.files.push((stem, out_rs.clone()));
                }
                _ => {} // skip special components
            }
        }
    }

    let mut root_tree = Node::default();
    for (_module_name, out_rs) in &generated {
        // out_rs is under OUT_DIR/tsrust_gen/<rel>.rs
        let rel_from_out = out_rs.strip_prefix(&out_dir).unwrap();
        let rel_after_gen = rel_from_out.strip_prefix("tsrust_gen").unwrap();
        insert(&mut root_tree, rel_after_gen, out_rs.clone());
    }

    fn emit(tree: &Node, out_dir: &Path, buf: &mut String) {
        for (name, child) in &tree.children {
            buf.push_str(&format!("pub mod {} {{\n", name));
            emit(child, out_dir, buf);
            buf.push_str("}\n");
        }
        for (stem, out_rs) in &tree.files {
            let rel_from_out = out_rs
                .strip_prefix(out_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            buf.push_str(&format!(
                "pub mod {name} {{ include!(concat!(env!(\"TSRUST_OUT\"), \"/{rel}\")); }}\n",
                name = stem,
                rel = rel_from_out
            ));
        }
    }

    // Write glue once
    let mods_path = out_dir.join("tsrust_mods.rs");
    let mut mods_src = String::new();
    emit(&root_tree, &out_dir, &mut mods_src);
    fs::write(&mods_path, mods_src)
        .with_context(|| format!("writing {}", mods_path.display()))?;

    // 5) Build a simple line-map JSON for nicer error pointing
    #[derive(Debug, serde::Serialize)]
    struct FnLineMap {
        name: String,
        src_start: usize,
        gen_start: usize,
    }
    #[derive(Debug, serde::Serialize)]
    struct FileLineMap {
        gen_file: String,
        src_file: String,
        fns: Vec<FnLineMap>,
    }
    #[derive(Debug, serde::Serialize)]
    struct CrateLineMap {
        files: Vec<FileLineMap>,
    }

    // helpers already exist in this module:
    //   - find_fn_starts_src
    //   - find_fn_starts_gen

    let mut map = CrateLineMap { files: Vec::new() };
    for (_module_name, out_rs) in &generated {
        let gen_abs = out_rs.clone();

        // Derive original .tsrust source path from mirrored structure
        let rel_from_out = out_rs.strip_prefix(&out_dir).unwrap(); // tsrust_gen/<rel>.rs
        let rel_after_gen = rel_from_out.strip_prefix("tsrust_gen").unwrap(); // /<rel>.rs
        let mut src_candidate = PathBuf::from(src_root.clone());
        src_candidate.push(rel_after_gen);
        src_candidate.set_extension("tsrust");

        let gen_text = fs::read_to_string(out_rs)?;
        let src_text = fs::read_to_string(&src_candidate).unwrap_or_default();

        let mut fns = Vec::new();
        let src_fns = find_fn_starts_src(&src_text);
        let gen_fns = find_fn_starts_gen(&gen_text);

        // Rough join by function name
        for (name, src_start) in src_fns {
            if let Some((_, gen_start)) = gen_fns.iter().find(|(n, _)| n == &name) {
                fns.push(FnLineMap {
                    name: name.clone(),
                    src_start,
                    gen_start: *gen_start,
                });
            }
        }

        map.files.push(FileLineMap {
            gen_file: gen_abs.to_string_lossy().into_owned(),
            src_file: src_candidate.to_string_lossy().into_owned(),
            fns,
        });
    }

    let map_path = out_dir.join("tsrust_line_map.json");
    fs::write(&map_path, serde_json::to_vec_pretty(&map)?)?;
    println!(
        "cargo:warning=tsrust: wrote line map to {}",
        map_path.display()
    );

    Ok(())
}
