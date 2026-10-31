//! DataModel (`game`): the root of the instance tree. Its children are the services.

use crate::core::*;

type R = Result<ScriptValue, String>;

fn get_service(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    match c.dm.service(&name) {
        Some(id) => Ok(ScriptValue::Instance(id)),
        None => Err(format!("'{name}' is not a valid Service name")),
    }
}

/// Like `GetService`, but nil for a service that does not exist.
fn find_service(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    Ok(ScriptValue::opt_instance(c.dm.service(&name)))
}

fn shutdown(c: &mut MethodCall) -> R {
    c.host.quit();
    Ok(ScriptValue::Nil)
}

/// Not part of Roblox: switch to another scene file, by name ("Level2") or by path ("Scenes/Level2.scene").
/// The host looks the name up in the project (see `GameSource::resolve_scene`).
fn load_place(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    c.host.load_scene(name);
    Ok(ScriptValue::Nil)
}

/// A Forge game is loaded before its scripts run.
fn is_loaded(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Bool(true))
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("GetService", get_service).desc("Returns a service by name."),
    MethodDesc::new("FindService", find_service).desc("Returns a service by name, or nil if there is none."),
    MethodDesc::new("IsLoaded", is_loaded).desc("Whether the game has loaded (it always has when scripts run)."),
    MethodDesc::new("Shutdown", shutdown).desc("Closes the game."),
    MethodDesc::new("LoadPlace", load_place).desc("Switches to another scene file."),
];

// The numbers that say which Roblox place this is: a Forge game is not published, so they are 0 (and the job id empty).
static PROPS: &[PropMeta] = &[
    PropMeta::integer("PlaceId", 0).readonly().hidden().desc("The Roblox place number (0: a Forge game has none)."),
    PropMeta::integer("GameId", 0).readonly().hidden().desc("The Roblox universe number (0: a Forge game has none)."),
    PropMeta::integer("PlaceVersion", 0).readonly().hidden(),
    PropMeta::integer("CreatorId", 0).readonly().hidden(),
    PropMeta::string("JobId", "").readonly().hidden().desc("The id of the server (empty: there is one game and no server)."),
];

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("Loaded").desc("The game finished loading (never fired: it has before scripts run).")];

pub static CLASS: ClassDesc = ClassDesc::new("DataModel", Some("Instance"))
    .describe("The root of the game; its children are the services.")
    .root()
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS)
    .unsupported(&["GetObjects", "HttpGetAsync", "HttpPostAsync", "OpenScreenshotsFolder"])
    .luau(include_str!("data_model.luau"));