use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::io::IsTerminal;
use walkdir::WalkDir; // for is_terminal()

#[derive(Debug, Deserialize)]
struct FnLineMap {
    #[allow(dead_code)]
    name: String,
    src_start: usize,
    gen_start: usize,
}
#[derive(Debug, Deserialize)]
struct FileLineMap {
    gen_file: String,
    src_file: String,
    fns: Vec<FnLineMap>,
}
#[derive(Debug, Deserialize)]
struct CrateLineMap {
    files: Vec<FileLineMap>,
}

fn candidate_target_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    let mut cur = std::env::current_dir().unwrap();
    for _ in 0..6 {
        let t = cur.join("target");
        if t.is_dir() {
            dirs.push(t);
        }
        if !cur.pop() {
            break;
        }
    }
    dirs
}

fn load_all_maps_anywhere() -> Result<Vec<CrateLineMap>> {
    let mut maps = Vec::new();
    for root in candidate_target_dirs() {
        for entry in WalkDir::new(&root) {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.file_name().to_string_lossy() == "tsrust_line_map.json" {
                if let Ok(data) = fs::read(entry.path()) {
                    if let Ok(map) = serde_json::from_slice::<CrateLineMap>(&data) {
                        maps.push(map);
                    }
                }
            }
        }
    }
    Ok(maps)
}

// helper: find first occurrence of tok at or after min_col0 (visual columns, on expanded text)
fn find_token_col_at_or_after(expanded_line: &str, tok: &str, min_col0: usize) -> Option<usize> {
    let mut i = 0; // byte index into expanded_line
    let mut col0 = 0; // visual column (0-based)
    while let Some(rel) = expanded_line[i..].find(tok) {
        let before = &expanded_line[i..i + rel];
        col0 += before.chars().count();
        if col0 >= min_col0 {
            return Some(col0);
        }
        // advance past this match and keep searching
        i += rel + tok.len();
        col0 += tok.len();
    }
    None
}

fn strip_ansi(s: &str) -> String {
    // remove ANSI escape codes so the regex can match cleanly
    let re = Regex::new(r"\x1B\[[0-9;]*[mK]").unwrap();
    re.replace_all(s, "").into_owned()
}

fn read_line_1_based(path: &str, line: usize) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(f);
    let mut s = String::new();
    for _ in 1..line {
        s.clear();
        if reader.read_line(&mut s).ok()? == 0 {
            return None;
        }
    }
    s.clear();
    if reader.read_line(&mut s).ok()? == 0 {
        return None;
    }
    while matches!(s.chars().last(), Some('\n' | '\r')) {
        s.pop();
    }
    Some(s)
}

fn read_next_non_empty_line(
    path: &str,
    start_line_1: usize,
    limit_ahead: usize,
) -> Option<(usize, String)> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(f);
    let mut buf = String::new();
    for _ in 1..start_line_1 {
        buf.clear();
        if reader.read_line(&mut buf).ok()? == 0 {
            return None;
        }
    }
    for i in 0..=limit_ahead {
        buf.clear();
        if reader.read_line(&mut buf).ok()? == 0 {
            return None;
        }
        let trimmed = buf.trim_end_matches(&['\n', '\r'][..]);
        if !trimmed.trim().is_empty() {
            return Some((start_line_1 + i, trimmed.to_string()));
        }
    }
    None
}

fn gen_suffix(p: &str) -> Option<String> {
    // compare only after "tsrust_gen/" so OUT_DIR hashes don’t matter
    let norm = p.replace('\\', "/");
    norm.rsplit_once("tsrust_gen/").map(|(_, s)| s.to_string())
}

fn find_culprit_ahead(
    path: &str,
    start_line_1: usize,
    needle: &str,
    limit_ahead: usize,
) -> Option<(
    usize,  /*line*/
    usize,  /*0-based col*/
    String, /*line text*/
)> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut r = BufReader::new(f);
    let mut s = String::new();

    // fast-forward to (start_line_1 - 1)
    for _ in 1..start_line_1 {
        s.clear();
        if r.read_line(&mut s).ok()? == 0 {
            return None;
        }
    }

    for i in 0..=limit_ahead {
        s.clear();
        if r.read_line(&mut s).ok()? == 0 {
            return None;
        }
        let line_no = start_line_1 + i;
        let trimmed = s.trim_end_matches(&['\n', '\r'][..]).to_string();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(byte_pos) = trimmed.find(needle) {
            let col = trimmed[..byte_pos].chars().count();
            return Some((line_no, col, trimmed));
        }
    }
    None
}

fn expand_tabs_to_cols(s: &str, tabw: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = tabw - (col % tabw);
            for _ in 0..spaces {
                out.push(' ');
                col += 1;
            }
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

fn map_line(maps: &[CrateLineMap], gen_path: &str, gen_line: usize) -> Option<(String, usize)> {
    let want_suf = gen_suffix(gen_path);
    for m in maps {
        for f in &m.files {
            let matches =
                if let (Some(a), Some(b)) = (want_suf.as_ref(), gen_suffix(&f.gen_file).as_ref()) {
                    a == b
                } else {
                    f.gen_file == gen_path
                };
            if !matches {
                continue;
            }

            if f.fns.is_empty() {
                return Some((f.src_file.clone(), gen_line));
            }
            // pick the nearest function whose gen_start <= gen_line
            let mut best: Option<&FnLineMap> = None;
            for fun in &f.fns {
                if fun.gen_start <= gen_line {
                    best = match best {
                        None => Some(fun),
                        Some(prev) if fun.gen_start > prev.gen_start => Some(fun),
                        other => other,
                    };
                }
            }
            if let Some(fun) = best {
                let delta = gen_line.saturating_sub(fun.gen_start);
                let src_line = fun.src_start + delta;
                return Some((f.src_file.clone(), src_line));
            } else {
                return Some((f.src_file.clone(), gen_line));
            }
        }
    }
    None
}

fn rewrite_stderr(stderr: &str) -> Result<String> {
    let maps = load_all_maps_anywhere()?;
    let clean = strip_ansi(stderr);

    // Color only if stderr is a terminal and NO_COLOR isn't set.
    let (red, reset) = if std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
    {
        ("\x1b[1;31m", "\x1b[0m") // bright red
    } else {
        ("", "")
    };

    // Matches lines like:
    //   --> /abs/.../tsrust_gen/foo.rs:12:9
    //   --> C:\abs\...\tsrust_gen\foo.rs:12:9
    let arrow = Regex::new(r"(?m)^ *--> ([^:\n]+):(\d+):(\d+)").unwrap();

    let mut out = String::with_capacity(clean.len());
    let mut last_idx = 0usize;

    let mut iter = arrow.captures_iter(&clean).peekable();

    // let mut prev_arrow_end = 0usize;
    while let Some(m) = iter.next() {
        // Extract arrow captures
        let whole = m.get(0).unwrap();
        let gen_path = m.get(1).unwrap().as_str();
        let line: usize = m.get(2).unwrap().as_str().parse().unwrap_or(1);
        let col: usize = m.get(3).unwrap().as_str().parse().unwrap_or(1);

        // region of text BEFORE this arrow (usually contains "error[E...] ... `tok` ...")
        let header_region = &clean[last_idx..whole.start()];
        // region of text AFTER this arrow, up to the next arrow (notes, snippet, etc.)
        let next_start = iter
            .peek()
            .and_then(|n| n.get(0))
            .map(|mm| mm.start())
            .unwrap_or(clean.len());
        let diag_block = &clean[whole.start()..next_start];

        let culprit_re = Regex::new(r"`([^\s`]+)`").unwrap();
        let culprit = culprit_re
            .captures(header_region) // prefer header text
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            .or_else(|| {
                culprit_re // fallback: after the arrow
                    .captures(diag_block)
                    .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            });

        // push everything before the arrow as-is
        out.push_str(&clean[last_idx..whole.start()]);

        // --- compute mapped path/line ---
        let (mapped_path, mapped_line) = if let Some((p, l)) = map_line(&maps, gen_path, line) {
            (p, l)
        } else {
            (gen_path.to_string(), line)
        };

        // --- retarget BEFORE printing snippet (aim at the actual token if we detected it) ---
        let (disp_line, disp_col, disp_text) = if let Some(tok) = culprit.as_deref() {
            if let Some((ln, token_col0, text)) =
                find_culprit_ahead(&mapped_path, mapped_line, tok, 4)
            {
                (ln, token_col0 + 1, Some(text)) // rustc columns are 1-based
            } else {
                (mapped_line, col, None)
            }
        } else {
            (mapped_line, col, None)
        };

        // --- write the rewritten arrow using mapped location ---
        // out.push_str(&format!("--> {}:{}:{}", mapped_path, disp_line, disp_col));
        out.push_str(&format!(
            "{red}--> {mapped_path}:{disp_line}:{disp_col}{reset}"
        ));

        // Move cursor to after the original arrow in the input stream
        let mut cursor = whole.end();

        // Try to rewrite the next snippet block:
        //   \n  |\n  <num> | <code>\n
        let after = &clean[cursor..];
        let pipe_line = Regex::new(r"(?s)^\n( *\|)\n").unwrap();
        let code_line = Regex::new(r"(?m)^(\s*)(\d+)(\s*\|\s)(.*)").unwrap();

        if let Some(pl_caps) = pipe_line.captures(after) {
            // re-emit the blank pipe line verbatim
            let whole = pl_caps.get(0).unwrap();
            out.push_str(whole.as_str());
            cursor += whole.end();

            let after2 = &clean[cursor..];
            if let Some(cl_caps) = code_line.captures(after2) {
                let cl = cl_caps.get(0).unwrap();
                // any prelude up to the code line
                out.push_str(&after2[..cl.start()]);

                // rebuild margin with the **mapped** line number
                let lead_ws = cl_caps.get(1).unwrap().as_str();
                let post_bar = cl_caps.get(3).unwrap().as_str();
                out.push_str(lead_ws);
                out.push_str(&disp_line.to_string());
                out.push_str(post_bar);

                // choose the source text to show
                let mut shown: Option<(usize, String)> = disp_text
                    .clone()
                    .map(|t| (disp_line, t))
                    .or_else(|| {
                        read_line_1_based(&mapped_path, disp_line)
                            .filter(|s| !s.trim().is_empty())
                            .map(|s| (disp_line, s))
                    })
                    .or_else(|| read_next_non_empty_line(&mapped_path, disp_line, 4));

                // print the source line (expand tabs to keep caret math stable)
                let printed_src = if let Some((_ln, src_text)) = shown.take() {
                    let expanded = expand_tabs_to_cols(&src_text, 4);
                    out.push_str(&expanded);
                    expanded
                } else {
                    // fallback to rustc’s text if we couldn't load the file
                    let fallback = cl_caps.get(4).unwrap().as_str().to_string();
                    out.push_str(&fallback);
                    fallback
                };

                // advance cursor past the original code line; the caret line follows next
                cursor += cl.end();

                // caret line: place carets under the token (or at disp_col if token missing)
                let after3 = &clean[cursor..];
                let caret_line = Regex::new(r"(?m)^(\s*\|\s)(\s*)(\^+)(.*)$").unwrap();
                if let Some(caps) = caret_line.captures(after3) {
                    let whole = caps.get(0).unwrap();
                    // write anything before the caret line
                    out.push_str(&after3[..whole.start()]);
                    let caret_prefix = caps.get(1).unwrap().as_str();
                    let caret_marks = caps.get(3).unwrap().as_str();
                    let caret_suffix = caps.get(4).map(|m| m.as_str()).unwrap_or("");
                    // compute visual indent
                    let min_col0 = disp_col.saturating_sub(1);
                    let target_indent = if let Some(tok) = culprit.as_deref() {
                        find_token_col_at_or_after(&printed_src, tok, min_col0).unwrap_or(min_col0)
                    } else {
                        min_col0
                    };
                    let indent_spaces: String =
                        std::iter::repeat(' ').take(target_indent).collect();
                    out.push_str(caret_prefix);
                    out.push_str(&indent_spaces);
                    out.push_str(red); // start red
                    out.push_str(caret_marks); // "^^^"
                    out.push_str(caret_suffix); // trailing message, keep as-is but red too
                    out.push_str(reset); // end red

                    cursor += whole.end();
                }
            }
        }

        // prev_arrow_end = next_start;
        last_idx = cursor;
    }
    out.push_str(&clean[last_idx..]);
    Ok(out)
}

fn main() -> Result<()> {
    // Always capture output, even if cargo exits with 101.
    let result = duct::cmd!("cargo", "check")
        .stderr_capture()
        .stdout_capture()
        .unchecked()
        .run()
        .context("failed to spawn cargo check")?;

    let stdout_s = String::from_utf8_lossy(&result.stdout);
    let stderr_s = String::from_utf8_lossy(&result.stderr);

    print!("{}", stdout_s);
    eprint!("{}", rewrite_stderr(&stderr_s)?);

    // Mirror cargo's exit code
    std::process::exit(result.status.code().unwrap_or(1));
}
