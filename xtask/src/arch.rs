//! Enforces the onion's dependency rule: dependencies point inward only.
//!
//! Each member names its ring in `[package.metadata.mujina] ring`; none or an unknown one fails.
//! [`MATRIX`] says which rings each ring may use; a test keeps docs/architecture.md's table equal.

use serde_json::Value;

use crate::{TaskResult, workspace};

use Ring::{
    Adapter, AdapterSupport, Application, Domain, Installer, Leaf, Plumbing, Root, SettingsApp,
    Tool,
};

/// A ring of the onion, as a crate names it in `[package.metadata.mujina] ring = "…"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ring {
    Domain,
    Application,
    Plumbing,
    Leaf,
    AdapterSupport,
    Adapter,
    Root,
    SettingsApp,
    Installer,
    Tool,
}

impl Ring {
    /// The name in the manifest and in docs/architecture.md.
    fn name(self) -> &'static str {
        match self {
            Domain => "domain",
            Application => "application",
            Plumbing => "plumbing",
            Leaf => "leaf",
            AdapterSupport => "adapter-support",
            Adapter => "adapter",
            Root => "root",
            SettingsApp => "settings-app",
            Installer => "installer",
            Tool => "tool",
        }
    }
}

/// Crates from outside the workspace a ring may use; deny.toml says which are allowed at all.
#[derive(Clone, Copy, Debug)]
enum Outside {
    Nothing,
    Only(&'static [&'static str]),
    Any,
}

impl Outside {
    fn allows(self, dependency: &str) -> bool {
        match self {
            Outside::Nothing => false,
            Outside::Only(crates) => crates.contains(&dependency),
            Outside::Any => true,
        }
    }
}

/// One row of the ring matrix.
struct Rule {
    ring: Ring,
    /// The rings whose crates a crate of this ring may depend on.
    rings: &'static [Ring],
    outside: Outside,
}

/// The ring matrix, the one source of the rule. A crate may not depend on a crate of its own
/// ring unless its row says so.
const MATRIX: &[Rule] = &[
    Rule {
        ring: Domain,
        rings: &[],
        outside: Outside::Nothing,
    },
    // thiserror only derives `Error` impls; the leaf's `Msg` gives the doctor's titles.
    Rule {
        ring: Application,
        rings: &[Domain, Leaf],
        outside: Outside::Only(&["thiserror"]),
    },
    // Plumbing must not know Mujina.
    Rule {
        ring: Plumbing,
        rings: &[],
        outside: Outside::Any,
    },
    // Any ring could build on a leaf; a row lists it once a crate of that ring does.
    Rule {
        ring: Leaf,
        rings: &[],
        outside: Outside::Nothing,
    },
    // What several adapters share, in Mujina's types.
    Rule {
        ring: AdapterSupport,
        rings: &[Domain, Application, Plumbing],
        outside: Outside::Any,
    },
    // Adapters implement ports; never sideways into another adapter or outward into the root.
    Rule {
        ring: Adapter,
        rings: &[Domain, Application, Plumbing, AdapterSupport],
        outside: Outside::Any,
    },
    // Sees every inner ring. Nothing builds on the settings app, the installer or the tooling.
    Rule {
        ring: Root,
        rings: &[Domain, Application, Plumbing, AdapterSupport, Adapter],
        outside: Outside::Any,
    },
    // Reuses the composition root's tool module instead of wiring adapters again (ADR-0011).
    Rule {
        ring: SettingsApp,
        rings: &[Domain, Application, Plumbing, Leaf, Root],
        outside: Outside::Any,
    },
    // Runs the home app rule in-process with adapter-windows' registry adapter (a row cannot name
    // one crate). Knows no launcher or device; uses the leaf through the application ring.
    Rule {
        ring: Installer,
        rings: &[Domain, Application, Plumbing, Leaf, Adapter],
        outside: Outside::Any,
    },
    Rule {
        ring: Tool,
        rings: &[],
        outside: Outside::Any,
    },
];

pub fn check() -> TaskResult {
    let metadata = workspace::metadata()?;
    // With --no-deps these are exactly the workspace members.
    let members = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;

    let violations = violations(members);
    if violations.is_empty() {
        println!("architecture ok: {} crates checked", members.len());
        Ok(())
    } else {
        Err(violations.join("\n       "))
    }
}

/// Every way `members`, the workspace's packages as `cargo metadata` lists them, break the rule.
fn violations(members: &[Value]) -> Vec<String> {
    let mut violations = Vec::new();
    for member in members {
        let rule = match rule(member) {
            Ok(rule) => rule,
            Err(violation) => {
                violations.push(violation);
                continue;
            }
        };
        for dependency in member["dependencies"].as_array().into_iter().flatten() {
            // Dev- and build-dependencies do not end up in the shipped dependency graph.
            if !dependency["kind"].is_null() {
                continue;
            }
            // A dependency listed twice (for all targets and for Windows) is one violation.
            if let Some(violation) = violation(member, rule, dependency, members)
                && !violations.contains(&violation)
            {
                violations.push(violation);
            }
        }
    }
    violations
}

/// The row of the ring `package` names in its Cargo.toml.
fn rule(package: &Value) -> Result<&'static Rule, String> {
    let name = name(package);
    let rings = || {
        MATRIX
            .iter()
            .map(|rule| rule.ring.name())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match &package["metadata"]["mujina"]["ring"] {
        Value::Null => Err(format!(
            "{name} names no ring: add [package.metadata.mujina] ring = \"…\" to its Cargo.toml \
             (one of {}; see docs/architecture.md)",
            rings()
        )),
        Value::String(ring) => MATRIX
            .iter()
            .find(|rule| rule.ring.name() == ring)
            .ok_or_else(|| {
                format!(
                    "{name} names the unknown ring \"{ring}\" (one of {})",
                    rings()
                )
            }),
        ring => Err(format!(
            "{name} names the unknown ring {ring} (one of {})",
            rings()
        )),
    }
}

/// What is wrong with `member`, of `rule`'s ring, depending on `dependency`, if anything.
fn violation(member: &Value, rule: &Rule, dependency: &Value, members: &[Value]) -> Option<String> {
    let (name, ring) = (name(member), rule.ring.name());
    // Workspace crates are path dependencies (no `source`, a `path`), whatever their name says.
    let by_path = dependency["source"].is_null() || dependency.get("path").is_some();
    let dependency = self::name(dependency);
    if !by_path {
        return (!rule.outside.allows(dependency)).then(|| {
            format!("{name} ({ring}) must not depend on {dependency}, a crate from outside")
        });
    }
    let Some(target) = members
        .iter()
        .find(|member| self::name(member) == dependency)
    else {
        return Some(format!(
            "{name} ({ring}) must not depend on {dependency}, a path dependency that is no \
             workspace member and so has no ring"
        ));
    };
    // A member without a known ring is reported on its own.
    let target = self::rule(target).ok()?;
    (!rule.rings.contains(&target.ring)).then(|| {
        format!(
            "{name} ({ring}) must not depend on {dependency} ({})",
            target.ring.name()
        )
    })
}

fn name(package: &Value) -> &str {
    package["name"].as_str().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{MATRIX, Outside, Ring, violations};

    fn member(name: &str, ring: Option<&str>, dependencies: &[Value]) -> Value {
        json!({
            "name": name,
            "metadata": ring.map(|ring| json!({ "mujina": { "ring": ring } })),
            "dependencies": dependencies,
        })
    }

    /// A path dependency, as cargo metadata lists one.
    fn path(name: &str) -> Value {
        json!({ "name": name, "source": null, "kind": null, "path": format!("/w/crates/{name}") })
    }

    /// A crates.io dependency, as cargo metadata lists one.
    fn registry(name: &str) -> Value {
        json!({
            "name": name,
            "source": "registry+https://github.com/rust-lang/crates.io-index",
            "kind": null,
        })
    }

    fn allowed(from: &str, to: &str) -> bool {
        violations(&[
            member("a", Some(from), &[path("b")]),
            member("b", Some(to), &[]),
        ])
        .is_empty()
    }

    fn allowed_outside(ring: &str, dependency: &str) -> bool {
        violations(&[member("a", Some(ring), &[registry(dependency)])]).is_empty()
    }

    #[test]
    fn the_matrix_has_one_row_per_ring() {
        let rings = [
            Ring::Domain,
            Ring::Application,
            Ring::Plumbing,
            Ring::Leaf,
            Ring::AdapterSupport,
            Ring::Adapter,
            Ring::Root,
            Ring::SettingsApp,
            Ring::Installer,
            Ring::Tool,
        ];
        for ring in rings {
            let rows = MATRIX.iter().filter(|rule| rule.ring == ring).count();
            assert_eq!(rows, 1, "{ring:?}");
        }
        assert_eq!(MATRIX.len(), rings.len());
    }

    #[test]
    fn inner_rings_are_closed() {
        assert!(!allowed_outside("domain", "serde"));
        assert!(!allowed("domain", "application"));
        assert!(!allowed("domain", "domain"));
        assert!(allowed("application", "domain"));
        assert!(allowed_outside("application", "thiserror"));
        assert!(!allowed_outside("application", "windows-sys"));
        assert!(!allowed("application", "plumbing"));
        assert!(!allowed("application", "adapter"));
    }

    #[test]
    fn a_leaf_uses_nothing() {
        assert!(!allowed("leaf", "domain"));
        assert!(!allowed("leaf", "plumbing"));
        assert!(!allowed("leaf", "leaf"));
        assert!(!allowed_outside("leaf", "serde"));
        assert!(allowed("settings-app", "leaf"));
        assert!(allowed("application", "leaf"));
        assert!(allowed("installer", "leaf"));
        // Not yet: no crate of this ring uses one.
        assert!(!allowed("domain", "leaf"));
    }

    #[test]
    fn plumbing_does_not_know_mujina() {
        assert!(allowed_outside("plumbing", "windows-sys"));
        assert!(!allowed("plumbing", "domain"));
        assert!(!allowed("plumbing", "application"));
        assert!(!allowed("plumbing", "plumbing"));
    }

    #[test]
    fn adapters_do_not_reach_sideways_or_outward() {
        assert!(allowed("adapter", "domain"));
        assert!(allowed("adapter", "application"));
        assert!(allowed("adapter", "plumbing"));
        assert!(allowed_outside("adapter", "serde_json"));
        assert!(allowed_outside("adapter", "windows-sys"));
        assert!(!allowed("adapter", "adapter"));
        assert!(!allowed("adapter", "root"));
        assert!(!allowed("adapter", "settings-app"));
    }

    #[test]
    fn the_adapter_kit_sits_between_the_adapters_and_the_inner_rings() {
        let kit = |dependencies: &[Value]| {
            member("mujina-adapter-kit", Some("adapter-support"), dependencies)
        };
        let inner = [
            member("mujina-domain", Some("domain"), &[]),
            member("mujina-application", Some("application"), &[]),
            member("mujina-winutil", Some("plumbing"), &[]),
        ];
        let used = kit(&[
            path("mujina-domain"),
            path("mujina-application"),
            path("mujina-winutil"),
        ]);
        let steam = member(
            "mujina-adapter-steam",
            Some("adapter"),
            &[path("mujina-adapter-kit")],
        );
        let mut workspace = inner.to_vec();
        workspace.extend([used, steam]);
        assert_eq!(violations(&workspace), Vec::<String>::new());

        let reaching = kit(&[path("mujina-adapter-steam")]);
        let steam = member("mujina-adapter-steam", Some("adapter"), &[]);
        assert_eq!(
            violations(&[reaching, steam]),
            [
                "mujina-adapter-kit (adapter-support) must not depend on mujina-adapter-steam \
                 (adapter)"
            ]
        );
        assert!(!allowed("adapter-support", "root"));
        assert!(!allowed("adapter-support", "adapter-support"));
    }

    #[test]
    fn entry_points_are_leaves() {
        for ring in [
            "domain",
            "application",
            "plumbing",
            "adapter-support",
            "adapter",
        ] {
            assert!(allowed("root", ring), "root on {ring}");
        }
        // There is one composition root; a second one building on it would be a new row.
        assert!(!allowed("root", "root"));
        for ring in ["domain", "application", "plumbing", "leaf", "root"] {
            assert!(allowed("settings-app", ring), "settings-app on {ring}");
        }
        // Through the composition root's tool module, never an adapter of its own.
        assert!(!allowed("settings-app", "adapter"));
        assert!(!allowed("settings-app", "adapter-support"));
        assert!(allowed_outside("settings-app", "slint"));
        // The home app rule runs in-process, with the registry adapter: no mujinactl.exe to find.
        for ring in ["domain", "application", "plumbing", "leaf", "adapter"] {
            assert!(allowed("installer", ring), "installer on {ring}");
        }
        assert!(!allowed("installer", "adapter-support"));
        assert!(allowed_outside("installer", "slint"));
        assert!(allowed_outside("tool", "serde_json"));
        for leaf in ["settings-app", "installer", "tool"] {
            for ring in [
                "domain",
                "application",
                "plumbing",
                "adapter-support",
                "adapter",
                "root",
            ] {
                assert!(!allowed(ring, leaf), "{ring} on {leaf}");
            }
        }
        assert!(!allowed("settings-app", "installer"));
        assert!(!allowed("installer", "settings-app"));
        assert!(!allowed("installer", "root"));
        assert!(!allowed("tool", "domain"));
        assert!(!allowed("tool", "root"));
    }

    #[test]
    fn a_member_without_a_ring_fails() {
        let no_metadata = member("mujina-new", None, &[]);
        let other_metadata = json!({ "name": "mujina-new", "metadata": { "docs": { "rs": {} } } });
        let no_ring = json!({ "name": "mujina-new", "metadata": { "mujina": {} } });
        for package in [no_metadata, other_metadata, no_ring] {
            let violations = violations(&[package]);
            assert_eq!(violations.len(), 1);
            assert!(
                violations[0]
                    .starts_with("mujina-new names no ring: add [package.metadata.mujina]"),
                "{violations:?}"
            );
        }
    }

    #[test]
    fn an_unknown_ring_fails() {
        assert_eq!(
            violations(&[member("mujina-new", Some("adapters"), &[])]),
            [
                "mujina-new names the unknown ring \"adapters\" (one of domain, application, \
                 plumbing, leaf, adapter-support, adapter, root, settings-app, installer, tool)"
            ]
        );
        let number = json!({ "name": "mujina-new", "metadata": { "mujina": { "ring": 3 } } });
        let violations = violations(&[number]);
        assert_eq!(violations.len(), 1);
        assert!(
            violations[0].starts_with("mujina-new names the unknown ring 3"),
            "{violations:?}"
        );
    }

    #[test]
    fn a_dependency_on_a_member_without_a_ring_is_reported_once() {
        let workspace = [
            member(
                "mujina-adapter-steam",
                Some("adapter"),
                &[path("mujina-new")],
            ),
            member("mujina-new", None, &[]),
        ];
        let violations = violations(&workspace);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("mujina-new names no ring"));
    }

    #[test]
    fn a_workspace_crate_counts_by_its_ring_not_its_name() {
        // No mujina- prefix, still a crate of the workspace.
        let workspace = [
            member("mujina-adapter-steam", Some("adapter"), &[path("playnite")]),
            member("playnite", Some("adapter"), &[]),
        ];
        assert_eq!(
            violations(&workspace),
            ["mujina-adapter-steam (adapter) must not depend on playnite (adapter)"]
        );
        // A renamed dependency is listed under the package's name.
        let mut renamed = path("mujina-adapter-generic");
        renamed["rename"] = json!("generic");
        let workspace = [
            member("mujina-adapter-steam", Some("adapter"), &[renamed]),
            member("mujina-adapter-generic", Some("adapter"), &[]),
        ];
        assert_eq!(
            violations(&workspace),
            ["mujina-adapter-steam (adapter) must not depend on mujina-adapter-generic (adapter)"]
        );
    }

    #[test]
    fn a_crate_from_outside_counts_as_outside_whatever_its_name() {
        assert!(allowed_outside("adapter", "mujina-lookalike"));
        assert_eq!(
            violations(&[member(
                "mujina-domain",
                Some("domain"),
                &[registry("mujina-app")]
            )]),
            ["mujina-domain (domain) must not depend on mujina-app, a crate from outside"]
        );
        // Not a workspace member although one has that name: it comes from a registry.
        let workspace = [
            member(
                "mujina-adapter-steam",
                Some("adapter"),
                &[registry("mujina-app")],
            ),
            member("mujina-app", Some("root"), &[]),
        ];
        assert_eq!(violations(&workspace), Vec::<String>::new());
    }

    #[test]
    fn a_path_dependency_that_is_no_member_fails() {
        let workspace = [member("mujina-app", Some("root"), &[path("vendored")])];
        assert_eq!(
            violations(&workspace),
            [
                "mujina-app (root) must not depend on vendored, a path dependency that is no \
                 workspace member and so has no ring"
            ]
        );
    }

    #[test]
    fn only_shipped_dependencies_are_checked() {
        let mut dev = path("mujina-app");
        dev["kind"] = json!("dev");
        let mut build = registry("cc");
        build["kind"] = json!("build");
        let workspace = [
            member("mujina-domain", Some("domain"), &[dev, build]),
            member("mujina-app", Some("root"), &[]),
        ];
        assert_eq!(violations(&workspace), Vec::<String>::new());

        // A dependency for one target only still ships there, and is named once.
        let mut windows = registry("windows-sys");
        windows["target"] = json!("cfg(windows)");
        let workspace = [member(
            "mujina-application",
            Some("application"),
            &[windows.clone(), windows],
        )];
        assert_eq!(
            violations(&workspace),
            [
                "mujina-application (application) must not depend on windows-sys, a crate from \
                 outside"
            ]
        );
    }

    /// The rings table between the arch-check markers in docs/architecture.md: for every row,
    /// the ring, the rings it may depend on and the crates from outside it may use, as written.
    fn documented() -> Vec<[String; 3]> {
        let doc = include_str!("../../docs/architecture.md");
        let begin = doc
            .find("<!-- arch-check: begin -->")
            .expect("docs/architecture.md has no arch-check begin marker");
        let end = doc
            .find("<!-- arch-check: end -->")
            .expect("docs/architecture.md has no arch-check end marker");
        doc[begin..end]
            .lines()
            // The rows, not the header, the separator or the markers.
            .filter(|line| line.starts_with("| `"))
            .map(|line| {
                let cells: Vec<&str> = line.split('|').map(str::trim).collect();
                [cells[1], cells[3], cells[4]].map(str::to_string)
            })
            .collect()
    }

    /// The same, written from the matrix.
    fn expected() -> Vec<[String; 3]> {
        let list = |names: Vec<&str>| {
            names
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        MATRIX
            .iter()
            .map(|rule| {
                let rings = if rule.rings.is_empty() {
                    "nothing".to_string()
                } else {
                    list(rule.rings.iter().map(|ring| ring.name()).collect())
                };
                let outside = match rule.outside {
                    Outside::Nothing => "none".to_string(),
                    Outside::Only(crates) => format!("only {}", list(crates.to_vec())),
                    Outside::Any => "any".to_string(),
                };
                [format!("`{}`", rule.ring.name()), rings, outside]
            })
            .collect()
    }

    #[test]
    fn the_rings_table_in_the_docs_is_the_matrix() {
        assert_eq!(
            documented(),
            expected(),
            "the rings table in docs/architecture.md differs from arch::MATRIX"
        );
    }
}
