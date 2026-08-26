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
        .expect("architecture tests crate sits at <root>/tests/architecture")
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

const ARCHITECTURE_TEST_CRATE: &str = "north-architecture-tests";

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
            "axum",
            "tokio",
            "tokio-tungstenite",
            "tungstenite",
            "north-domain",
            "north-server",
            "north-daemon",
            "north-persistence",
        ],
        reason: "JSON wire types stay independent from WebSocket, runtime, and business hosts",
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
        reason: "daemon reports facts/events: no business database or server internals; local transport journal is allowed",
    },
    BoundaryRule {
        crate_name: "north-persistence",
        forbidden: &["axum", "north-server", "north-daemon"],
        reason: "infrastructure must not depend on application hosts",
    },
];

struct DependencyAllowlist {
    crate_name: &'static str,
    allowed: &'static [&'static str],
    reason: &'static str,
}

const PURE_CRATE_ALLOWLISTS: &[DependencyAllowlist] = &[
    DependencyAllowlist {
        crate_name: "north-domain",
        allowed: &[],
        reason: "domain is pure business logic",
    },
    DependencyAllowlist {
        crate_name: "north-protocol",
        allowed: &["serde", "serde_json"],
        reason: "protocol is pure JSON wire data",
    },
];

fn disallowed_dependencies(dependencies: &[String], allowed: &[&str]) -> Vec<String> {
    dependencies
        .iter()
        .filter(|dependency| !allowed.contains(&dependency.as_str()))
        .cloned()
        .collect()
}

#[test]
fn pure_crate_dependency_allowlists_hold() {
    let members = member_dependencies(&workspace_metadata());
    let mut violations = Vec::new();
    for rule in PURE_CRATE_ALLOWLISTS {
        let Some(dependencies) = members.get(rule.crate_name) else {
            violations.push(format!("missing pure crate `{}`", rule.crate_name));
            continue;
        };
        for dependency in disallowed_dependencies(dependencies, rule.allowed) {
            violations.push(format!(
                "{} depends on `{dependency}` — forbidden: {}",
                rule.crate_name, rule.reason
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "pure crate dependency allowlist violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn dependency_allowlist_helper_rejects_unapproved_dependencies() {
    let dependencies = vec!["serde".to_string(), "reqwest".to_string()];
    assert_eq!(
        disallowed_dependencies(&dependencies, &["serde"]),
        vec!["reqwest".to_string()]
    );
}

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

/// The transport adapters are the only host-side owners of WebSocket dependencies;
/// both hosts cross the application boundary through north-protocol.
#[test]
fn transport_dependency_direction_is_explicit() {
    let members = member_dependencies(&workspace_metadata());
    for (crate_name, dependency) in [
        ("north-server", "north-protocol"),
        ("north-daemon", "north-protocol"),
    ] {
        assert!(
            members
                .get(crate_name)
                .is_some_and(|dependencies| dependencies.iter().any(|dep| dep == dependency)),
            "{crate_name} must depend on {dependency} for North application frames"
        );
    }
}

#[test]
fn production_crates_do_not_depend_on_architecture_tests() {
    let members = member_dependencies(&workspace_metadata());
    let mut violations = Vec::new();
    for (name, deps) in members {
        if name != ARCHITECTURE_TEST_CRATE && deps.iter().any(|dep| dep == ARCHITECTURE_TEST_CRATE)
        {
            violations.push(name);
        }
    }
    assert!(
        violations.is_empty(),
        "production crates must not depend on `{ARCHITECTURE_TEST_CRATE}`: {violations:?}"
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

fn is_repository_validation_crate(name: &str) -> bool {
    [
        "-archtests",
        "-architecture-tests",
        "-integration-tests",
        "-e2e-tests",
        "-smoke-tests",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

#[test]
fn validation_crates_stay_outside_production_crates_tree() {
    let crates_dir = repo_root().join("crates");
    let mut violations = Vec::new();
    let entries = fs::read_dir(&crates_dir).expect("read crates/");
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if is_repository_validation_crate(name) {
                violations.push(name.to_string());
            }
        }
    }
    assert!(
        violations.is_empty(),
        "repository validation crates must live outside crates/: {violations:?}"
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

fn collect_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn daemon_does_not_own_business_retry_policy() {
    // Local reconnect/backoff and runtime reattachment belong in the daemon;
    // server-owned execution state and business retry budgets do not.
    let daemon_src = repo_root().join("crates/north-daemon/src");
    if !daemon_src.exists() {
        return;
    }
    let mut sources = Vec::new();
    collect_rust_sources(&daemon_src, &mut sources);
    const FORBIDDEN_MARKERS: &[&str] = &[
        "struct RetryPolicy",
        "enum RetryPolicy",
        "struct ExecutionState",
        "enum ExecutionState",
        "const MAX_ATTEMPTS",
        "const MAX_RETRY_ATTEMPTS",
        "retry_budget",
        "ExecutionState::Retrying",
    ];
    let mut violations = Vec::new();
    for path in sources {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for marker in FORBIDDEN_MARKERS {
            if text.contains(marker) {
                violations.push(format!("{} contains `{marker}`", path.display()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "daemon must not own server execution retry policy; keep only local transport recovery:\n{}",
        violations.join("\n")
    );
}

fn collect_sql_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sql_sources(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("sql") {
            out.push(path);
        }
    }
}

#[test]
fn repository_schema_never_stores_credentials() {
    // Skip until repository migrations exist. Once they do, inspect only table
    // definitions whose name contains repository; daemon credential tables are
    // intentionally outside this rule.
    let migrations = repo_root().join("migrations");
    if !migrations.exists() {
        return;
    }
    let mut sources = Vec::new();
    collect_sql_sources(&migrations, &mut sources);
    const FORBIDDEN_FIELDS: &[&str] = &[
        "token",
        "access_token",
        "secret",
        "secret_hash",
        "password",
        "credential",
        "private_key",
        "ssh_key",
    ];
    let mut in_repository_table = false;
    let mut violations = Vec::new();
    for path in sources {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            let lower = line.to_ascii_lowercase();
            if lower.contains("create table") && lower.contains("repository") {
                in_repository_table = true;
            }
            if in_repository_table {
                let field = line
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c| matches!(c, '"' | '`' | ','));
                if FORBIDDEN_FIELDS.contains(&field) {
                    violations.push(format!(
                        "{} contains repository credential field `{field}`",
                        path.display()
                    ));
                }
                if lower.contains(");") {
                    in_repository_table = false;
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "repository configuration must never store Git credentials:\n{}",
        violations.join("\n")
    );
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
