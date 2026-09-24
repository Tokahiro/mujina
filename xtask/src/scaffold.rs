//! `cargo xtask new-launcher` and `new-device`: a skeleton that builds and passes the checks, with
//! `TODO`s for what only knowledge of the launcher or device can fill in.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{TaskResult, workspace};

const LAUNCHER: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        include_str!("../templates/launcher/Cargo.toml.in"),
    ),
    (
        "src/lib.rs",
        include_str!("../templates/launcher/lib.rs.in"),
    ),
    (
        "src/descriptor.rs",
        include_str!("../templates/launcher/descriptor.rs.in"),
    ),
    (
        "src/runtime.rs",
        include_str!("../templates/launcher/runtime.rs.in"),
    ),
    ("lang/de.po", include_str!("../templates/launcher/de.po.in")),
];

const DEVICE: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        include_str!("../templates/device/Cargo.toml.in"),
    ),
    ("src/lib.rs", include_str!("../templates/device/lib.rs.in")),
    (
        "src/descriptor.rs",
        include_str!("../templates/device/descriptor.rs.in"),
    ),
    (
        "src/runtime.rs",
        include_str!("../templates/device/runtime.rs.in"),
    ),
    ("lang/de.po", include_str!("../templates/device/de.po.in")),
];

const PROFILE: &str = include_str!("../templates/device/profile.toml.in");

/// Keys of `[launcher]` itself, read before the launchers' sections (application's `CORE`).
const RESERVED_LAUNCHER_IDS: &[&str] = &["kind", "on_exit", "menu", "overlay"];

/// Values and keys of `[device]` itself, and `adapter-keyboard`'s `OWN_ID`.
const RESERVED_DEVICE_IDS: &[&str] = &["auto", "none", "button", "profile", "custom"];

/// How an id is spelt, so that users can write it in `config.toml` and it makes a crate name.
const LAUNCHER_ID_RULE: &str = "lower case letters, digits and _, beginning with a letter, \
                                ending with a letter or digit, no two _ in a row";
const DEVICE_ID_RULE: &str = "lower case letters, digits, _ and -, beginning with a letter, \
                              ending with a letter or digit, no two of _ and - in a row";

/// What the templates add to the id's type name (`@TYPE@Launcher` and the rest).
const LAUNCHER_TYPES: &[&str] = &["Descriptor", "Launcher", "Options", "Runtime"];
const DEVICE_TYPES: &[&str] = &["Buttons", "Descriptor", "Runtime"];

/// Imported names an id could clash with: `home` would make a second `HomeLauncher`.
const LAUNCHER_IMPORTS: &[&str] = &[
    "HomeLauncher",
    "LauncherDescriptor",
    "LauncherRuntime",
    "SessionLauncher",
];
const DEVICE_IMPORTS: &[&str] = &["DeviceButtons", "DeviceDescriptor", "DeviceRuntime"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Launcher,
    /// A device crate, for buttons that are no key chords.
    Device,
    /// A profile in `profiles/devices/`, for key chords; adapter-keyboard's build script finds it.
    Profile,
}

/// A launcher or a device to make, its id checked.
#[derive(Debug)]
struct Skeleton {
    kind: Kind,
    id: String,
    /// What Mujina Settings lists it as.
    name: String,
}

/// A file the skeleton writes, relative to the workspace root, and whether it is new.
#[derive(Debug)]
struct Change {
    path: PathBuf,
    text: String,
    new: bool,
}

pub fn new_launcher(args: &[String]) -> TaskResult {
    let (id, name, _) = arguments(args, false)?;
    make(Kind::Launcher, &id, name)
}

pub fn new_device(args: &[String]) -> TaskResult {
    let (id, name, profile) = arguments(args, true)?;
    make(
        if profile { Kind::Profile } else { Kind::Device },
        &id,
        name,
    )
}

/// What `--help` adds to a skeleton task's usage; `None` for any other task.
pub fn id_help(task: &str) -> Option<String> {
    let (rule, reserved, example) = match task {
        "new-launcher" => (
            LAUNCHER_ID_RULE,
            RESERVED_LAUNCHER_IDS,
            "home makes HomeLauncher",
        ),
        "new-device" => (
            DEVICE_ID_RULE,
            RESERVED_DEVICE_IDS,
            "device makes DeviceButtons",
        ),
        _ => return None,
    };
    Some(format!(
        "\n<ID>: {rule}.\nRefused: an id that is taken, one config.toml gives a meaning of its \
         own ({}), and, for a crate, one that names a type the same as one the crate imports \
         ({example}).\n\
         --name <NAME>: what Mujina Settings lists it as; without it, the id's words with \
         capitals.\n",
        reserved.join(", ")
    ))
}

/// `<id> [--name <text>] [--profile]`, where `profile` says whether `--profile` is allowed.
fn arguments(args: &[String], profile: bool) -> Result<(String, Option<String>, bool), String> {
    let mut id = None;
    let mut name = None;
    let mut as_profile = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--name" => {
                name = Some(args.next().ok_or("--name needs a name")?.clone());
            }
            "--profile" if profile => as_profile = true,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ if id.is_some() => return Err(format!("one id only, not also {arg}")),
            _ => id = Some(arg.clone()),
        }
    }
    let example = if as_profile {
        "new-device my-handheld --profile"
    } else if profile {
        "new-device rog-ally"
    } else {
        "new-launcher playnite"
    };
    let id = id.ok_or_else(|| format!("which id? e.g. `cargo xtask {example}`"))?;
    Ok((id, name, as_profile))
}

fn make(kind: Kind, id: &str, name: Option<String>) -> TaskResult {
    let root = workspace::root();
    let skeleton = Skeleton::new(kind, id, name)?;
    let changes = plan(&root, &skeleton)?;
    write(&root, &changes)?;
    for change in &changes {
        let verb = if change.new { "made" } else { "listed it in" };
        println!("{verb} {}", shown(&change.path));
    }
    if kind == Kind::Profile {
        println!(
            "next: fill in its TODOs, then `cargo test -p mujina-adapter-keyboard`; in the pull \
             request, paste what `mujinactl doctor` and `mujinactl probe` printed \
             (docs/new-device.md)"
        );
        return Ok(());
    }
    rustfmt(&root, &changes);
    lock(&root)?;
    let dir = format!("crates/{}", skeleton.dir());
    let (guide, checklist) = if kind == Kind::Launcher {
        ("docs/new-launcher.md", "a section for it")
    } else {
        (
            "docs/new-device.md",
            "a row under \"Devices with a crate of their own\"",
        )
    };
    println!(
        "updated Cargo.lock for the new crate: commit it with the crate, since every build runs \
         with --locked"
    );
    println!(
        "next: fill in the TODOs in {dir} ({guide}), add {checklist} to \
         docs/on-device-checklist.md, then `cargo test -p {}` and `cargo xtask i18n-check`",
        skeleton.package()
    );
    Ok(())
}

impl Skeleton {
    fn new(kind: Kind, id: &str, name: Option<String>) -> Result<Self, String> {
        check_id(kind, id)?;
        let name = match name {
            Some(name) => {
                let unusable = name.trim().is_empty()
                    || name.trim() != name
                    || name
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '"' | '\\' | '{' | '}'));
                if unusable {
                    // Braces too: the name is written into a `format!` string.
                    return Err(format!(
                        "name {name:?}: some text, without quotes, backslashes, braces or line \
                         breaks"
                    ));
                }
                name
            }
            None => words(id).join(" "),
        };
        Ok(Self {
            kind,
            id: id.to_string(),
            name,
        })
    }

    fn dir(&self) -> String {
        format!("adapter-{}", self.id.replace('_', "-"))
    }

    fn package(&self) -> String {
        format!("mujina-{}", self.dir())
    }

    fn ident(&self) -> String {
        self.package().replace('-', "_")
    }

    /// Its Rust type names begin with this: `rog-ally` is `RogAlly`.
    fn type_name(&self) -> String {
        words(&self.id).concat()
    }

    fn fill(&self, template: &str) -> String {
        template
            .replace("@ID@", &self.id)
            .replace("@NAME@", &self.name)
            .replace("@TYPE@", &self.type_name())
            .replace("@CRATE@", &self.package())
    }
}

/// The words of `id`, each with a capital: `rog-ally` is `Rog`, `Ally`.
fn words(id: &str) -> Vec<String> {
    id.split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}

/// Checks the spelling as the conformance tests do, then refuses reserved ids and imported names.
fn check_id(kind: Kind, id: &str) -> Result<(), String> {
    let (what, rule, reserved, types, imports) = match kind {
        Kind::Launcher => (
            "launcher",
            LAUNCHER_ID_RULE,
            RESERVED_LAUNCHER_IDS,
            LAUNCHER_TYPES,
            LAUNCHER_IMPORTS,
        ),
        Kind::Device => (
            "device",
            DEVICE_ID_RULE,
            RESERVED_DEVICE_IDS,
            DEVICE_TYPES,
            DEVICE_IMPORTS,
        ),
        // A profile is no Rust: nothing is named after it.
        Kind::Profile => (
            "device",
            DEVICE_ID_RULE,
            RESERVED_DEVICE_IDS,
            &[][..],
            &[][..],
        ),
    };
    let separator = |c: char| c == '_' || (c == '-' && what == "device");
    let spelt = id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || separator(c))
        && id.ends_with(|c: char| c.is_ascii_alphanumeric())
        && !id
            .as_bytes()
            .windows(2)
            .any(|pair| separator(char::from(pair[0])) && separator(char::from(pair[1])));
    if !spelt {
        return Err(format!("{what} id {id:?}: {rule}"));
    }
    if reserved.contains(&id) {
        return Err(format!(
            "{what} id {id:?} has a meaning of its own in config.toml"
        ));
    }
    let type_name = words(id).concat();
    if let Some(name) = types
        .iter()
        .map(|suffix| format!("{type_name}{suffix}"))
        .find(|name| imports.contains(&name.as_str()))
    {
        return Err(format!(
            "{what} id {id:?} would name a type {name}, which clashes with the Mujina type of \
             that name the crate uses"
        ));
    }
    Ok(())
}

/// Everything the skeleton writes, or why it cannot be made here. Reads, never writes.
fn plan(root: &Path, skeleton: &Skeleton) -> Result<Vec<Change>, String> {
    let profile = PathBuf::from(format!("profiles/devices/{}.toml", skeleton.id));
    let dir = PathBuf::from("crates").join(skeleton.dir());
    if skeleton.kind != Kind::Launcher {
        refuse_taken_device_id(root, &skeleton.id)?;
    }
    if skeleton.kind == Kind::Profile {
        if root.join(&dir).exists() {
            return Err(format!(
                "{} exists: a device crate may have the id {:?} already",
                shown(&dir),
                skeleton.id
            ));
        }
        return Ok(vec![Change {
            path: profile,
            text: skeleton.fill(PROFILE),
            new: true,
        }]);
    }
    if root.join(&dir).exists() {
        return Err(format!("{} exists already", shown(&dir)));
    }

    let files = if skeleton.kind == Kind::Launcher {
        LAUNCHER
    } else {
        DEVICE
    };
    let mut changes: Vec<Change> = files
        .iter()
        .map(|(path, template)| Change {
            path: dir.join(path),
            text: skeleton.fill(template),
            new: true,
        })
        .collect();

    let manifest = PathBuf::from("Cargo.toml");
    let line = format!(
        "{} = {{ path = \"crates/{}\" }}",
        skeleton.package(),
        skeleton.dir()
    );
    let text = read(root, &manifest)?;
    changes.push(Change {
        text: workspace_dependency(&text, &skeleton.package(), &line)
            .map_err(|why| refused(&manifest, why, &line))?,
        path: manifest,
        new: false,
    });

    let app = PathBuf::from("crates/app/Cargo.toml");
    let line = format!("{}.workspace = true", skeleton.package());
    let block = if skeleton.kind == Kind::Launcher {
        "The launchers"
    } else {
        "The devices"
    };
    let text = read(root, &app)?;
    changes.push(Change {
        text: app_dependency(&text, block, &skeleton.package(), &line)
            .map_err(|why| refused(&app, why, &line))?,
        path: app,
        new: false,
    });

    let registry = PathBuf::from("crates/app/src/registry.rs");
    let list = if skeleton.kind == Kind::Launcher {
        "LAUNCHERS"
    } else {
        "DEVICE_PLUGINS"
    };
    let entry = format!("&{}::PLUGIN", skeleton.ident());
    let text = read(root, &registry)?;
    changes.push(Change {
        text: registry_entry(&text, list, &entry)
            .map_err(|why| refused(&registry, why, &format!("{entry}, in {list}")))?,
        path: registry,
        new: false,
    });
    Ok(changes)
}

fn refuse_taken_device_id(root: &Path, id: &str) -> TaskResult {
    let folder = root.join("profiles/devices");
    let entries =
        std::fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let taken = path.file_stem().is_some_and(|stem| stem == id)
            || std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok())
                .is_some_and(|profile| profile.get("id").and_then(toml::Value::as_str) == Some(id));
        if taken {
            return Err(format!(
                "the device profile {} has the id {id:?} already",
                shown(path.strip_prefix(root).unwrap_or(&path))
            ));
        }
    }
    Ok(())
}

/// The root `Cargo.toml` with `line` after the last adapter of `[workspace.dependencies]`.
fn workspace_dependency(text: &str, package: &str, line: &str) -> Result<String, Unlisted> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim() == "[workspace.dependencies]")
        .ok_or("it has no [workspace.dependencies]")?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.starts_with('['))
        .map_or(lines.len(), |at| start + 1 + at);
    let section = &lines[start + 1..end];
    if section.iter().any(|line| is_dependency(line, package)) {
        return Err(Unlisted::Already(format!(
            "{package} is listed there already"
        )));
    }
    let last = section
        .iter()
        .rposition(|line| line.starts_with("mujina-adapter-"))
        .ok_or("[workspace.dependencies] names no mujina-adapter- crate to add it after")?;
    Ok(inserted(text, &lines, start + 1 + last + 1, line))
}

/// `crates/app/Cargo.toml` with `line` at the end of the block whose comment begins with `block`.
fn app_dependency(text: &str, block: &str, package: &str, line: &str) -> Result<String, Unlisted> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.iter().any(|line| is_dependency(line, package)) {
        return Err(Unlisted::Already(format!(
            "{package} is listed there already"
        )));
    }
    let comment = lines
        .iter()
        .position(|line| {
            line.trim_start()
                .strip_prefix('#')
                .is_some_and(|comment| comment.trim_start().starts_with(block))
        })
        .ok_or_else(|| format!("no comment beginning with {block:?} to add it under"))?;
    let mut after = comment + 1;
    while lines.get(after).is_some_and(|line| line.starts_with('#')) {
        after += 1;
    }
    while lines
        .get(after)
        .is_some_and(|line| line.starts_with("mujina-adapter-"))
    {
        after += 1;
    }
    Ok(inserted(text, &lines, after, line))
}

fn is_dependency(line: &str, package: &str) -> bool {
    line.strip_prefix(package)
        .is_some_and(|rest| rest.starts_with([' ', '=', '.']))
}

fn inserted(text: &str, lines: &[&str], at: usize, line: &str) -> String {
    let mut all: Vec<&str> = lines.to_vec();
    all.insert(at, line);
    let mut out = all.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// `registry.rs` with `entry` last in the list `pub static <list>: … = &[…];`, one entry to a
/// line; rustfmt then puts a short list on one line, as it wants it.
fn registry_entry(text: &str, list: &str, entry: &str) -> Result<String, Unlisted> {
    let head = format!("pub static {list}:");
    let start = text
        .find(&head)
        .ok_or_else(|| format!("no `{head}` to add it to"))?;
    let open = text[start..]
        .find("= &[")
        .map(|at| start + at + "= &[".len())
        .ok_or_else(|| format!("{list} is no `&[…]`"))?;
    let close = text[open..]
        .find("];")
        .map(|at| open + at)
        .ok_or_else(|| format!("{list} does not end in `];`"))?;
    if text[open..close].contains("//") {
        return Err(format!("{list} has comments between its entries").into());
    }
    let mut entries: Vec<&str> = text[open..close]
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();
    if entries.contains(&entry) {
        return Err(Unlisted::Already(format!("{list} has {entry} already")));
    }
    entries.push(entry);
    let mut out = text[..open].to_string();
    for entry in entries {
        out.push_str("\n    ");
        out.push_str(entry);
        out.push(',');
    }
    out.push('\n');
    out.push_str(&text[close..]);
    Ok(out)
}

/// Why an edit of one of the three lists wrote nothing.
#[derive(Debug)]
enum Unlisted {
    /// The file lists the crate already, though its folder is gone (removed by hand, say).
    Already(String),
    NoPlace(String),
}

impl From<String> for Unlisted {
    fn from(problem: String) -> Self {
        Self::NoPlace(problem)
    }
}

impl From<&str> for Unlisted {
    fn from(problem: &str) -> Self {
        Self::NoPlace(problem.to_string())
    }
}

fn refused(path: &Path, why: Unlisted, line: &str) -> String {
    match why {
        Unlisted::Already(listed) => format!(
            "{}: {listed}. Nothing was written; remove that entry if no crate of that name is \
             wanted any more, or choose another id",
            shown(path)
        ),
        Unlisted::NoPlace(problem) => format!(
            "{}: {problem}. Nothing was written; add `{line}` there by hand, or fix the file and \
             run the task again",
            shown(path)
        ),
    }
}

fn read(root: &Path, path: &Path) -> Result<String, String> {
    std::fs::read_to_string(root.join(path)).map_err(|error| format!("{}: {error}", shown(path)))
}

/// A new file is never written over one that exists.
fn write(root: &Path, changes: &[Change]) -> TaskResult {
    for change in changes {
        let path = root.join(&change.path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        }
        let written = if change.new {
            std::fs::File::create_new(&path)
                .and_then(|mut file| std::io::Write::write_all(&mut file, change.text.as_bytes()))
        } else {
            std::fs::write(&path, &change.text)
        };
        written.map_err(|error| format!("{}: {error}", shown(&change.path)))?;
    }
    Ok(())
}

/// Formats the Rust files `changes` wrote; if rustfmt fails, only says so.
fn rustfmt(root: &Path, changes: &[Change]) {
    let files: Vec<PathBuf> = changes
        .iter()
        .filter(|change| change.path.extension().is_some_and(|ext| ext == "rs"))
        .map(|change| root.join(&change.path))
        .collect();
    let formatted = Command::new("rustfmt")
        .args(["--edition", "2024"])
        .args(&files)
        .current_dir(root)
        .status();
    if !formatted.is_ok_and(|status| status.success()) {
        println!("rustfmt did not run through: run `cargo fmt --all`");
    }
}

/// `--workspace` keeps every other entry ("Other packages are updated only if they don't already
/// exist in the lockfile", cargo-update); offline first, since a skeleton needs no new crate.
fn lock(root: &Path) -> TaskResult {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    for offline in [true, false] {
        let mut update = Command::new(&cargo);
        update
            .args(["update", "--workspace", "--quiet"])
            .current_dir(root);
        if offline {
            update.arg("--offline");
        }
        if update.status().is_ok_and(|status| status.success()) {
            return Ok(());
        }
    }
    Err("`cargo update --workspace` failed: run it yourself, then commit Cargo.lock".to_string())
}

/// `path` with forward slashes, as the messages show it.
fn shown(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary copy of the manifests, registry and profiles, with an empty folder per crate.
    struct Workspace(PathBuf);

    impl Workspace {
        fn new(test: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "mujina-xtask-scaffold-{}-{test}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            let real = workspace::root();
            for file in [
                "Cargo.toml",
                "crates/app/Cargo.toml",
                "crates/app/src/registry.rs",
            ] {
                let to = root.join(file);
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::copy(real.join(file), to).unwrap();
            }
            for folder in ["crates", "profiles/devices"] {
                for entry in std::fs::read_dir(real.join(folder)).unwrap() {
                    let entry = entry.unwrap();
                    let to = root.join(folder).join(entry.file_name());
                    if entry.path().is_dir() {
                        std::fs::create_dir_all(to).unwrap();
                    } else {
                        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                        std::fs::copy(entry.path(), to).unwrap();
                    }
                }
            }
            Self(root)
        }

        fn make(&self, kind: Kind, id: &str) -> Result<Vec<Change>, String> {
            let skeleton = Skeleton::new(kind, id, None)?;
            let changes = plan(&self.0, &skeleton)?;
            write(&self.0, &changes)?;
            Ok(changes)
        }

        fn read(&self, path: &str) -> String {
            std::fs::read_to_string(self.0.join(path)).unwrap()
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn check_crate(copy: &Workspace, dir: &str, package: &str) {
        let manifest: toml::Table = copy.read(&format!("{dir}/Cargo.toml")).parse().unwrap();
        assert_eq!(manifest["package"]["name"].as_str(), Some(package));
        assert_eq!(
            manifest["package"]["metadata"]["mujina"]["ring"].as_str(),
            Some("adapter")
        );
        let mut sources = Vec::new();
        for file in ["src/lib.rs", "src/descriptor.rs", "src/runtime.rs"] {
            let source = copy.read(&format!("{dir}/{file}"));
            if let Err(error) = syn::parse_file(&source) {
                panic!("{dir}/{file}: {error}");
            }
            assert!(!source.contains('@'), "{dir}/{file}: a placeholder is left");
            sources.push(source);
        }
        let po = copy.read(&format!("{dir}/lang/de.po"));
        let problems = crate::i18n::rust_catalog_problems(&sources, &po).unwrap();
        assert!(problems.is_empty(), "{dir}/lang/de.po: {problems:?}");
    }

    fn dependency<'a>(manifest: &'a toml::Table, section: &str, package: &str) -> &'a toml::Value {
        let dependencies = match section {
            "workspace" => &manifest["workspace"]["dependencies"],
            _ => &manifest["dependencies"],
        };
        dependencies
            .get(package)
            .unwrap_or_else(|| panic!("{package} is not a dependency"))
    }

    fn registry_list(copy: &Workspace, list: &str) -> Vec<String> {
        let source = copy.read("crates/app/src/registry.rs");
        let file = syn::parse_file(&source).unwrap();
        for item in file.items {
            if let syn::Item::Static(item) = item
                && item.ident == list
            {
                let syn::Expr::Reference(reference) = *item.expr else {
                    panic!("{list} is no reference");
                };
                let syn::Expr::Array(array) = *reference.expr else {
                    panic!("{list} is no array");
                };
                return array
                    .elems
                    .iter()
                    .map(|entry| {
                        quote_path(entry).unwrap_or_else(|| panic!("{list}: an odd entry"))
                    })
                    .collect();
            }
        }
        panic!("registry.rs has no {list}");
    }

    /// `&a::b::C` as "a::b::C".
    fn quote_path(entry: &syn::Expr) -> Option<String> {
        let syn::Expr::Reference(reference) = entry else {
            return None;
        };
        let syn::Expr::Path(path) = &*reference.expr else {
            return None;
        };
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        Some(segments.join("::"))
    }

    #[test]
    fn a_launcher_is_made_and_listed_in_the_three_places() {
        let copy = Workspace::new("launcher");
        let before = registry_list(&copy, "LAUNCHERS");
        let changes = copy.make(Kind::Launcher, "made_up").unwrap();
        assert_eq!(changes.iter().filter(|change| change.new).count(), 5);
        check_crate(&copy, "crates/adapter-made-up", "mujina-adapter-made-up");
        let descriptor = copy.read("crates/adapter-made-up/src/descriptor.rs");
        assert!(descriptor.contains("\"made_up\""), "its id");
        assert!(descriptor.contains("Msg::new(\"Made Up\")"), "its name");
        assert!(descriptor.contains("[launcher.made_up]"), "its template");

        let root: toml::Table = copy.read("Cargo.toml").parse().unwrap();
        assert_eq!(
            dependency(&root, "workspace", "mujina-adapter-made-up")["path"].as_str(),
            Some("crates/adapter-made-up")
        );
        let app: toml::Table = copy.read("crates/app/Cargo.toml").parse().unwrap();
        assert_eq!(
            dependency(&app, "app", "mujina-adapter-made-up")["workspace"].as_bool(),
            Some(true)
        );
        let app_text = copy.read("crates/app/Cargo.toml");
        let at = |text: &str| app_text.find(text).unwrap();
        assert!(at("mujina-adapter-generic.workspace") < at("mujina-adapter-made-up.workspace"));
        assert!(at("mujina-adapter-made-up.workspace") < at("# The devices"));

        let mut expected = before;
        expected.push("mujina_adapter_made_up::PLUGIN".to_string());
        assert_eq!(registry_list(&copy, "LAUNCHERS"), expected);

        let again = copy.make(Kind::Launcher, "made_up").unwrap_err();
        assert!(
            again.contains("crates/adapter-made-up exists already"),
            "{again}"
        );
    }

    #[test]
    fn a_device_crate_is_made_and_listed_among_the_devices() {
        let copy = Workspace::new("device");
        let before = registry_list(&copy, "DEVICE_PLUGINS");
        copy.make(Kind::Device, "made-up-pad").unwrap();
        check_crate(
            &copy,
            "crates/adapter-made-up-pad",
            "mujina-adapter-made-up-pad",
        );
        let lib = copy.read("crates/adapter-made-up-pad/src/lib.rs");
        assert!(lib.contains("MadeUpPadDescriptor"), "{lib}");
        let app_text = copy.read("crates/app/Cargo.toml");
        let at = |text: &str| app_text.find(text).unwrap();
        assert!(
            at("mujina-adapter-keyboard.workspace") < at("mujina-adapter-made-up-pad.workspace")
        );
        let mut expected = before;
        expected.push("mujina_adapter_made_up_pad::PLUGIN".to_string());
        assert_eq!(registry_list(&copy, "DEVICE_PLUGINS"), expected);

        // A second one joins the first (the list is empty until the first device crate).
        copy.make(Kind::Device, "made-up-pad-two").unwrap();
        expected.push("mujina_adapter_made_up_pad_two::PLUGIN".to_string());
        assert_eq!(registry_list(&copy, "DEVICE_PLUGINS"), expected);
        let root: toml::Table = copy.read("Cargo.toml").parse().unwrap();
        dependency(&root, "workspace", "mujina-adapter-made-up-pad-two");
    }

    #[test]
    fn a_profile_is_one_file_as_the_keyboard_crate_reads_it() {
        let copy = Workspace::new("profile");
        let changes = copy.make(Kind::Profile, "made-up-mini").unwrap();
        assert_eq!(changes.len(), 1, "no manifest or registry changes");
        let profile: toml::Table = copy
            .read("profiles/devices/made-up-mini.toml")
            .parse()
            .unwrap();
        assert_eq!(profile["id"].as_str(), Some("made-up-mini"));
        assert_eq!(profile["name"].as_str(), Some("Made Up Mini"));
        assert!(profile["match"]["manufacturer"].is_str());
        assert!(profile["match"]["product"].is_str());
        let buttons = profile["buttons"].as_array().unwrap();
        assert_eq!(buttons.len(), 1);
        for key in ["key", "label", "chord"] {
            assert!(buttons[0][key].is_str(), "{key}");
        }
        let known = ["id", "name", "match", "buttons"];
        assert!(profile.keys().all(|key| known.contains(&key.as_str())));

        let again = copy.make(Kind::Profile, "made-up-mini").unwrap_err();
        assert!(again.contains("has the id"), "{again}");
    }

    #[test]
    fn taken_and_reserved_ids_are_refused() {
        let copy = Workspace::new("refused");
        for (kind, id, says) in [
            (
                Kind::Launcher,
                "steam",
                "crates/adapter-steam exists already",
            ),
            (Kind::Launcher, "kind", "a meaning of its own"),
            (
                Kind::Launcher,
                "big-box",
                "lower case letters, digits and _,",
            ),
            (Kind::Device, "onexplayer", "has the id \"onexplayer\""),
            (Kind::Profile, "onexplayer", "has the id \"onexplayer\""),
            (Kind::Device, "custom", "a meaning of its own"),
            (Kind::Profile, "auto", "a meaning of its own"),
            (
                Kind::Device,
                "keyboard",
                "crates/adapter-keyboard exists already",
            ),
            (Kind::Profile, "keyboard", "a device crate may have"),
            (Kind::Launcher, "Playnite", "beginning with a letter"),
            (Kind::Launcher, "2box", "beginning with a letter"),
            (Kind::Device, "ally-", "ending with a letter or digit"),
            (Kind::Device, "rog--ally", "no two of _ and - in a row"),
            (Kind::Device, "rog-_ally", "no two of _ and - in a row"),
            (Kind::Launcher, "big__box", "no two _ in a row"),
            (Kind::Launcher, "", "beginning with a letter"),
            (Kind::Launcher, "home", "HomeLauncher, which clashes"),
            (Kind::Launcher, "session", "SessionLauncher, which clashes"),
            (
                Kind::Launcher,
                "launcher",
                "which clashes with the Mujina type",
            ),
            (Kind::Device, "device", "which clashes with the Mujina type"),
        ] {
            let refused = copy.make(kind, id).unwrap_err();
            assert!(refused.contains(says), "{kind:?} {id:?}: {refused}");
        }
        assert_eq!(
            copy.read("Cargo.toml"),
            std::fs::read_to_string(workspace::root().join("Cargo.toml")).unwrap()
        );
        assert!(Skeleton::new(Kind::Profile, "device", None).is_ok());
    }

    fn imported(source: &str) -> Vec<String> {
        fn leaves(tree: &syn::UseTree, out: &mut Vec<String>) {
            match tree {
                syn::UseTree::Path(path) => leaves(&path.tree, out),
                syn::UseTree::Name(name) => out.push(name.ident.to_string()),
                syn::UseTree::Rename(rename) => out.push(rename.rename.to_string()),
                syn::UseTree::Glob(_) => {}
                syn::UseTree::Group(group) => {
                    for tree in &group.items {
                        leaves(tree, out);
                    }
                }
            }
        }
        fn uses(items: &[syn::Item], out: &mut Vec<String>) {
            for item in items {
                match item {
                    syn::Item::Use(item) => leaves(&item.tree, out),
                    syn::Item::Mod(module) => {
                        if let Some((_, inner)) = &module.content {
                            uses(inner, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        uses(&syn::parse_file(source).unwrap().items, &mut out);
        out
    }

    #[test]
    fn the_names_an_id_could_clash_with_are_those_the_templates_import() {
        for (kind, files, types, imports) in [
            (Kind::Launcher, LAUNCHER, LAUNCHER_TYPES, LAUNCHER_IMPORTS),
            (Kind::Device, DEVICE, DEVICE_TYPES, DEVICE_IMPORTS),
        ] {
            let skeleton = Skeleton::new(kind, "made_up", None).unwrap();
            let mut made = Vec::new();
            let mut all_imported = Vec::new();
            for (path, template) in files {
                for (at, _) in template.match_indices("@TYPE@") {
                    let rest = &template[at + "@TYPE@".len()..];
                    made.push(
                        rest.chars()
                            .take_while(char::is_ascii_alphanumeric)
                            .collect::<String>(),
                    );
                }
                if path.starts_with("src/") {
                    all_imported.extend(imported(&skeleton.fill(template)));
                }
            }
            made.sort();
            made.dedup();
            assert_eq!(made, types, "{kind:?}: the type names the templates make");
            // Mujina's names, not the crate's own that one of its files imports from another.
            let own = skeleton.type_name();
            let mut could_clash: Vec<&str> = all_imported
                .iter()
                .map(String::as_str)
                .filter(|name| !name.starts_with(&own))
                .filter(|name| {
                    types
                        .iter()
                        .any(|suffix| name.len() > suffix.len() && name.ends_with(suffix))
                })
                .collect();
            could_clash.sort_unstable();
            could_clash.dedup();
            assert_eq!(
                could_clash, imports,
                "{kind:?}: the names the templates import"
            );
        }
    }

    #[test]
    fn the_help_says_how_an_id_is_spelt() {
        let help = id_help("new-device").unwrap();
        assert!(help.contains(DEVICE_ID_RULE), "{help}");
        assert!(
            help.contains("auto, none, button, profile, custom"),
            "{help}"
        );
        assert!(id_help("new-launcher").unwrap().contains("HomeLauncher"));
        assert_eq!(id_help("arch-check"), None);
    }

    #[test]
    fn a_name_may_be_given_but_not_one_that_breaks_a_file() {
        let named = Skeleton::new(Kind::Device, "rog-ally", Some("ROG Ally".to_string())).unwrap();
        assert_eq!(named.name, "ROG Ally");
        assert_eq!(named.type_name(), "RogAlly");
        assert_eq!(named.ident(), "mujina_adapter_rog_ally");
        for name in [
            "",
            " Ally",
            "The \"Ally\"",
            r"C:\Ally",
            "two\nlines",
            "Ally {2}",
        ] {
            assert!(
                Skeleton::new(Kind::Device, "rog-ally", Some(name.to_string())).is_err(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn an_edit_without_its_place_says_what_to_add_by_hand() {
        let registry = Path::new("crates/app/src/registry.rs");
        let why = app_dependency(
            "[dependencies]\nlog.workspace = true\n",
            "The devices",
            "x",
            "x.workspace = true",
        )
        .unwrap_err();
        assert!(
            matches!(&why, Unlisted::NoPlace(problem) if problem.contains("The devices")),
            "{why:?}"
        );
        let why =
            registry_entry("pub fn devices() {}\n", "DEVICE_PLUGINS", "&x::PLUGIN").unwrap_err();
        let told = refused(registry, why, "&x::PLUGIN");
        assert!(told.contains("pub static DEVICE_PLUGINS:"), "{told}");
        assert!(told.contains("add `&x::PLUGIN` there by hand"), "{told}");
    }

    #[test]
    fn an_entry_left_over_is_to_be_removed_not_added() {
        let why = workspace_dependency(
            "[workspace.dependencies]\nmujina-adapter-x = { path = \"crates/adapter-x\" }\n",
            "mujina-adapter-x",
            "mujina-adapter-x = { path = \"crates/adapter-x\" }",
        )
        .unwrap_err();
        let told = refused(Path::new("Cargo.toml"), why, "mujina-adapter-x = …");
        assert!(told.contains("is listed there already"), "{told}");
        assert!(told.contains("remove that entry"), "{told}");
        assert!(!told.contains("by hand"), "{told}");
        let why = registry_entry(
            "pub static DEVICE_PLUGINS: &[&DevicePlugin] = &[&x::PLUGIN];\n",
            "DEVICE_PLUGINS",
            "&x::PLUGIN",
        )
        .unwrap_err();
        assert!(matches!(why, Unlisted::Already(_)), "{why:?}");
    }

    #[test]
    fn the_arguments_are_an_id_a_name_and_for_a_device_whether_it_is_a_profile() {
        let args = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            arguments(&args(&["ally", "--profile", "--name", "ROG Ally"]), true).unwrap(),
            ("ally".to_string(), Some("ROG Ally".to_string()), true)
        );
        assert!(arguments(&args(&["ally", "--profile"]), false).is_err());
        assert!(arguments(&args(&["a", "b"]), true).is_err());
        assert!(arguments(&args(&[]), true).is_err());
        assert!(arguments(&args(&["a", "--name"]), true).is_err());
        let which = |list: &[&str], profile| arguments(&args(list), profile).unwrap_err();
        assert!(which(&[], false).contains("`cargo xtask new-launcher "));
        assert!(which(&[], true).contains("`cargo xtask new-device rog-ally`"));
        assert!(which(&["--profile"], true).contains("new-device my-handheld --profile`"));
    }
}
