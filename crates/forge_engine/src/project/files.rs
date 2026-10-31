//! File operations on a project's files, for the Asset Browser and the script editor.
//!
//! Every operation takes project-relative paths, refuses anything that would leave the project, touch
//! the manifest or the editor's own `.forge` folder, or overwrite something that is already there, and
//! returns what moved (`old path -> new path`, one pair per *file*, also when a folder moved) so that
//! [`Project::remap_references`] can point scenes at the new places. Nothing is ever deleted outright:
//! [`Project::trash_path`] moves things into `.forge/trash`.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, ensure, Context, Result};
use walkdir::WalkDir;

use super::{file_name, join, kinds::stem_of, parent_of, write_atomic, AssetKind, Project, LEGACY_MANIFEST, MANIFEST, META_DIR};
use crate::{assets::clean_path, dm::file::PlaceFile};

/// What happened to one file offered to [`Project::import_files`].
pub type ImportOutcome = (PathBuf, std::result::Result<String, String>);

/// What [`Project::remap_references`] changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemapReport {
    /// Scene and prefab files that were rewritten.
    pub files: Vec<String>,
    /// References that were updated, in all of them.
    pub references: usize,
    /// The start scene moved and the manifest follows it.
    pub start_scene_changed: bool,
    /// The icon or the splash screen moved and the manifest follows it.
    pub launch_changed: bool,
}

/// Whether a name can be used for a file or a folder.
pub fn check_name(name: &str) -> Result<()> {
    ensure!(!name.trim().is_empty(), "the name is empty");
    ensure!(name == name.trim(), "the name cannot start or end with a space");
    ensure!(!name.ends_with('.'), "the name cannot end with a dot");
    ensure!(name != "." && name != "..", "'{name}' is not a usable name");
    ensure!(name.len() <= 150, "the name is too long");
    if let Some(bad) = name.chars().find(|c| c.is_control() || "<>:\"/\\|?*".contains(*c)) {
        bail!("the name cannot contain '{}'", if bad.is_control() { "control characters".to_owned() } else { bad.to_string() });
    }
    Ok(())
}

fn is_protected(rel: &str) -> bool {
    rel == MANIFEST || rel == LEGACY_MANIFEST || rel == META_DIR || rel.starts_with(&format!("{META_DIR}/"))
}

/// Pairs `(old file, new file)` for a file, or for every file in a folder, when `old` becomes `new`.
fn file_pairs(root: &Path, old: &str, new: &str) -> Vec<(String, String)> {
    let abs = root.join(old);
    if abs.is_dir() {
        let mut out: Vec<(String, String)> = WalkDir::new(&abs)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.path().strip_prefix(&abs).ok().map(|sub| sub.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")))
            .map(|sub| (join(old, &sub), join(new, &sub)))
            .collect();
        out.sort();
        out
    } else {
        vec![(old.to_owned(), new.to_owned())]
    }
}

fn copy_recursive(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            copy_recursive(&entry.path(), &to.join(name))?;
        }
    } else {
        fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    }
    Ok(())
}

impl Project {
    /// A project-relative path that exists, is inside the project and may be changed.
    fn existing(&self, rel: &str) -> Result<(String, PathBuf)> {
        let rel = clean_path(rel)?.trim_end_matches('/').to_owned();
        ensure!(!rel.is_empty(), "that is the project folder itself");
        ensure!(!is_protected(&rel), "'{rel}' belongs to the project and cannot be changed here");
        let abs = self.root.join(&rel);
        ensure!(abs.exists(), "'{rel}' does not exist (any more)");
        Ok((rel, abs))
    }

    /// Creates a folder called `name` inside `dir` and returns its path.
    pub fn create_folder(&self, dir: &str, name: &str) -> Result<String> {
        check_name(name)?;
        let dir = clean_path(dir)?.trim_end_matches('/').to_owned();
        ensure!(dir.is_empty() || self.root.join(&dir).is_dir(), "the folder '{dir}' does not exist");
        ensure!(!is_protected(&join(&dir, name)), "'{name}' is reserved");
        let rel = join(&dir, name);
        let abs = self.root.join(&rel);
        ensure!(!abs.exists(), "'{rel}' already exists");
        fs::create_dir_all(&abs).with_context(|| format!("creating {rel}"))?;
        Ok(rel)
    }

    /// Gives a file or folder a new name (it stays in its folder). Returns the files that moved.
    pub fn rename_path(&self, rel: &str, new_name: &str) -> Result<Vec<(String, String)>> {
        let (rel, abs) = self.existing(rel)?;
        check_name(new_name)?;
        let new_rel = join(parent_of(&rel), new_name);
        ensure!(!is_protected(&new_rel), "'{new_name}' is reserved");
        if new_rel == rel {
            return Ok(Vec::new());
        }
        let new_abs = self.root.join(&new_rel);
        let pairs = file_pairs(&self.root, &rel, &new_rel);
        if new_rel.to_lowercase() == rel.to_lowercase() {
            // Only the capitals differ: on a case-insensitive disk the target "exists", so go through a temporary name.
            let tmp = self.root.join(join(parent_of(&rel), &format!(".rename-{}", file_name(&rel))));
            fs::rename(&abs, &tmp).and_then(|()| fs::rename(&tmp, &new_abs)).with_context(|| format!("renaming {rel}"))?;
        } else {
            ensure!(!new_abs.exists(), "'{new_rel}' already exists");
            fs::rename(&abs, &new_abs).with_context(|| format!("renaming {rel}"))?;
        }
        Ok(pairs)
    }

    /// Moves a file or folder into the folder `dest_dir` (`""` is the project folder). Returns the files that moved.
    pub fn move_path(&self, rel: &str, dest_dir: &str) -> Result<Vec<(String, String)>> {
        let (rel, abs) = self.existing(rel)?;
        let dest = clean_path(dest_dir)?.trim_end_matches('/').to_owned();
        ensure!(dest.is_empty() || self.root.join(&dest).is_dir(), "the folder '{dest}' does not exist");
        ensure!(!is_protected(&dest), "files cannot be moved into '{dest}'");
        if dest == parent_of(&rel) {
            return Ok(Vec::new());
        }
        ensure!(dest != rel && !dest.starts_with(&format!("{rel}/")), "a folder cannot be moved into itself");
        let new_rel = join(&dest, file_name(&rel));
        let new_abs = self.root.join(&new_rel);
        ensure!(!new_abs.exists(), "'{new_rel}' already exists");
        let pairs = file_pairs(&self.root, &rel, &new_rel);
        fs::rename(&abs, &new_abs).with_context(|| format!("moving {rel}"))?;
        Ok(pairs)
    }

    /// Copies a file or folder next to the original (`Name Copy.ext`, `Name Copy 2.ext`, ...) and returns the copy's path.
    pub fn duplicate_path(&self, rel: &str) -> Result<String> {
        let (rel, abs) = self.existing(rel)?;
        let (dir, name) = (parent_of(&rel), file_name(&rel));
        let (stem, ext) = if abs.is_dir() { (name, "") } else { (stem_of(name), &name[stem_of(name).len()..]) };
        let mut n = 1;
        let copy = loop {
            let candidate = if n == 1 { format!("{stem} Copy{ext}") } else { format!("{stem} Copy {n}{ext}") };
            if !self.root.join(join(dir, &candidate)).exists() {
                break join(dir, &candidate);
            }
            n += 1;
        };
        copy_recursive(&abs, &self.root.join(&copy)).with_context(|| format!("duplicating {rel}"))?;
        Ok(copy)
    }

    /// "Deletes" a file or folder by moving it into `.forge/trash/<time>/`, so a mistake can be undone by
    /// hand. Returns where it went.
    pub fn trash_path(&self, rel: &str) -> Result<String> {
        let (rel, abs) = self.existing(rel)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let mut n = 0;
        let target = loop {
            let candidate = format!("{META_DIR}/trash/{stamp}-{n}/{rel}");
            if !self.root.join(&candidate).exists() {
                break candidate;
            }
            n += 1;
        };
        let target_abs = self.root.join(&target);
        if let Some(parent) = target_abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&abs, &target_abs).with_context(|| format!("deleting {rel}"))?;
        Ok(target)
    }

    /// Copies files and folders from anywhere on disk into the project folder `dest_dir`. A name that is
    /// taken gets a number (`crate2.obj`); a model with no geometry, a picture or a sound that cannot be read and a damaged
    /// material are refused (see [`check_importable`]). One result per source.
    pub fn import_files(&self, sources: &[PathBuf], dest_dir: &str) -> Vec<ImportOutcome> {
        let dest = match clean_path(dest_dir) {
            Ok(d) => d.trim_end_matches('/').to_owned(),
            Err(e) => return sources.iter().map(|s| (s.clone(), Err(format!("{e:#}")))).collect(),
        };
        let dest_ok = (dest.is_empty() || self.root.join(&dest).is_dir()) && !is_protected(&dest);
        sources
            .iter()
            .map(|source| {
                let outcome = (|| -> Result<String> {
                    ensure!(dest_ok, "the folder '{dest}' cannot take files");
                    ensure!(source.exists(), "{} does not exist", source.display());
                    ensure!(!source.starts_with(&self.root) || !source.starts_with(self.root.join(META_DIR)), "files inside .forge are not imported");
                    let name = source.file_name().and_then(|n| n.to_str()).context("the file has no usable name")?;
                    check_name(name)?;
                    if source.is_file() {
                        check_importable(name, source)?;
                    }
                    // A folder cannot be copied into itself.
                    ensure!(!(source.is_dir() && self.root.join(&dest).starts_with(source)), "a folder cannot be imported into itself");
                    let target_name = if source.is_dir() {
                        let mut n = 1;
                        loop {
                            let c = if n == 1 { name.to_owned() } else { format!("{name}{n}") };
                            if !self.root.join(join(&dest, &c)).exists() {
                                break c;
                            }
                            n += 1;
                        }
                    } else {
                        self.unique_file_name(&dest, stem_of(name), &name[stem_of(name).len()..])
                    };
                    let rel = join(&dest, &target_name);
                    copy_recursive(source, &self.root.join(&rel))?;
                    Ok(rel)
                })();
                (source.clone(), outcome.map_err(|e| format!("{e:#}")))
            })
            .collect()
    }

    /// After files moved, points every scene and prefab (and the start scene) at their new places.
    /// `moved` is `(old path, new path)`, as the operations above and [`super::AssetDb::scan`] report it.
    /// `skip_scene` is a scene the caller keeps in memory and updates itself.
    pub fn remap_references(&mut self, moved: &[(String, String)], skip_scene: Option<&str>) -> Result<RemapReport> {
        let mut report = RemapReport::default();
        if moved.is_empty() {
            return Ok(report);
        }
        let map: HashMap<String, String> = moved.iter().cloned().collect();
        for rel in self.scenes().into_iter().chain(self.prefabs()) {
            if skip_scene == Some(rel.as_str()) {
                continue;
            }
            let Ok(bytes) = fs::read(self.abs(&rel)?) else { continue };
            let Ok(mut place) = serde_json::from_slice::<PlaceFile>(&bytes) else { continue };
            let changed = place.remap_paths(&map);
            if changed > 0 {
                write_atomic(&self.abs(&rel)?, serde_json::to_string_pretty(&place)?)?;
                report.files.push(rel);
                report.references += changed;
            }
        }
        if let Some(new) = map.get(&self.manifest.project.start_scene) {
            self.manifest.project.start_scene = new.clone();
            report.start_scene_changed = true;
        }
        // The icon and the splash screen are named by their place in the project, so they follow their files.
        for file in [&mut self.manifest.launch.icon, &mut self.manifest.launch.splash] {
            if let Some(new) = file.as_ref().and_then(|f| map.get(f)) {
                *file = Some(new.clone());
                report.launch_changed = true;
            }
        }
        if report.start_scene_changed || report.launch_changed {
            self.save_manifest()?;
        }
        Ok(report)
    }
}

/// Checks that a file about to be imported is what its name says, so a broken file is refused with a reason now instead of
/// failing quietly in a game later: a model must have triangles, a picture must decode, a sound must be playable, a material
/// must parse. Other kinds are not looked at. (A `.gltf` that keeps its buffers in other files is not checked: they may be
/// imported after it.)
pub fn check_importable(name: &str, path: &Path) -> Result<()> {
    use crate::assets::{material::MaterialDef, mesh, texture};
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    let read = || fs::read(path).with_context(|| format!("reading {name}"));
    match AssetKind::of(name) {
        AssetKind::Mesh if mesh::is_loadable(name) && ext != "gltf" => {
            mesh::parse(ext, &read()?, "", &|_| None, 1.0).map(|_| ()).map_err(|e| anyhow::anyhow!("{name}: {e}, so it was not imported"))
        }
        AssetKind::Texture if texture::is_decodable(ext) => texture::decode(&read()?, ext).map(|_| ()).map_err(|e| anyhow::anyhow!("{name}: {e}, so it was not imported")),
        AssetKind::Audio if crate::audio::clip::is_audio(name) => crate::audio::clip::probe(read()?).map_err(|e| anyhow::anyhow!("{name}: {e}, so it was not imported")),
        AssetKind::Material => MaterialDef::parse(&read()?).map(|_| ()).map_err(|e| anyhow::anyhow!("{name}: {e}, so it was not imported")),
        _ => Ok(()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::AssetDb;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_files_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            fs::remove_dir_all(&dir).ok();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn project(s: &Scratch) -> Project {
        Project::create(&s.0, "Files Test").unwrap()
    }

    fn put(p: &Project, rel: &str, text: &str) {
        let path = p.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn names_are_checked() {
        assert!(check_name("Fine Name.luau").is_ok());
        for bad in ["", " x", "x ", "a/b", "a\\b", "a:b", "x.", "..", "a*b", "a\tb"] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn renaming_and_moving_a_script_updates_the_scenes_that_use_it_and_nothing_else() {
        let s = Scratch::new();
        let mut p = project(&s);
        let main = p.manifest.project.start_scene.clone();
        let before = p.read_place(&main).unwrap();
        let old = before.script_paths().into_iter().find(|(name, _)| name == "Spin").unwrap().1;

        let moved = p.rename_path(&old, "Spinner.luau").unwrap();
        assert_eq!(moved, [(old.clone(), join(parent_of(&old), "Spinner.luau"))]);
        let report = p.remap_references(&moved, None).unwrap();
        assert_eq!((report.files.clone(), report.references), (vec![main.clone()], 1));

        fs::create_dir_all(p.root.join("Gameplay/Props")).unwrap();
        let moved2 = p.move_path(&moved[0].1, "Gameplay/Props").unwrap();
        p.remap_references(&moved2, None).unwrap();
        let after = p.read_place(&main).unwrap();
        let spin = after.script_paths().into_iter().find(|(name, _)| name == "Spin").unwrap().1;
        assert_eq!(spin, "Gameplay/Props/Spinner.luau");
        assert!(p.root.join(&spin).is_file() && !p.root.join(&old).exists());
        // Only that reference changed: the other scripts are where they were.
        assert_eq!(after.script_paths().len(), before.script_paths().len());
        assert_eq!(after.script_paths().iter().filter(|(_, path)| before.script_paths().iter().any(|(_, b)| b == path)).count(), before.script_paths().len() - 1);
        // And the scene still loads with every script found.
        assert!(crate::project::export::validate(&p).iter().all(|d| d.severity != crate::project::export::Severity::Error), "{:?}", crate::project::export::validate(&p));
    }

    #[test]
    fn the_icon_and_the_splash_screen_follow_their_files() {
        let s = Scratch::new();
        let mut p = project(&s);
        put(&p, "Assets/Textures/icon.png", "x");
        put(&p, "Assets/Textures/splash.png", "x");
        p.manifest.launch.icon = Some("Assets/Textures/icon.png".into());
        p.manifest.launch.splash = Some("Assets/Textures/splash.png".into());
        p.save_manifest().unwrap();
        let moved = p.move_path("Assets/Textures", "Assets/Models").unwrap();
        let report = p.remap_references(&moved, None).unwrap();
        assert!(report.launch_changed && !report.start_scene_changed);
        assert_eq!(p.manifest.launch.icon.as_deref(), Some("Assets/Models/Textures/icon.png"));
        let reopened = Project::open(&p.root).unwrap();
        assert_eq!(reopened.manifest.launch.splash.as_deref(), Some("Assets/Models/Textures/splash.png"), "and it was saved");
    }

    #[test]
    fn moving_a_folder_moves_every_file_in_it_and_follows_the_start_scene() {
        let s = Scratch::new();
        let mut p = project(&s);
        let start = p.manifest.project.start_scene.clone();
        let moved = p.rename_path("Scenes", "Levels").unwrap();
        assert!(moved.iter().any(|(old, new)| old == &start && new == "Levels/MainScene.scene"), "{moved:?}");
        let report = p.remap_references(&moved, None).unwrap();
        assert!(report.start_scene_changed);
        assert_eq!(p.manifest.project.start_scene, "Levels/MainScene.scene");
        assert_eq!(Project::open(&p.root).unwrap().manifest.project.start_scene, "Levels/MainScene.scene", "and it was saved");
    }

    #[test]
    fn nothing_is_overwritten_or_moved_where_it_cannot_go() {
        let s = Scratch::new();
        let p = project(&s);
        put(&p, "A/one.txt", "1");
        put(&p, "B/one.txt", "2");
        assert!(p.move_path("A/one.txt", "B").unwrap_err().to_string().contains("already exists"));
        assert!(p.rename_path("A/one.txt", "..").is_err());
        put(&p, "A/two.txt", "x");
        assert!(p.rename_path("A/one.txt", "two.txt").unwrap_err().to_string().contains("already exists"));
        fs::create_dir_all(p.root.join("A/inner")).unwrap();
        assert!(p.move_path("A", "A/inner").unwrap_err().to_string().contains("into itself"));
        assert!(p.rename_path(MANIFEST, "x.toml").is_err() && p.trash_path(MANIFEST).is_err() && p.move_path(META_DIR, "A").is_err());
        assert!(p.rename_path("nothing/here.txt", "x.txt").is_err());
        assert!(p.move_path("../outside.txt", "A").is_err());
        assert_eq!(fs::read_to_string(p.root.join("B/one.txt")).unwrap(), "2", "the file in the way is untouched");
        assert_eq!(p.move_path("A/one.txt", "A").unwrap(), Vec::<(String, String)>::new(), "moving into its own folder does nothing");
    }

    #[test]
    fn a_name_that_only_changes_case_is_a_rename() {
        let s = Scratch::new();
        let p = project(&s);
        put(&p, "Assets/crate.obj", "v 0 0 0\n");
        let moved = p.rename_path("Assets/crate.obj", "Crate.obj").unwrap();
        assert_eq!(moved, [("Assets/crate.obj".to_owned(), "Assets/Crate.obj".to_owned())]);
        let names: Vec<String> = fs::read_dir(p.root.join("Assets")).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.contains(&"Crate.obj".to_owned()) && !names.iter().any(|n| n.starts_with(".rename")), "{names:?}");
    }

    #[test]
    fn duplicates_get_free_names_and_deleting_moves_to_the_trash() {
        let s = Scratch::new();
        let p = project(&s);
        put(&p, "Shared/Modules/Util.luau", "return {}");
        assert_eq!(p.duplicate_path("Shared/Modules/Util.luau").unwrap(), "Shared/Modules/Util Copy.luau");
        assert_eq!(p.duplicate_path("Shared/Modules/Util.luau").unwrap(), "Shared/Modules/Util Copy 2.luau");
        put(&p, "Assets/Models/a.obj", "v 0 0 0\n");
        assert_eq!(p.duplicate_path("Assets/Models").unwrap(), "Assets/Models Copy");
        assert!(p.root.join("Assets/Models Copy/a.obj").is_file());

        let gone = p.trash_path("Shared/Modules/Util.luau").unwrap();
        assert!(!p.root.join("Shared/Modules/Util.luau").exists());
        assert_eq!(fs::read_to_string(p.root.join(&gone)).unwrap(), "return {}", "recoverable from {gone}");
        let (folders, files) = p.walk();
        assert!(!files.iter().any(|f| f.path.starts_with(META_DIR)) && !folders.iter().any(|f| f.starts_with(META_DIR)), "the trash is not part of the project's content");
    }

    #[test]
    fn importing_copies_in_with_free_names_and_refuses_broken_meshes() {
        let s = Scratch::new();
        let p = project(&s);
        let outside = Scratch::new();
        fs::create_dir_all(&outside.0).unwrap();
        fs::write(outside.0.join("crate.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        fs::write(outside.0.join("empty.obj"), "# nothing here\n").unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255])).save(outside.0.join("tex.png")).unwrap();
        fs::write(outside.0.join("broken.png"), "png").unwrap();
        fs::write(outside.0.join("broken.wav"), "wav").unwrap();
        fs::write(outside.0.join("good.mat"), crate::assets::material::MaterialDef::default().to_json()).unwrap();
        fs::write(outside.0.join("bad.mat"), "{ nope").unwrap();
        fs::create_dir_all(outside.0.join("pack")).unwrap();
        fs::write(outside.0.join("pack/inner.wav"), "wav").unwrap();

        let sources = vec![outside.0.join("crate.obj"), outside.0.join("empty.obj"), outside.0.join("missing.obj"), outside.0.join("tex.png"), outside.0.join("pack")];
        let results = p.import_files(&sources, "Assets/Models");
        assert_eq!(results[0].1, Ok("Assets/Models/crate.obj".to_owned()));
        assert!(results[1].1.as_ref().unwrap_err().contains("no faces"), "{:?}", results[1]);
        assert!(results[2].1.as_ref().unwrap_err().contains("does not exist"));
        assert_eq!(results[3].1, Ok("Assets/Models/tex.png".to_owned()));
        assert_eq!(results[4].1, Ok("Assets/Models/pack".to_owned()));
        assert!(p.root.join("Assets/Models/pack/inner.wav").is_file());
        assert!(!p.root.join("Assets/Models/empty.obj").exists(), "a refused import leaves nothing behind");
        // Pictures, sounds and materials are checked too: a damaged one is refused with a reason, not discovered in a game.
        let checked = p.import_files(&[outside.0.join("broken.png"), outside.0.join("broken.wav"), outside.0.join("good.mat"), outside.0.join("bad.mat")], "Assets/Models");
        assert!(checked[0].1.as_ref().unwrap_err().contains("not imported"), "{:?}", checked[0]);
        assert!(checked[1].1.as_ref().unwrap_err().contains("not imported"), "{:?}", checked[1]);
        assert_eq!(checked[2].1, Ok("Assets/Models/good.mat".to_owned()));
        assert!(checked[3].1.as_ref().unwrap_err().contains("not a valid material"), "{:?}", checked[3]);
        assert!(!p.root.join("Assets/Models/broken.png").exists() && !p.root.join("Assets/Models/bad.mat").exists());

        let again = p.import_files(&sources[..1], "Assets/Models");
        assert_eq!(again[0].1, Ok("Assets/Models/crate2.obj".to_owned()), "an existing name is never overwritten");
        assert!(p.import_files(&sources[..1], "../elsewhere")[0].1.is_err());
        // The new files are found, and an external move of one of them is recognised.
        let (db, _) = AssetDb::scan(&p, None);
        assert!(db.entry("Assets/Models/crate.obj").is_some());
    }
}
