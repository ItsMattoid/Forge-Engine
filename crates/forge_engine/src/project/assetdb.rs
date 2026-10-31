//! The index of a project's files: what the Asset Browser lists and what the editor asks when it needs
//! to know what a file is, whether anything uses it and where it went.
//!
//! * **Discovery** walks the real folders ([`Project::walk`]); nothing is registered by hand.
//! * **Stable ids**: every file gets an id that is kept in `.forge/assets.json`. When a file vanishes
//!   from one path and a file with the same contents (size and a fingerprint of its start and end)
//!   appears at another, the scan reports it as a *move* and the id follows it. That is how a file moved
//!   or renamed outside the editor is recognised, so the references to it can be updated (see
//!   [`super::files`]) instead of being left dangling.
//! * **Dependencies**: scenes and prefabs are read for the scripts and assets they use (and the manifest
//!   for the start scene), so "what uses this file" can be answered before it is deleted.
//!
//! A scan costs one walk of the folders plus a read of the few files that are new or changed.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use super::{file_name, kinds::parent_of, write_atomic, AssetKind, FileInfo, Project, LEGACY_MANIFEST, MANIFEST, META_DIR};
use crate::dm::file::PlaceFile;

/// Where the ids are remembered (inside the project, so they travel with it).
pub const INDEX_FILE: &str = ".forge/assets.json";

/// How much of the start and of the end of a file is looked at to recognise it again.
const PRINT_BYTES: u64 = 32 * 1024;

#[derive(Serialize, Deserialize, Default)]
struct IndexFile {
    version: u32,
    next_id: u64,
    assets: Vec<IndexRec>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct IndexRec {
    id: u64,
    path: String,
    size: u64,
    /// Milliseconds since the epoch.
    mtime: u64,
    print: u64,
}

/// One file of the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    /// Stable across moves and renames (see the module docs).
    pub id: u64,
    pub path: String,
    pub kind: AssetKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    print: u64,
}

impl AssetEntry {
    pub fn name(&self) -> &str {
        file_name(&self.path)
    }
}

/// What a scan found out compared with the one before.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Files that are now somewhere else: `(old path, new path)`.
    pub moved: Vec<(String, String)>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// Files at the same path whose size or modification time is not what it was.
    pub modified: Vec<String>,
    /// Anything about the list of files or folders is different (also when only a file's contents changed).
    pub changed: bool,
}

#[derive(Clone, Debug, Default)]
struct DirContents {
    folders: Vec<String>,
    files: Vec<usize>,
}

#[derive(Clone, Debug)]
struct DepRec {
    size: u64,
    modified: Option<SystemTime>,
    files: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AssetDb {
    entries: Vec<AssetEntry>,
    folders: Vec<String>,
    by_path: HashMap<String, usize>,
    dirs: BTreeMap<String, DirContents>,
    /// Scene / prefab / manifest path -> the files it uses.
    deps: HashMap<String, DepRec>,
    /// File path -> the scenes / prefabs / manifest that use it.
    used_by: HashMap<String, Vec<String>>,
    next_id: u64,
    /// The index file as last written, so an unchanged index is not written again.
    written: String,
    /// Grows whenever the files or folders differ from the scan before.
    pub generation: u64,
}

fn millis(t: Option<SystemTime>) -> u64 {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

/// A fingerprint of a file: its size and its first and last bytes (FNV-1a). Enough to tell that a file
/// that turned up somewhere is the one that disappeared from somewhere else, without reading all of it.
fn fingerprint(path: &Path, size: u64) -> u64 {
    fn feed(hash: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *hash ^= *b as u64;
            *hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    let mut hash = 0xcbf29ce484222325u64;
    feed(&mut hash, &size.to_le_bytes());
    let Ok(mut file) = fs::File::open(path) else { return hash };
    let mut buf = vec![0u8; PRINT_BYTES.min(size) as usize];
    if file.read_exact(&mut buf).is_ok() {
        feed(&mut hash, &buf);
    }
    if size > PRINT_BYTES {
        let from = size.saturating_sub(PRINT_BYTES).max(PRINT_BYTES);
        if file.seek(SeekFrom::Start(from)).is_ok() {
            buf.clear();
            if file.take(PRINT_BYTES).read_to_end(&mut buf).is_ok() {
                feed(&mut hash, &buf);
            }
        }
    }
    hash
}

fn case_insensitive_key(path: &str) -> String {
    path.to_lowercase()
}

impl AssetDb {
    /// Scans the project. `previous` is the result of the scan before (it keeps the ids and avoids
    /// re-reading what did not change); without it the remembered index on disk is used.
    pub fn scan(project: &Project, previous: Option<&AssetDb>) -> (AssetDb, SyncReport) {
        let (folders, files) = project.walk();

        // What was known: the previous scan, or the index file.
        let (mut next_id, known): (u64, HashMap<String, IndexRec>) = match previous {
            Some(p) => (p.next_id, p.entries.iter().map(|e| (e.path.clone(), IndexRec { id: e.id, path: e.path.clone(), size: e.size, mtime: millis(e.modified), print: e.print })).collect()),
            None => {
                let index: IndexFile = fs::read_to_string(project.root.join(INDEX_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
                (index.next_id, index.assets.into_iter().map(|r| (r.path.clone(), r)).collect())
            }
        };

        let found: HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
        let mut gone: Vec<IndexRec> = known.values().filter(|r| !found.contains(r.path.as_str())).cloned().collect();
        gone.sort_by(|a, b| a.path.cmp(&b.path));
        let mut gone_by_print: HashMap<(u64, u64), Vec<IndexRec>> = HashMap::new();
        for rec in gone {
            gone_by_print.entry((rec.print, rec.size)).or_default().push(rec);
        }

        let mut entries: Vec<AssetEntry> = Vec::with_capacity(files.len());
        let mut report = SyncReport::default();
        let abs = |f: &FileInfo| project.root.join(&f.path);
        for f in &files {
            let kind = AssetKind::of(file_name(&f.path));
            let (id, print) = match known.get(&f.path) {
                Some(rec) if rec.size == f.size && rec.mtime == millis(f.modified) => (rec.id, rec.print),
                Some(rec) => (rec.id, fingerprint(&abs(f), f.size)),
                None => {
                    let print = fingerprint(&abs(f), f.size);
                    // The same contents that left another path: the file was moved or renamed.
                    let ext = |p: &str| p.rsplit('.').next().map(str::to_ascii_lowercase);
                    // (An empty file looks like every other empty file, so it is never taken for a moved one.)
                    let taken = gone_by_print.get_mut(&(print, f.size)).filter(|_| f.size > 0).and_then(|candidates| {
                        let same_ext: Vec<usize> = (0..candidates.len()).filter(|i| ext(&candidates[*i].path) == ext(&f.path)).collect();
                        let pick = match same_ext.len() {
                            0 => None,
                            1 => Some(same_ext[0]),
                            _ => same_ext.iter().copied().find(|i| file_name(&candidates[*i].path) == file_name(&f.path)),
                        };
                        pick.map(|i| candidates.remove(i))
                    });
                    match taken {
                        Some(old) => {
                            report.moved.push((old.path, f.path.clone()));
                            (old.id, print)
                        }
                        None => {
                            next_id += 1;
                            report.added.push(f.path.clone());
                            (next_id, print)
                        }
                    }
                }
            };
            entries.push(AssetEntry { id, path: f.path.clone(), kind, size: f.size, modified: f.modified, print });
        }
        report.modified = entries.iter().filter(|e| known.get(&e.path).is_some_and(|rec| rec.size != e.size || rec.mtime != millis(e.modified))).map(|e| e.path.clone()).collect();
        report.removed = gone_by_print.into_values().flatten().map(|r| r.path).collect();
        report.removed.sort();
        report.moved.sort();

        let mut db = AssetDb { entries, folders, next_id, ..AssetDb::default() };
        db.index_paths();
        db.read_dependencies(project, previous);

        let unchanged = previous.is_some_and(|p| p.entries == db.entries && p.folders == db.folders);
        report.changed = !unchanged;
        db.generation = previous.map_or(1, |p| if unchanged { p.generation } else { p.generation + 1 });
        db.written = previous.map(|p| p.written.clone()).unwrap_or_default();
        db.save_index(project);
        (db, report)
    }

    fn index_paths(&mut self) {
        self.by_path = self.entries.iter().enumerate().map(|(i, e)| (e.path.clone(), i)).collect();
        self.dirs.clear();
        self.dirs.insert(String::new(), DirContents::default());
        for folder in &self.folders {
            self.dirs.entry(folder.clone()).or_default();
            self.dirs.entry(parent_of(folder).to_owned()).or_default().folders.push(folder.clone());
        }
        for (i, e) in self.entries.iter().enumerate() {
            self.dirs.entry(parent_of(&e.path).to_owned()).or_default().files.push(i);
        }
        let entries = &self.entries;
        for contents in self.dirs.values_mut() {
            contents.folders.sort_by_key(|f| case_insensitive_key(file_name(f)));
            contents.files.sort_by_key(|i| case_insensitive_key(entries[*i].name()));
        }
    }

    /// Reads what each scene and prefab uses (only those that are new or changed), and the manifest's start scene.
    fn read_dependencies(&mut self, project: &Project, previous: Option<&AssetDb>) {
        for e in self.entries.iter().filter(|e| matches!(e.kind, AssetKind::Scene | AssetKind::Prefab)) {
            let reuse = previous.and_then(|p| p.deps.get(&e.path)).filter(|d| d.size == e.size && d.modified == e.modified);
            let rec = match reuse {
                Some(d) => d.clone(),
                None => {
                    let files = fs::read(project.root.join(&e.path)).ok().and_then(|b| serde_json::from_slice::<PlaceFile>(&b).ok()).map(|p| p.referenced_files()).unwrap_or_default();
                    DepRec { size: e.size, modified: e.modified, files }
                }
            };
            self.deps.insert(e.path.clone(), rec);
        }
        // (The manifest is read when it is scanned; its start scene is the only file it names.)
        let start = project.manifest.project.start_scene.clone();
        if !start.is_empty() {
            self.deps.insert(project.manifest_file.to_owned(), DepRec { size: 0, modified: None, files: vec![start] });
        }
        for (user, rec) in &self.deps {
            for file in &rec.files {
                self.used_by.entry(file.clone()).or_default().push(user.clone());
            }
        }
        for users in self.used_by.values_mut() {
            users.sort();
            users.dedup();
        }
    }

    fn save_index(&mut self, project: &Project) {
        let mut assets: Vec<IndexRec> = self.entries.iter().map(|e| IndexRec { id: e.id, path: e.path.clone(), size: e.size, mtime: millis(e.modified), print: e.print }).collect();
        assets.sort_by_key(|r| r.id);
        let Ok(text) = serde_json::to_string_pretty(&IndexFile { version: 1, next_id: self.next_id, assets }) else { return };
        if text == self.written {
            return;
        }
        // (A failure to write the index is not an error: ids are then simply not remembered between sessions.)
        let dir = project.root.join(META_DIR);
        if fs::create_dir_all(&dir).is_ok() && write_atomic(&project.root.join(INDEX_FILE), &text).is_ok() {
            self.written = text;
        }
    }

    /// Tells the index that files moved (old -> new), so the next scan sees them where they are and
    /// does not report the same moves again.
    pub fn record_moves(&mut self, moved: &[(String, String)]) {
        let map: HashMap<&str, &str> = moved.iter().map(|(o, n)| (o.as_str(), n.as_str())).collect();
        for e in &mut self.entries {
            if let Some(new) = map.get(e.path.as_str()) {
                e.path = (*new).to_owned();
                e.kind = AssetKind::of(file_name(new));
            }
        }
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        self.index_paths();
    }

    // ---- queries -------------------------------------------------------------------------------

    pub fn entries(&self) -> &[AssetEntry] {
        &self.entries
    }

    pub fn folders(&self) -> &[String] {
        &self.folders
    }

    pub fn entry(&self, path: &str) -> Option<&AssetEntry> {
        self.by_path.get(path).map(|i| &self.entries[*i])
    }

    pub fn has_folder(&self, path: &str) -> bool {
        path.is_empty() || self.dirs.contains_key(path)
    }

    /// The sub-folders (paths) and files directly inside `dir` (`""` is the project root), each sorted by name.
    pub fn contents(&self, dir: &str) -> (&[String], Vec<&AssetEntry>) {
        match self.dirs.get(dir) {
            Some(c) => (&c.folders, c.files.iter().map(|i| &self.entries[*i]).collect()),
            None => (&[], Vec::new()),
        }
    }

    /// The scenes, prefabs and manifest that use this file (or, for a folder, anything inside it).
    pub fn used_by(&self, path: &str) -> Vec<String> {
        let prefix = format!("{path}/");
        let mut users: Vec<String> = self.used_by.iter().filter(|(file, _)| file.as_str() == path || file.starts_with(&prefix)).flat_map(|(_, users)| users.iter().cloned()).collect();
        users.sort();
        users.dedup();
        // (A file inside the folder being asked about does not count as using it.)
        users.retain(|u| u != path && !u.starts_with(&prefix));
        users
    }

    /// The files a scene or prefab uses.
    pub fn dependencies_of(&self, path: &str) -> Vec<String> {
        self.deps.get(path).map(|d| d.files.clone()).unwrap_or_default()
    }

    /// Everything inside `dir` (recursively) that matches the search, folders first. The root is `""`.
    pub fn search(&self, dir: &str, query: &SearchQuery) -> Vec<SearchHit> {
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let mut hits = Vec::new();
        for folder in self.folders.iter().filter(|f| f.starts_with(&prefix)) {
            if query.matches(file_name(folder), AssetKind::Folder) {
                hits.push(SearchHit::Folder(folder.clone()));
            }
        }
        for e in self.entries.iter().filter(|e| e.path.starts_with(&prefix)) {
            if query.matches(e.name(), e.kind) {
                hits.push(SearchHit::Asset(e.path.clone()));
            }
        }
        hits
    }

    pub fn is_manifest(path: &str) -> bool {
        path == MANIFEST || path == LEGACY_MANIFEST
    }
}

/// One result of a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchHit {
    Folder(String),
    Asset(String),
}

/// What the Asset Browser's search box means: words that must all appear in the name, `type:mesh` to
/// keep one kind, `ext:png` (or `.png`) to keep one extension. Case is ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    words: Vec<String>,
    kinds: Vec<AssetKind>,
    exts: Vec<String>,
}

impl SearchQuery {
    pub fn parse(text: &str) -> SearchQuery {
        let mut q = SearchQuery::default();
        for token in text.split_whitespace() {
            let lower = token.to_lowercase();
            if let Some(word) = lower.strip_prefix("type:").or_else(|| lower.strip_prefix("t:")) {
                match AssetKind::from_word(word) {
                    Some(kind) => q.kinds.push(kind),
                    // (An unknown type matches nothing rather than everything.)
                    None => q.words.push(lower.clone()),
                }
            } else if let Some(ext) = lower.strip_prefix("ext:").or_else(|| lower.strip_prefix("e:")).or_else(|| lower.strip_prefix('.')) {
                if !ext.is_empty() {
                    q.exts.push(ext.to_owned());
                }
            } else {
                q.words.push(lower);
            }
        }
        q
    }

    /// Whether there is anything to search for.
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.kinds.is_empty() && self.exts.is_empty()
    }

    pub fn matches(&self, name: &str, kind: AssetKind) -> bool {
        let lower = name.to_lowercase();
        (self.kinds.is_empty() || self.kinds.contains(&kind))
            && (self.exts.is_empty() || self.exts.iter().any(|e| lower.ends_with(&format!(".{e}"))))
            && self.words.iter().all(|w| lower.contains(w.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_assetdb_test_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            fs::remove_dir_all(&dir).ok();
            Scratch(dir)
        }
        fn project(&self) -> Project {
            Project::create(&self.0, "Index Test").expect("create project")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn write(project: &Project, rel: &str, text: &str) {
        let path = project.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn discovery_finds_everything_wherever_it_is_and_skips_hidden_and_build_folders() {
        let s = Scratch::new();
        let p = s.project();
        write(&p, "Anywhere/Deep/Down/thing.luau", "print(1)");
        write(&p, "Assets/Models/crate.obj", "v 0 0 0\n");
        write(&p, "build/old/junk.luau", "x");
        write(&p, ".git/config", "x");
        let (db, report) = AssetDb::scan(&p, None);
        assert!(db.entry("Anywhere/Deep/Down/thing.luau").is_some_and(|e| e.kind == AssetKind::Script));
        assert!(db.entry("Assets/Models/crate.obj").is_some_and(|e| e.kind == AssetKind::Mesh));
        assert!(db.entry("build/old/junk.luau").is_none() && db.entry(".git/config").is_none());
        assert!(db.has_folder("Anywhere/Deep") && db.has_folder("Assets/Textures") && !db.has_folder(".git"));
        assert!(report.added.iter().any(|a| a == "Assets/Models/crate.obj") && report.moved.is_empty());
        assert!(p.scripts().iter().any(|s| s == "Anywhere/Deep/Down/thing.luau"), "scripts are found outside Server / Client / Shared");
        assert!(!p.scripts().iter().any(|s| s.starts_with("build/")));
    }

    #[test]
    fn an_id_follows_a_file_that_is_moved_or_renamed_outside_the_editor() {
        let s = Scratch::new();
        let p = s.project();
        write(&p, "Assets/Models/crate.obj", "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
        write(&p, "Assets/Models/barrel.obj", "v 5 5 5\nf 1 1 1\n");
        let (db, _) = AssetDb::scan(&p, None);
        let crate_id = db.entry("Assets/Models/crate.obj").unwrap().id;
        let barrel_id = db.entry("Assets/Models/barrel.obj").unwrap().id;
        assert_ne!(crate_id, barrel_id);

        fs::create_dir_all(p.root.join("Props")).unwrap();
        fs::rename(p.root.join("Assets/Models/crate.obj"), p.root.join("Props/box.obj")).unwrap();
        let (db2, report) = AssetDb::scan(&p, Some(&db));
        assert_eq!(report.moved, [("Assets/Models/crate.obj".to_owned(), "Props/box.obj".to_owned())]);
        assert_eq!(db2.entry("Props/box.obj").unwrap().id, crate_id, "same file, same id");
        assert_eq!(db2.entry("Assets/Models/barrel.obj").unwrap().id, barrel_id);
        assert!(report.added.is_empty() && report.removed.is_empty() && report.changed);
        assert!(db2.generation > db.generation);

        // A scan with nothing new reports nothing.
        let (db3, quiet) = AssetDb::scan(&p, Some(&db2));
        assert!(!quiet.changed && quiet.moved.is_empty());
        assert_eq!(db3.generation, db2.generation);
    }

    #[test]
    fn ids_are_remembered_between_sessions_and_a_deleted_or_new_file_is_reported() {
        let s = Scratch::new();
        let p = s.project();
        write(&p, "Assets/a.obj", "v 1 2 3\n");
        let (first, _) = AssetDb::scan(&p, None);
        let id = first.entry("Assets/a.obj").unwrap().id;
        drop(first);

        // A new session: no previous scan, only the index file.
        fs::rename(p.root.join("Assets/a.obj"), p.root.join("Assets/b.obj")).unwrap();
        write(&p, "Assets/new.obj", "v 9 9 9\n");
        let (second, report) = AssetDb::scan(&p, None);
        assert_eq!(report.moved, [("Assets/a.obj".to_owned(), "Assets/b.obj".to_owned())]);
        assert_eq!(second.entry("Assets/b.obj").unwrap().id, id);
        assert_eq!(report.added, ["Assets/new.obj"]);

        fs::remove_file(p.root.join("Assets/new.obj")).unwrap();
        let (_, report) = AssetDb::scan(&p, Some(&second));
        assert_eq!(report.removed, ["Assets/new.obj"]);
    }

    #[test]
    fn identical_files_are_not_mistaken_for_each_other_and_a_damaged_index_is_ignored() {
        let s = Scratch::new();
        let p = s.project();
        write(&p, "A/same.obj", "v 1 1 1\n");
        write(&p, "B/same.obj", "v 1 1 1\n");
        let (db, _) = AssetDb::scan(&p, None);
        let (a, b) = (db.entry("A/same.obj").unwrap().id, db.entry("B/same.obj").unwrap().id);
        assert_ne!(a, b);
        fs::remove_file(p.root.join("A/same.obj")).unwrap();
        let (db2, report) = AssetDb::scan(&p, Some(&db));
        assert_eq!(db2.entry("B/same.obj").unwrap().id, b, "the file that stayed keeps its id");
        assert_eq!(report.removed, ["A/same.obj"]);

        fs::write(p.root.join(INDEX_FILE), "{ not json").unwrap();
        let (db3, _) = AssetDb::scan(&p, None);
        assert!(db3.entry("B/same.obj").is_some(), "a damaged index only loses the remembered ids");
    }

    #[test]
    fn directory_listings_are_sorted_with_folders_first_and_searches_are_recursive() {
        let s = Scratch::new();
        let p = s.project();
        write(&p, "Assets/Models/Characters/hero.obj", "v 0 0 0\n");
        write(&p, "Assets/Models/tree.obj", "v 0 0 0\n");
        write(&p, "Assets/Textures/hero.png", "png");
        write(&p, "Assets/Models/Zebra.obj", "v 0 0 0\n");
        let (db, _) = AssetDb::scan(&p, None);
        let (folders, files) = db.contents("Assets/Models");
        assert_eq!(folders, ["Assets/Models/Characters"]);
        assert_eq!(files.iter().map(|e| e.name()).collect::<Vec<_>>(), ["tree.obj", "Zebra.obj"], "sorted by name, ignoring case");
        assert!(db.contents("nowhere").0.is_empty());

        let names = |hits: Vec<SearchHit>| -> Vec<String> { hits.into_iter().map(|h| match h { SearchHit::Folder(f) => f, SearchHit::Asset(a) => a }).collect() };
        assert_eq!(names(db.search("Assets", &SearchQuery::parse("hero"))), ["Assets/Models/Characters/hero.obj", "Assets/Textures/hero.png"]);
        assert_eq!(names(db.search("Assets/Models", &SearchQuery::parse("hero"))), ["Assets/Models/Characters/hero.obj"], "only inside the folder being browsed");
        assert_eq!(names(db.search("", &SearchQuery::parse("type:texture"))), ["Assets/Textures/hero.png"]);
        assert_eq!(names(db.search("", &SearchQuery::parse("hero ext:obj"))), ["Assets/Models/Characters/hero.obj"]);
        assert_eq!(names(db.search("", &SearchQuery::parse(".png"))), ["Assets/Textures/hero.png"]);
        assert_eq!(names(db.search("", &SearchQuery::parse("type:folder chara"))), ["Assets/Models/Characters"]);
        assert!(db.search("", &SearchQuery::parse("type:nonsense")).is_empty(), "an unknown type matches nothing");
        assert!(SearchQuery::parse("   ").is_empty());
    }

    #[test]
    fn scenes_are_read_for_what_they_use() {
        let s = Scratch::new();
        let p = s.project();
        let (db, _) = AssetDb::scan(&p, None);
        let main = p.manifest.project.start_scene.clone();
        let deps = db.dependencies_of(&main);
        assert!(deps.iter().any(|d| d.ends_with("FirstPersonController.luau")), "{deps:?}");
        let controller = deps.iter().find(|d| d.ends_with("FirstPersonController.luau")).unwrap();
        assert_eq!(db.used_by(controller), [main.clone()]);
        assert_eq!(db.used_by(&main), [MANIFEST], "the manifest uses its start scene");
        // (The template's coins make a sound: the scene uses it, and so, as a folder, does everything above it.)
        assert_eq!(db.used_by("Assets/Audio/Coin.wav"), [main.clone()]);
        assert!(!db.used_by("Assets").is_empty());
        assert!(db.used_by("Assets/Materials").is_empty(), "the starter materials are only used by name, by what draws with them");
        assert!(!db.used_by("Client").is_empty(), "a folder is used when anything inside it is");
    }
}
