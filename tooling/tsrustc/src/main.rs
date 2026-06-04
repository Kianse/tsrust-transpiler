use anyhow::{Context, Result};
use clap::Parser;
use std::{
    fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use regex::Regex;

// ---- Line-map structs (same schema as error-post expects) ----
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct FnLineMap {
    name: String,
    src_start: usize,
    gen_start: usize,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct FileLineMap {
    gen_file: String,
    src_file: String,
    fns: Vec<FnLineMap>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CrateLineMap {
    files: Vec<FileLineMap>,
}


// Find `function foo(` (or `export function foo(`) in .tsrust
fn find_fn_starts_src(src_text: &str) -> Vec<(String, usize)> {
    let re = Regex::new(r#"(?m)^\s*(?:export\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("#).unwrap();
    let mut out = Vec::new();
    for caps in re.captures_iter(src_text) {
        let name = caps[1].to_string();
        let line = src_text[..caps.get(0).unwrap().start()].bytes().filter(|&b| b==b'\n').count()+1;
        out.push((name, line));
    }
    out
}

// Find `fn foo(` (or `pub fn foo(`) in generated .rs
fn find_fn_starts_gen(gen_text: &str) -> Vec<(String, usize)> {
    let re = Regex::new(r#"(?m)^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("#).unwrap();
    let mut out = Vec::new();
    for caps in re.captures_iter(gen_text) {
        let name = caps[1].to_string();
        let line = gen_text[..caps.get(0).unwrap().start()].bytes().filter(|&b| b==b'\n').count()+1;
        out.push((name, line));
    }
    out
}

// Write a line-map file into <app_dir>/target/tsrust_line_map.json (where the rewriter will find it)
fn write_line_map(app_dir: &Path, src_root: &Path, modules: &[(String, PathBuf)]) -> anyhow::Result<()> {
    use std::fs;

    let mut map = CrateLineMap { files: Vec::new() };

    for (_module_name, gen_rs) in modules {
        // gen_rs is an absolute path in our temp project’s gen dir.
        // Reconstruct the original source path by swapping .rs -> .tsrust and “tsrust_gen/…” -> “src_root/…”
        // We encoded it during transpile_all using the relative path under src_root.
        // Here we can derive it by mirroring path under app_dir/tsrust_gen back to src_root.
        // We stored `modules` using the same relative structure, so do this:
        //   app_dir/tsrust_gen/<rel>.rs  ->  src_root/<rel>.tsrust
        let rel = gen_rs.strip_prefix(app_dir.join("tsrust_gen")).unwrap_or(gen_rs);
        let mut src_path = src_root.join(rel);
        src_path.set_extension("tsrust");

        let gen_text = fs::read_to_string(gen_rs)?;
        let src_text = fs::read_to_string(&src_path).unwrap_or_default();

        let mut fns = Vec::new();
        let src_fns = find_fn_starts_src(&src_text);
        let gen_fns = find_fn_starts_gen(&gen_text);
        for (name, src_start) in src_fns {
            if let Some((_, gen_start)) = gen_fns.iter().find(|(n, _)| n == &name) {
                fns.push(FnLineMap { name, src_start, gen_start: *gen_start });
            }
        }

        map.files.push(FileLineMap {
            gen_file: gen_rs.to_string_lossy().into_owned(),
            src_file: src_path.to_string_lossy().into_owned(),
            fns,
        });
    }

    let target_dir = app_dir.join("target");
    std::fs::create_dir_all(&target_dir)?;
    let map_path = target_dir.join("tsrust_line_map.json");
    std::fs::write(&map_path, serde_json::to_vec_pretty(&map)?)?;
    Ok(())
}

// --- minimal stderr rewriter (trimmed from tooling/error-post) ---
fn strip_ansi(s: &str) -> String {
    let re = Regex::new(r"\x1B\[[0-9;]*[mK]").unwrap();
    re.replace_all(s, "").into_owned()
}

fn read_line_1_based(path: &str, line: usize) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut r = BufReader::new(f);
    let mut s = String::new();
    for _ in 1..line { s.clear(); if r.read_line(&mut s).ok()? == 0 { return None; } }
    s.clear();
    if r.read_line(&mut s).ok()? == 0 { return None; }
    while matches!(s.chars().last(), Some('\n' | '\r')) { s.pop(); }
    Some(s)
}

fn expand_tabs_to_cols(s: &str, tabw: usize) -> String {
    let mut out = String::new(); let mut col=0usize;
    for ch in s.chars() {
        if ch=='\t' { let n=tabw-(col%tabw); for _ in 0..n { out.push(' '); col+=1; } }
        else { out.push(ch); col+=1; }
    }
    out
}

fn gen_suffix(p: &str) -> Option<String> {
    let norm = p.replace('\\', "/");
    norm.rsplit_once("tsrust_gen/").map(|(_, s)| s.to_string())
}

fn map_line(maps: &[CrateLineMap], gen_path: &str, gen_line: usize) -> Option<(String, usize)> {
    let want_suf = gen_suffix(gen_path);
    for m in maps {
        for f in &m.files {
            let matches = if let (Some(a), Some(b)) = (want_suf.as_ref(), gen_suffix(&f.gen_file).as_ref()) {
                a == b
            } else {
                f.gen_file == gen_path
            };
            if !matches { continue; }
            if f.fns.is_empty() { return Some((f.src_file.clone(), gen_line)); }
            let mut best: Option<&FnLineMap> = None;
            for fun in &f.fns {
                if fun.gen_start <= gen_line {
                    best = match best {
                        None => Some(fun),
                        Some(prev) if fun.gen_start > prev.gen_start => Some(fun),
                        o => o,
                    };
                }
            }
            if let Some(fun) = best {
                let delta = gen_line.saturating_sub(fun.gen_start);
                return Some((f.src_file.clone(), fun.src_start + delta));
            } else {
                return Some((f.src_file.clone(), gen_line));
            }
        }
    }
    None
}

fn load_line_maps_from(app_dir: &Path) -> anyhow::Result<Vec<CrateLineMap>> {
    let mut maps = Vec::new();
    // we write it to app_dir/target/tsrust_line_map.json
    let p = app_dir.join("target").join("tsrust_line_map.json");
    if p.is_file() {
        if let Ok(data) = std::fs::read(p) {
            if let Ok(map) = serde_json::from_slice::<CrateLineMap>(&data) {
                maps.push(map);
            }
        }
    }
    Ok(maps)
}

fn rewrite_stderr_for_app(app_dir: &Path, stderr: &str) -> String {
    let maps = load_line_maps_from(app_dir).unwrap_or_default();
    if maps.is_empty() { return stderr.to_string(); }

    let clean = strip_ansi(stderr);
    let arrow = Regex::new(r"(?m)^ *--> ([^:\n]+):(\d+):(\d+)").unwrap();

    let mut out = String::with_capacity(clean.len());
    let mut last_idx = 0usize;

    let mut iter = arrow.captures_iter(&clean).peekable();
    while let Some(m) = iter.next() {
        let whole = m.get(0).unwrap();
        let gen_path = m.get(1).unwrap().as_str();
        let line: usize = m.get(2).unwrap().as_str().parse().unwrap_or(1);
        let col: usize = m.get(3).unwrap().as_str().parse().unwrap_or(1);

        // push prefix
        out.push_str(&clean[last_idx..whole.start()]);

        // map
        let (mapped_path, mapped_line) = map_line(&maps, gen_path, line).unwrap_or((gen_path.to_string(), line));
        out.push_str(&format!("--> {}:{}:{}", mapped_path, mapped_line, col));

        // try to rewrite snippet right after
        let mut cursor = whole.end();
        let after = &clean[cursor..];
        let pipe_line = Regex::new(r"(?s)^\n( *\|)\n").unwrap();
        let code_line = Regex::new(r"(?m)^(\s*)(\d+)(\s*\|\s)(.*)").unwrap();

        if let Some(pl_caps) = pipe_line.captures(after) {
            let whole = pl_caps.get(0).unwrap(); out.push_str(whole.as_str()); cursor += whole.end();
            let after2 = &clean[cursor..];
            if let Some(cl_caps) = code_line.captures(after2) {
                let cl = cl_caps.get(0).unwrap();
                out.push_str(&after2[..cl.start()]);
                let lead_ws = cl_caps.get(1).unwrap().as_str();
                let post_bar = cl_caps.get(3).unwrap().as_str();
                out.push_str(lead_ws);
                out.push_str(&mapped_line.to_string());
                out.push_str(post_bar);

                let printed = read_line_1_based(&mapped_path, mapped_line)
                    .map(|s| expand_tabs_to_cols(&s, 4))
                    .unwrap_or_else(|| cl_caps.get(4).unwrap().as_str().to_string());
                out.push_str(&printed);
                cursor += cl.end();

                // caret line: keep as-is (column may not line up perfectly, good enough)
                let after3 = &clean[cursor..];
                let caret_line = Regex::new(r"(?m)^(\s*\|\s)(\s*)(\^+)(.*)$").unwrap();
                if let Some(caps) = caret_line.captures(after3) {
                    let whole = caps.get(0).unwrap();
                    out.push_str(&after3[..whole.end()]);
                    cursor += whole.end();
                }
            }
        }

        last_idx = cursor;
    }
    out.push_str(&clean[last_idx..]);
    out
}

/// Simple one-shot compiler for .tsrust sources.
///
/// Examples:
///   tsrustc -i examples/hello_tsrust/src -o ./hello
///   tsrustc -i examples/hello_tsrust/src -o ./hello --entry 'main::add_then_double(2,3)'
///   tsrustc -i examples/hello_tsrust/src -o ./hello --debug
#[derive(Parser, Debug)]
#[command(name = "tsrustc")]
#[command(version)]
#[command(about = "Compile .tsrust sources into a single Rust binary", long_about = None)]
struct Args {
    /// Input: a .tsrust file or a directory containing .tsrust files
    #[arg(short = 'i', long = "input")]
    input: PathBuf,

    /// Output binary path (copied here after build completes)
    #[arg(short = 'o', long = "output", default_value = "a.out")]
    output: PathBuf,

    /// Optional entry expression to call from generated main(), e.g. "main::add_then_double(2,3)"
    /// If not given, a minimal empty main() is generated.
    #[arg(long = "entry")]
    entry: Option<String>,

    /// Keep the build folder next to the input (at <input_dir>/.tsrust_build)
    /// rather than using a temporary directory.
    #[arg(long = "debug")]
    debug_keep_build: bool,

    /// Build in release mode
    #[arg(long = "release")]
    release: bool,

    /// After build, copy only the generated Rust sources into <input>/out/tsrust_gen
    #[arg(long = "keep-gen")]
    keep_gen: bool,
}

fn collect_sources(input: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if input.is_file() {
        if input.extension().and_then(|s| s.to_str()) == Some("tsrust") {
            files.push(input.to_path_buf());
        } else {
            anyhow::bail!(
                "Input file must have .tsrust extension: {}",
                input.display()
            );
        }
    } else {
        for entry in WalkDir::new(input) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|s| s.to_str()) == Some("tsrust") {
                files.push(entry.path().to_path_buf());
            }
        }
    }

    if files.is_empty() {
        anyhow::bail!("No .tsrust files found under {}", input.display());
    }
    Ok(files)
}

fn create_build_dir(input: &Path, keep: bool) -> Result<PathBuf> {
    if keep {
        let base = if input.is_dir() {
            input
        } else {
            input.parent().unwrap_or_else(|| Path::new("."))
        };
        let dir = base.join(".tsrust_build");
        if dir.exists() {
            fs::remove_dir_all(&dir).ok();
        }
        fs::create_dir_all(&dir)?;
        Ok(dir)
    } else {
        // tempdir (manual) so we can clean on drop at the end
        let mut tries = 0;
        loop {
            let candidate = std::env::temp_dir().join(format!("tsrust_build_{}", rand_suffix()));
            if !candidate.exists() {
                fs::create_dir_all(&candidate)?;
                return Ok(candidate);
            }
            tries += 1;
            if tries > 10 {
                anyhow::bail!("Could not create a unique temporary directory");
            }
        }
    }
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", nanos)
}

fn transpile_all(
    files: &[PathBuf],
    src_root: &Path,
    gen_dir: &Path,
) -> Result<Vec<(String /*module name*/, PathBuf /*generated .rs*/)>> {
    fs::create_dir_all(gen_dir)?;
    let mut out = Vec::new();

    for path in files {
        let src =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let rs = tsrust_transpiler::transpile_str(&src, &path.display().to_string())?;

        // mirror directory structure under gen_dir
        let rel = path.strip_prefix(src_root).unwrap_or(path);
        let mut out_rs = gen_dir.join(rel);
        out_rs.set_extension("rs");
        if let Some(parent) = out_rs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out_rs, rs).with_context(|| format!("writing {}", out_rs.display()))?;

        // module name = file stem
        let module_name = rel
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mod")
            .to_string();

        out.push((module_name, out_rs));
    }

    Ok(out)
}

fn write_cargo_toml(app_dir: &Path) -> Result<()> {
    let cargo = r#"[package]
name = "tsrust_app"
version = "0.1.0"
edition = "2021"

[dependencies]
"#;
    fs::write(app_dir.join("Cargo.toml"), cargo)?;
    Ok(())
}

fn write_main_rs(app_dir: &Path, modules: &[(String, PathBuf)], entry: Option<&str>) -> Result<()> {
    let src_dir = app_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    // Build module glue: `pub mod name { include!("<abs path>"); }`
    let mut glue = String::new();
    for (name, rs_path) in modules {
        let p = rs_path.canonicalize().unwrap_or(rs_path.clone());
        let p_str = p.to_string_lossy().replace('\\', "\\\\");
        glue.push_str(&format!(
            "pub mod {name} {{ include!(\"{path}\"); }}\n",
            name = name,
            path = p_str
        ));
    }

    let main_body = if let Some(entry_expr) = entry {
        format!(
            r#"{glue}

fn main() {{
    // call user-provided entry expression
    let _ = {{ {entry} }};
}}
"#,
            glue = glue,
            entry = entry_expr
        )
    } else {
        format!(
            r#"{glue}

fn main() {{
    // no entry specified; do nothing
}}
"#,
            glue = glue
        )
    };

    fs::write(src_dir.join("main.rs"), main_body)?;
    Ok(())
}

fn build_and_copy(app_dir: &Path, output: &Path, release: bool) -> Result<()> {
    let mut args = vec!["build"];
    if release { args.push("--release"); }

    let out = duct::cmd("cargo", args)
        .dir(app_dir)
        .stderr_capture()
        .stdout_capture()
        .unchecked()        // <-- DO NOT early-exit on non-zero
        .run()
        .context("failed to run cargo build")?;

    // always print stdout
    print!("{}", String::from_utf8_lossy(&out.stdout));

    if !out.status.success() {
        // rewrite stderr to point back to .tsrust files
        let raw = String::from_utf8_lossy(&out.stderr);
        let pretty = rewrite_stderr_for_app(app_dir, &raw);
        eprint!("{pretty}");
        anyhow::bail!("cargo build failed with status {}", out.status);
    } else {
        // print stderr as-is if success (warnings etc.)
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }

    let exe_suffix = std::env::consts::EXE_SUFFIX;
    let built = if release {
        app_dir.join("target").join("release").join(format!("tsrust_app{exe_suffix}"))
    } else {
        app_dir.join("target").join("debug").join(format!("tsrust_app{exe_suffix}"))
    };

    std::fs::create_dir_all(output.parent().unwrap_or_else(|| Path::new(".")))?;
    std::fs::copy(&built, output)
        .with_context(|| format!("copying {} -> {}", built.display(), output.display()))?;
    Ok(())
}



/// Copy ONLY the generated Rust sources (not the whole target/) into keep_root/tsrust_gen
fn copy_generated_sources_only(gen_dir: &Path, keep_root: &Path) -> anyhow::Result<()> {
    let dst_root = keep_root.join("tsrust_gen");
    fs::create_dir_all(&dst_root)?;

    for entry in WalkDir::new(gen_dir) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(gen_dir)?;
        let dst = dst_root.join(rel);
        if path.is_dir() {
            fs::create_dir_all(&dst)?;
        } else if path.is_file() {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, &dst)?;
        }
    }

    // optional: .gitignore to prevent accidental commits
    let gi = keep_root.join(".gitignore");
    if !gi.exists() {
        fs::write(gi, "*\n")?;
    }

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Determine input root: if a file, use its parent; if directory, use it.
    let input_root = if args.input.is_dir() {
        args.input.clone()
    } else {
        args.input
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };

    let files = collect_sources(&args.input)?;
    let app_dir = create_build_dir(&args.input, args.debug_keep_build)?;
    let gen_dir = app_dir.join("tsrust_gen");

    // Transpile all sources found under input_root
    let modules = transpile_all(&files, &input_root, &gen_dir)?;

    write_line_map(&app_dir, &input_root, &modules)?;

    // Write mini cargo app (tsrust_app)
    write_cargo_toml(&app_dir)?;
    write_main_rs(&app_dir, &modules, args.entry.as_deref())?;

    // Build & copy artifact
    build_and_copy(&app_dir, &args.output, args.release)?;

    // If requested, copy just the generated sources to <input>/out/tsrust_gen
    if args.keep_gen {
        let keep_root = input_root.join("out");
        copy_generated_sources_only(&gen_dir, &keep_root)?;
        eprintln!("kept generated sources at {}", keep_root.display());
    }

    if !args.debug_keep_build {
        // best-effort cleanup
        let _ = fs::remove_dir_all(&app_dir);
    }

    eprintln!("Built binary → {}", args.output.display());
    Ok(())
}
