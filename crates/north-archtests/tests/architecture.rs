//! Structural architecture enforcement for North.
//!
//! Dependency rules resolve against the EFFECTIVE Cargo dependency graph via
//! `cargo metadata --no-deps`: normal, dev, build, and target-specific
//! dependencies all count. Rules mirror docs/architecture/dependency-boundaries.md;
//! extend BOTH when boundaries change.

use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("archtests crate sits at <root>/crates/north-archtests")
        .to_path_buf()
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

/// Workspace metadata without the resolve graph: package declarations are all
/// we need, including every declared dependency kind.
fn workspace_metadata() -> Value {
    let output = Command::new(cargo_bin())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repo_root())
        .output()
        .expect("spawn cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid cargo metadata JSON")
}

/// Pure helper: every dependency name declared by one package object across
/// ALL dependency kinds (normal `kind: null`, `dev`, `build`, target-scoped).
/// Renamed dependencies still count under their real crate name (`name`).
fn package_dependency_names(package: &Value) -> Vec<String> {
    let empty = Vec::new();
    let deps = package
        .get("dependencies")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    deps.iter()
        .filter_map(|dep| dep.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// Map of workspace member name -> declared dependency names.
fn member_dependencies(metadata: &Value) -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    if let Some(packages) = metadata.get("packages").and_then(Value::as_array) {
        for package in packages {
            if let Some(name) = package.get("name").and_then(Value::as_str) {
                map.insert(name.to_string(), package_dependency_names(package));
            }
        }
    }
    map
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
            "serde_json",
            "north-server",
            "north-daemon",
            "north-persistence",
            "north-protocol",
        ],
        reason: "domain is pure business logic: no HTTP/DB/runtime/JSON, no other north crates",
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
    let metadata = workspace_metadata();
    let members = member_dependencies(&metadata);
    let mut violations = Vec::new();
    for rule in RULES {
        let Some(deps) = members.get(rule.crate_name) else {
            violations.push(format!(
                "workspace member `{}` is missing from cargo metadata",
                rule.crate_name
            ));
            continue;
        };
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
fn dependency_parser_covers_all_dependency_kinds() {
    // Meta-test: the parser must count normal, dev, build, and target-specific
    // dependencies, and renamed deps under their real crate name.
    let package: Value = serde_json::from_str(
        r#"{
            "name": "example",
            "dependencies": [
                { "name": "normal_dep", "kind": null, "optional": false },
                { "name": "dev_dep", "kind": "dev", "optional": false },
                { "name": "build_dep", "kind": "build", "optional": false },
                { "name": "targeted_dep", "kind": null, "optional": false,
                  "target": { "cfg": ["windows"] } },
                { "name": "renamed_real_name", "rename": "alias", "kind": null, "optional": false }
            ]
        }"#,
    )
    .unwrap();
    let names = package_dependency_names(&package);
    for expected in [
        "normal_dep",
        "dev_dep",
        "build_dep",
        "targeted_dep",
        "renamed_real_name",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "parser missed dependency `{expected}`: {names:?}"
        );
    }
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
