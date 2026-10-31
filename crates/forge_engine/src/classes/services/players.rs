//! Players: the players of the game. `Players.LocalPlayer` is the local player.
//!
//! There is one player. It exists from the start (so the Explorer shows it and scripts can configure it), but
//! "joins" when the game starts, just after the scripts of the scene have begun: `GetPlayers()` is empty until then,
//! and `PlayerAdded` fires for it - so a script written as Roblox scripts are (`PlayerAdded:Connect(...)`, plus a loop
//! over `GetPlayers()` for the players already in) handles the player once.
//!
//! The engine gives the player **no character**: making one, moving it and respawning it is what the game's own scripts do
//! (they set `Player.Character` to a model of their own; `CharacterAdded` / `CharacterRemoving` follow).

use crate::{
    core::*,
    dm::DataModel,
};

type R = Result<ScriptValue, String>;

fn local_player(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Ref(dm.find_by_class(id, "Player", false, true)))
}

fn num_players(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Num(joined(dm, id).len() as f32))
}

/// The Player objects (children of the Players service), joined or not.
fn all_players(dm: &DataModel) -> Vec<Id> {
    dm.service("Players").map(|svc| dm.children(svc).into_iter().filter(|p| dm.is_a(*p, "Player")).collect()).unwrap_or_default()
}

/// The players that have joined.
fn joined(dm: &DataModel, service: Id) -> Vec<Id> {
    dm.children(service).into_iter().filter(|p| dm.is_a(*p, "Player") && dm.flag(*p, "Joined")).collect()
}

static PROPS: &[PropMeta] = &[
    PropMeta::reference("LocalPlayer", "Player").group("Players").no_save().desc("The player on this computer.").computed_ro(local_player),
    PropMeta::number("NumPlayers", 0.0).group("Players").no_save().desc("How many players are in the game.").computed_ro(num_players),
    PropMeta::integer("MaxPlayers", 1).group("Players").hidden().readonly().desc("How many players the game holds."),
    PropMeta::integer("PreferredPlayers", 1).group("Players").hidden().readonly(),
];

// ---- methods ------------------------------------------------------------------------------------

fn get_players(c: &mut MethodCall) -> R {
    Ok(ScriptValue::instances(joined(c.dm, c.this)))
}

fn get_player_from_character(c: &mut MethodCall) -> R {
    let model = c.opt_instance(0)?;
    Ok(ScriptValue::opt_instance(model.and_then(|m| joined(c.dm, c.this).into_iter().find(|p| c.dm.reference(*p, "Character") == Some(m)))))
}

fn get_player_by_user_id(c: &mut MethodCall) -> R {
    let wanted = c.number(0)? as f32;
    Ok(ScriptValue::opt_instance(joined(c.dm, c.this).into_iter().find(|p| c.dm.num(*p, "UserId", 0.0) == wanted)))
}

fn get_player_by_name(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    Ok(ScriptValue::opt_instance(joined(c.dm, c.this).into_iter().find(|p| c.dm.name(*p).eq_ignore_ascii_case(&name))))
}

/// Only the local player's id is known; any other name or id is not a user of this game.
fn get_user_id_from_name(c: &mut MethodCall) -> R {
    let name = c.string(0)?;
    match all_players(c.dm).into_iter().find(|p| c.dm.name(*p).eq_ignore_ascii_case(&name)) {
        Some(p) => Ok(ScriptValue::Num(f64::from(c.dm.num(p, "UserId", 0.0)))),
        None => Err(format!("{name} is not a known user (Forge has no user accounts)")),
    }
}

fn get_name_from_user_id(c: &mut MethodCall) -> R {
    let id = c.number(0)? as f32;
    match all_players(c.dm).into_iter().find(|p| c.dm.num(*p, "UserId", 0.0) == id) {
        Some(p) => Ok(ScriptValue::Str(c.dm.name(p))),
        None => Err(format!("{id} is not a known user id (Forge has no user accounts)")),
    }
}

fn get_friends(_c: &mut MethodCall) -> R {
    unsupported("Players:GetFriendsAsync")
}

fn get_appearance(_c: &mut MethodCall) -> R {
    unsupported("Players:GetCharacterAppearanceInfoAsync")
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("GetPlayers", get_players).desc("The players in the game."),
    MethodDesc::new("GetPlayerFromCharacter", get_player_from_character).desc("The player that controls a character model, or nil."),
    MethodDesc::new("GetPlayerByUserId", get_player_by_user_id).desc("The player with this UserId, or nil."),
    MethodDesc::new("GetPlayerByName", get_player_by_name).desc("The player with this name, or nil."),
    MethodDesc::new("GetUserIdFromNameAsync", get_user_id_from_name).desc("The UserId of the local player (other users are unknown)."),
    MethodDesc::new("GetNameFromUserIdAsync", get_name_from_user_id).desc("The name of the local player (other users are unknown)."),
    MethodDesc::new("GetFriendsAsync", get_friends).desc("Not supported (needs Roblox's friends service)."),
    MethodDesc::new("GetCharacterAppearanceInfoAsync", get_appearance).desc("Not supported (needs Roblox's avatar service)."),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("PlayerAdded").desc("A player joined; passes the player."),
    SignalDesc::new("PlayerRemoving").desc("A player is leaving; passes the player."),
    SignalDesc::new("PlayerMembershipChanged").desc("A player's membership changed (never fired)."),
];

// ---- behaviour ----------------------------------------------------------------------------------

/// There is always exactly one (local) player; it joins when the game starts.
struct PlayersBehavior;

impl ClassBehavior for PlayersBehavior {
    fn on_created(&self, dm: &mut DataModel, id: Id) {
        if let Some(class) = class("Player") {
            let player = dm.spawn_class(class);
            dm.set_name(player, "Player1");
            let _ = dm.set_parent(player, Some(id));
        }
    }

    fn on_started(&self, dm: &mut DataModel, id: Id) {
        let waiting: Vec<Id> = dm.children(id).into_iter().filter(|p| dm.is_a(*p, "Player") && !dm.flag(*p, "Joined")).collect();
        for player in waiting {
            let _ = dm.set_prop(player, "Joined", Value::Bool(true));
            dm.fire(id, "PlayerAdded", || vec![ScriptValue::Instance(player)]);
        }
    }
}

pub static CLASS: ClassDesc = ClassDesc::new("Players", Some("Instance"))
    .describe("The players of the game.")
    .service(1)
    .groups(&["Players"])
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS)
    .behavior(&PlayersBehavior);
