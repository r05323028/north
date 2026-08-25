//! Structural architecture enforcement for North.
//!
//! Boring on purpose: these tests parse workspace manifests and frontend
//! sources and assert forbidden edges are absent. They run with the normal
//! `cargo test --workspace` gate and in CI. Rules mirror
//! docs/architecture/dependency-boundaries.md; extend BOTH when boundaries change.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("archtests crate sits at <root>/crates/north-archtests")
        .to_path_buf()
}

fn manifest(crate_name: &str) -> String {
    fs::read_to_string(
        repo_root()
            .join("crates")
            .join(crate_name)
            .join("Cargo.toml"),
    )
    .unwrap_or_else(|e| panic!("read Cargo.toml for {crate_name}: {e}"))
}

/// Extracts dependency keys from the `[dependencies]` section without pulling
/// in a TOML parser. Handles `key = "x"`, `key.workspace = true`, and skips comments.
fn declared_dependencies(manifest_text: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_dependencies = false;
    for raw in manifest_text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_dependencies = line == "[dependencies]";
            continue;
        }
        if !in_dependencies || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(key) = line.split('=').next() {
            let key = key.trim();
            if !key.is_empty() {
                deps.push(key.to_string());
            }
        }
    }
    deps
}

struct BoundaryRule {
    crate_name: &'static str,
    forbidden: &'static [&'static str],
    reason: &'static str,
}

const RULES: &[BoundaryRule] = &[
    BoundaryRule {
        crate_name: "north-domain",
        forbidden: &[
            "axum",
            "tokio",
            "sqlx",
            "reqwest",
            "north-server",
            "north-daemon",
            "north-persistence",
            "north-protocol",
        ],
        reason: "domain is pure business logic: no HTTP/DB/runtime, no other north crates",
    },
    BoundaryRule {
        crate_name: "north-protocol",
        forbidden: &[
            "north-domain",
            "north-server",
            "north-daemon",
            "north-persistence",
        ],
        reason: "wire types carry no requirement business behavior",
    },
    BoundaryRule {
        crate_name: "north-server",
        forbidden: &["north-daemon"],
        reason: "server owns business state and reaches the daemon only through north-protocol",
    },
    BoundaryRule {
        crate_name: "north-daemon",
        forbidden: &[
            "axum",
            "sqlx",
            "north-persistence",
            "north-server",
            "north-domain",
        ],
        reason: "daemon reports facts/events: no business rules, no storage, no server internals",
    },
    BoundaryRule {
        crate_name: "north-persistence",
        forbidden: &["axum", "north-server", "north-daemon"],
        reason: "infrastructure must not depend on application hosts",
    },
];

#[test]
fn crate_dependency_boundaries_hold() {
    let mut violations = Vec::new();
    for rule in RULES {
        let deps = declared_dependencies(&manifest(rule.crate_name));
        for dep in deps {
            if rule.forbidden.contains(&dep.as_str()) {
                violations.push(format!(
                    "{} depends on `{}` — forbidden: {}",
                    rule.crate_name, dep, rule.reason
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "dependency boundary violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_dumping_ground_crates() {
    const BANNED: &[&str] = &["common", "shared", "utils", "helpers", "core"];
    let crates_dir = repo_root().join("crates");
    let mut violations = Vec::new();
    let entries = fs::read_dir(&crates_dir).expect("read crates/");
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if BANNED.contains(&name) {
                violations.push(name.to_string());
            }
        }
    }
    assert!(
        violations.is_empty(),
        "dumping-ground crates are not allowed: {violations:?}"
    );
}

fn collect_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if matches!(name, "node_modules" | ".next" | "out") {
            continue;
        }
        if path.is_dir() {
            collect_sources(&path, out);
        } else if name.ends_with(".ts") || name.ends_with(".tsx") {
            out.push(path);
        }
    }
}

#[test]
fn browser_never_opens_websockets() {
    // docs/architecture/overview.md: browser↔server is HTTP + SSE only;
    // live updates use EventSource. Nothing in apps/web may open a socket.
    // Tolerate a not-yet-scaffolded apps/web during initial bootstrap only.
    let web = repo_root().join("apps/web");
    if !web.exists() {
        return;
    }
    let mut sources = Vec::new();
    collect_sources(&web, &mut sources);
    let mut violations = Vec::new();
    for path in sources {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for marker in ["new WebSocket", "ws://", "wss://"] {
            if text.contains(marker) {
                violations.push(format!("{} contains `{marker}`", path.display()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "frontend must not open WebSockets (HTTP + SSE only):\n{}",
        violations.join("\n")
    );
}
