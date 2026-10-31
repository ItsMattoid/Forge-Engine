//! Class and component discovery.
//!
//! Scans `src/classes/**` and `src/components/**` and generates, for each tree,
//! the nested module declarations (mirroring the folder layout) plus the lists the
//! registry reads. A source file takes part in the engine by exporting one or more
//! of these items:
//!
//! | item                                   | registers                     |
//! |----------------------------------------|-------------------------------|
//! | `pub static CLASS: ClassDesc`          | a class                       |
//! | `pub static COMPONENT: ComponentDesc`  | a component                   |
//! | `pub static ENUMS: &[EnumDesc]`        | script-visible enums          |
//!
//! Files that export none of them (shared helpers) are simply compiled as modules.
//! A class file may export its own `COMPONENT` for behaviour only that class needs
//! (a light's Bevy renderer, a camera's view); components shared by several
//! classes live in `src/components/`.
//!
//! Generated into `OUT_DIR`:
//!
//! * `classes_generated.rs`:    `ALL_CLASSES`, `OWNED_COMPONENTS`, `ENUM_LISTS`
//! * `components_generated.rs`: `ALL_COMPONENTS`

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

const CLASS_MARKER: &str = "pub static CLASS:";
const COMPONENT_MARKER: &str = "pub static COMPONENT:";
const ENUMS_MARKER: &str = "pub static ENUMS:";

/// The module tree of one source folder.
#[derive(Default)]
struct Tree {
    /// `(module name, absolute file path)`
    files: Vec<(String, String)>,
    dirs: BTreeMap<String, Tree>,
}

/// Paths (`group::file::ITEM`) of everything the files in a tree export.
#[derive(Default)]
struct Found {
    classes: Vec<String>,
    components: Vec<String>,
    enums: Vec<String>,
}

/// True when a line of the file starts with the marker (so mentions in comments don't count).
fn exports(source: &str, marker: &str) -> bool {
    source.lines().any(|line| line.trim_start().starts_with(marker))
}

fn scan(dir: &Path, module_path: &str, tree: &mut Tree, found: &mut Found) {
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();

    for path in entries {
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if path.is_dir() {
            let child_path = format!("{module_path}{name}::");
            scan(&path, &child_path, tree.dirs.entry(name).or_default(), found);
        } else if path.extension().is_some_and(|x| x == "rs") && name != "mod" {
            let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            tree.files.push((name.clone(), path.to_string_lossy().replace('\\', "/")));
            if exports(&source, CLASS_MARKER) {
                found.classes.push(format!("{module_path}{name}::CLASS"));
            }
            if exports(&source, COMPONENT_MARKER) {
                found.components.push(format!("{module_path}{name}::COMPONENT"));
            }
            if exports(&source, ENUMS_MARKER) {
                found.enums.push(format!("{module_path}{name}::ENUMS"));
            }
        }
    }
}

fn write_modules(tree: &Tree, depth: usize, out: &mut String) {
    let pad = "    ".repeat(depth);
    for (name, path) in &tree.files {
        out.push_str(&format!("{pad}#[path = \"{path}\"]\n{pad}pub mod {name};\n"));
    }
    for (name, sub) in &tree.dirs {
        out.push_str(&format!("{pad}pub mod {name} {{\n"));
        write_modules(sub, depth + 1, out);
        out.push_str(&format!("{pad}}}\n"));
    }
}

/// Emits `pub static NAME: &[&T] = &[&a::X, &b::X, ...];` (items are taken by reference).
fn write_list(out: &mut String, name: &str, ty: &str, items: &[String]) {
    out.push_str(&format!("\npub static {name}: &[&{ty}] = &[\n"));
    for item in items {
        out.push_str(&format!("    &{item},\n"));
    }
    out.push_str("];\n");
}

fn discover(manifest: &Path, folder: &str) -> (Tree, Found) {
    let mut tree = Tree::default();
    let mut found = Found::default();
    scan(&manifest.join("src").join(folder), "", &mut tree, &mut found);
    (tree, found)
}

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    println!("cargo:rerun-if-changed=build.rs");

    // Classes (and the components, enums they define themselves).
    let (tree, found) = discover(&manifest, "classes");
    let mut code = String::new();
    write_modules(&tree, 0, &mut code);
    write_list(&mut code, "ALL_CLASSES", "crate::core::ClassDesc", &found.classes);
    write_list(&mut code, "OWNED_COMPONENTS", "crate::core::ComponentDesc", &found.components);
    write_list(&mut code, "ENUM_LISTS", "[crate::core::EnumDesc]", &found.enums);
    fs::write(out_dir.join("classes_generated.rs"), code).expect("write classes_generated.rs");

    // Components shared by several classes.
    let (tree, found) = discover(&manifest, "components");
    assert!(found.classes.is_empty(), "src/components must not define classes: {:?}", found.classes);
    let mut code = String::new();
    write_modules(&tree, 0, &mut code);
    write_list(&mut code, "ALL_COMPONENTS", "crate::core::ComponentDesc", &found.components);
    fs::write(out_dir.join("components_generated.rs"), code).expect("write components_generated.rs");
}
