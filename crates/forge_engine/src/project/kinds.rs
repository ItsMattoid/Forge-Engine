//! What a file in a project is, judged by its name: the kind the Asset Browser shows and the
//! engine decides what it can do with.

use std::path::Path;

/// Scene files: `Scenes/MainScene.scene` (older projects: `scenes/main.scene.json`).
pub const SCENE_EXT: &str = ".scene";
pub const LEGACY_SCENE_EXT: &str = ".scene.json";
/// A saved group of objects that can be placed into a scene.
pub const PREFAB_EXT: &str = ".prefab";
pub const SCRIPT_EXT: &str = ".luau";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AssetKind {
    Folder,
    Scene,
    Prefab,
    /// Luau source: the only scripting language.
    Script,
    Mesh,
    Texture,
    Material,
    Audio,
    Animation,
    Font,
    /// Plain data and documents (`json`, `toml`, `txt`, ...).
    Data,
    Other,
}

impl AssetKind {
    pub const ALL: [AssetKind; 12] = [
        AssetKind::Folder,
        AssetKind::Scene,
        AssetKind::Prefab,
        AssetKind::Script,
        AssetKind::Mesh,
        AssetKind::Texture,
        AssetKind::Material,
        AssetKind::Audio,
        AssetKind::Animation,
        AssetKind::Font,
        AssetKind::Data,
        AssetKind::Other,
    ];

    /// The kind of a file by its name.
    pub fn of(name: &str) -> AssetKind {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(SCENE_EXT) || lower.ends_with(LEGACY_SCENE_EXT) {
            return AssetKind::Scene;
        }
        let ext = Path::new(&lower).extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "prefab" => AssetKind::Prefab,
            "luau" => AssetKind::Script,
            "obj" | "fbx" | "gltf" | "glb" | "dae" | "stl" | "ply" => AssetKind::Mesh,
            "png" | "jpg" | "jpeg" | "bmp" | "tga" | "gif" | "webp" | "hdr" | "dds" | "ktx2" | "tif" | "tiff" => AssetKind::Texture,
            "mat" | "material" => AssetKind::Material,
            "wav" | "mp3" | "ogg" | "flac" | "m4a" => AssetKind::Audio,
            "anim" | "animation" => AssetKind::Animation,
            "ttf" | "otf" | "woff" | "woff2" => AssetKind::Font,
            "json" | "toml" | "txt" | "md" | "csv" | "xml" | "yaml" | "yml" => AssetKind::Data,
            _ => AssetKind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AssetKind::Folder => "Folder",
            AssetKind::Scene => "Scene",
            AssetKind::Prefab => "Prefab",
            AssetKind::Script => "Luau Script",
            AssetKind::Mesh => "Mesh",
            AssetKind::Texture => "Texture",
            AssetKind::Material => "Material",
            AssetKind::Audio => "Audio",
            AssetKind::Animation => "Animation",
            AssetKind::Font => "Font",
            AssetKind::Data => "Data",
            AssetKind::Other => "File",
        }
    }

    /// The words a search for a type understands (`type:mesh`).
    pub fn from_word(word: &str) -> Option<AssetKind> {
        let w = word.trim().to_ascii_lowercase();
        Some(match w.as_str() {
            "folder" | "dir" | "directory" => AssetKind::Folder,
            "scene" | "scenes" | "level" => AssetKind::Scene,
            "prefab" | "prefabs" => AssetKind::Prefab,
            "script" | "scripts" | "luau" | "module" | "code" => AssetKind::Script,
            "mesh" | "meshes" | "model" | "models" => AssetKind::Mesh,
            "texture" | "textures" | "image" | "images" => AssetKind::Texture,
            "material" | "materials" => AssetKind::Material,
            "audio" | "sound" | "sounds" | "music" => AssetKind::Audio,
            "animation" | "animations" | "anim" => AssetKind::Animation,
            "font" | "fonts" => AssetKind::Font,
            "data" => AssetKind::Data,
            "other" | "file" => AssetKind::Other,
            _ => return None,
        })
    }

    /// Whether the engine can load and use files of this kind in a game today. (The browser lists the
    /// others too and says so when asked to use one.)
    pub fn usable_by_engine(name: &str) -> bool {
        match AssetKind::of(name) {
            AssetKind::Mesh => crate::assets::mesh::is_loadable(name),
            AssetKind::Texture => crate::assets::texture::is_decodable(name.rsplit('.').next().unwrap_or("")),
            AssetKind::Audio => crate::audio::clip::is_audio(name),
            AssetKind::Scene | AssetKind::Prefab | AssetKind::Script | AssetKind::Material => true,
            _ => false,
        }
    }

    /// Why the engine cannot use a file of this kind, if it cannot ("" when it can).
    pub fn unusable_reason(name: &str) -> String {
        if AssetKind::usable_by_engine(name) {
            return String::new();
        }
        if let Some(why) = crate::assets::mesh::unsupported_reason(name) {
            return why;
        }
        match AssetKind::of(name) {
            AssetKind::Mesh => format!("{name} is a model format Forge cannot read: it reads {}.", crate::assets::mesh::MESH_EXTENSIONS.join(", ")),
            AssetKind::Texture => format!("{name} is a picture format Forge cannot read: it reads png, jpg, bmp, tga, gif, webp, tif and hdr."),
            AssetKind::Audio => format!("{name} is a sound format Forge cannot read: it reads {}.", crate::audio::clip::AUDIO_EXTENSIONS.join(", ")),
            other => format!("Nothing in Forge uses {} files.", other.label().to_lowercase()),
        }
    }

    /// Files the engine reads the contents of when the browser needs a picture of them.
    pub fn has_thumbnail(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        AssetKind::of(name) == AssetKind::Texture && [".png", ".jpg", ".jpeg", ".bmp", ".gif", ".tga", ".webp"].iter().any(|e| lower.ends_with(e))
    }
}

/// The file name without its extension (`Spin.luau` -> `Spin`, `Level.scene.json` -> `Level`).
pub fn stem_of(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(LEGACY_SCENE_EXT) {
        return &name[..name.len() - LEGACY_SCENE_EXT.len()];
    }
    match name.rfind('.') {
        Some(0) | None => name,
        Some(i) => &name[..i],
    }
}

/// The last component of a project-relative path.
pub fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// The folder part of a project-relative path (`""` for the project root).
pub fn parent_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Joins a folder and a name (`""` is the project root).
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_the_file_name() {
        assert_eq!(AssetKind::of("Spin.luau"), AssetKind::Script);
        assert_eq!(AssetKind::of("MainScene.scene"), AssetKind::Scene);
        assert_eq!(AssetKind::of("main.scene.json"), AssetKind::Scene);
        assert_eq!(AssetKind::of("Crate.OBJ"), AssetKind::Mesh);
        assert_eq!(AssetKind::of("Brick.png"), AssetKind::Texture);
        assert_eq!(AssetKind::of("Door.prefab"), AssetKind::Prefab);
        assert_eq!(AssetKind::of("noext"), AssetKind::Other);
        assert_eq!(AssetKind::from_word("Models"), Some(AssetKind::Mesh));
        assert!(AssetKind::from_word("zzz").is_none());
        assert!(AssetKind::usable_by_engine("a.obj") && AssetKind::usable_by_engine("a.GLB") && AssetKind::usable_by_engine("a.png") && AssetKind::usable_by_engine("a.wav") && AssetKind::usable_by_engine("a.mat"));
        assert!(!AssetKind::usable_by_engine("a.fbx") && !AssetKind::usable_by_engine("a.dds") && !AssetKind::usable_by_engine("a.ttf"));
        assert!(AssetKind::unusable_reason("a.fbx").contains("glTF") && AssetKind::unusable_reason("a.obj").is_empty());
    }

    #[test]
    fn path_helpers_split_and_join() {
        assert_eq!((stem_of("Spin.luau"), stem_of("a.b.luau"), stem_of("Level.scene.json"), stem_of(".hidden")), ("Spin", "a.b", "Level", ".hidden"));
        assert_eq!((parent_of("Assets/Models/a.obj"), parent_of("a.obj"), file_name("Assets/a.obj")), ("Assets/Models", "", "a.obj"));
        assert_eq!((join("", "a"), join("X/Y", "a")), ("a".to_owned(), "X/Y/a".to_owned()));
    }
}
