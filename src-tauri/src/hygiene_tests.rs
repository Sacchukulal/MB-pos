//! The hygiene rules, as tests.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use std::path::{Path, PathBuf};

/// The workspace root — the folder above `src-tauri`.
fn workspace() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent().expect("src-tauri has a parent").to_path_buf()
}

/// Every `.rs` file in the product, tests included, `target` excluded.
fn rust_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for start in ["crates", "src-tauri"] {
        walk(&workspace().join(start), &mut out, "rs");
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, ext: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name == "target" || name == "node_modules" || name == "dist" {
                continue;
            }
            walk(&path, out, ext);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

fn show(path: &Path) -> String {
    path.strip_prefix(workspace())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Where in the file, so a failure names a line rather than a file.
fn line_of(text: &str, index: usize) -> usize {
    text.get(..index).map_or(1, |before| before.lines().count())
}

// Every `#[allow]` says why.

/// An `allow` with no reason is a lint somebody turned off in a hurry.
#[test]
fn every_allow_says_why() {
    let mut bare = Vec::new();
    for file in rust_files() {
        // This file talks ABOUT the attribute, so it matches itself.
        if show(&file).contains("hygiene_tests") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (at, _) in text.match_indices("allow(") {
            // Both `#[allow(` and `#![allow(`, and nothing else — `allow(` in a doc comment or
            // a string is not an attribute.
            let is_attribute = text
                .get(..at)
                .is_some_and(|before| before.ends_with("#[") || before.ends_with("#!["));
            if !is_attribute {
                continue;
            }
            let Some(rest) = text.get(at..) else { continue };
            let Some(end) = rest.find(")]") else { continue };
            let inside = rest.get(..end).unwrap_or_default();
            if !inside.contains("reason") {
                bare.push(format!("{}:{}", show(&file), line_of(&text, at)));
            }
        }
    }
    assert!(
        bare.is_empty(),
        "these turn a lint off without saying why — add `reason = \"…\"`:\n  {}",
        bare.join("\n  ")
    );
}

// Nothing panics on a user path.

/// `unwrap()` is a crash a shopkeeper cannot read, and requirement 3 says billing never stops.
#[test]
fn nothing_in_the_product_unwraps() {
    let mut found = Vec::new();
    for file in rust_files() {
        let name = show(&file);
        // Test files are allowed to unwrap: there, a panic IS the assertion.
        if name.contains("tests") || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        // A file with `#[cfg(test)] mod tests` at the bottom is fine as long as the unwrap is
        // inside it, so cut the file at that line.
        let product = text
            .split_once("\n#[cfg(test)]")
            .map_or(text.as_str(), |(before, _)| before);
        for (at, _) in product.match_indices(".unwrap()") {
            found.push(format!("{name}:{}", line_of(product, at)));
        }
    }
    assert!(
        found.is_empty(),
        "these panic instead of saying something a shopkeeper can act on:\n  {}",
        found.join("\n  ")
    );
}

// No half-finished note left in the tree.

/// A TODO is a decision somebody deferred and nobody wrote down.
#[test]
fn nothing_is_left_as_a_todo() {
    let mut notes = Vec::new();
    let mut files = rust_files();
    for start in ["ui/src", "ui/tests", "ui/scripts"] {
        for ext in ["ts", "tsx", "css", "mjs"] {
            walk(&workspace().join(start), &mut files, ext);
        }
    }
    for file in files {
        if show(&file).contains("hygiene_tests") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for marker in ["TODO", "FIXME", "XXX:"] {
            if let Some(at) = text.find(marker) {
                notes.push(format!("{}:{} — {marker}", show(&file), line_of(&text, at)));
            }
        }
    }
    assert!(
        notes.is_empty(),
        "deferred work belongs in FEATURE_SCOPE §15, where the owner sees it:\n  {}",
        notes.join("\n  ")
    );
}

// No secret in the tree.

/// Nothing that looks like a key is committed.
#[test]
fn no_secret_is_committed() {
    // Written split so this test does not match itself.
    let shapes: &[(&str, &str)] = &[
        ("a JSON web token", "eyJ"),
        ("a live payment key", "sk_live"),
        ("a GitHub token", "ghp_"),
        ("a private key", "BEGIN RSA PRIVATE"),
        ("a Supabase service key", "service_role"),
    ];
    let mut hits = Vec::new();
    let mut files = rust_files();
    for start in ["ui/src", "ui/tests"] {
        for ext in ["ts", "tsx", "json"] {
            walk(&workspace().join(start), &mut files, ext);
        }
    }
    for file in files {
        let name = show(&file);
        if name.contains("hygiene_tests") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for (what, shape) in shapes {
            if let Some(at) = text.find(shape) {
                hits.push(format!("{name}:{} — {what}", line_of(&text, at)));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "these look like committed secrets:\n  {}",
        hits.join("\n  ")
    );
}

// Every file the tree holds is a file the app loads.

/// A source file nothing declares is dead code the compiler never sees.
#[test]
fn every_rust_file_is_reachable_from_a_module_tree() {
    let mut orphans = Vec::new();
    for file in rust_files() {
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // These are roots: cargo finds them by convention, not by `mod`.
        if matches!(stem.as_str(), "main" | "lib" | "mod" | "build") {
            continue;
        }
        // A file directly inside `tests/` is its own test binary, and one inside `examples/` is
        // its own example binary.
        let own_binary = file.parent().is_some_and(|p| {
            p.file_name()
                .is_some_and(|n| n == "tests" || n == "examples")
        });
        if own_binary {
            continue;
        }
        // Somebody, somewhere in the same crate, has to say `mod <stem>`.
        let declared = rust_files().iter().any(|other| {
            other != &file
                && std::fs::read_to_string(other)
                    .unwrap_or_default()
                    .contains(&format!("mod {stem};"))
        });
        if !declared {
            orphans.push(show(&file));
        }
    }
    assert!(
        orphans.is_empty(),
        "nothing declares these, so the compiler never sees them:\n  {}",
        orphans.join("\n  ")
    );
}
