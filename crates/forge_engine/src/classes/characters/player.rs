//! Player: the local player (created by the Players service; there is one).
//!
//! The player has a `Character` (the model it controls: a script sets it), the containers every player gets
//! (`PlayerGui`, `PlayerScripts`), and the usual signals: `CharacterAdded` and `CharacterRemoving` fire whenever
//! `Character` changes. The engine makes no character and moves none: a player controller is a script (see the
//! starter project's `Client/Controllers/FirstPersonController.luau`).
//!
//! Account things (friends, groups, premium, the account's age) are answered as for a new guest account.

use crate::{
    core::{unsupported, ClassBehavior, ClassDesc, EnumDesc, Id, MethodCall, MethodDesc, PropMeta, ScriptValue, SignalDesc, Value},
    dm::DataModel,
};

type R = Result<ScriptValue, String>;

// ---- properties ---------------------------------------------------------------------------------

fn get_display_name(dm: &DataModel, id: Id) -> Option<Value> {
    let custom = dm.string(id, "DisplayNameOverride");
    Some(Value::Str(if custom.is_empty() { dm.name(id) } else { custom }))
}

fn set_display_name(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    dm.set_prop(id, "DisplayNameOverride", v)
}

static PROPS: &[PropMeta] = &[
    PropMeta::reference("Character", "Model").group("Player").order(0).runtime().desc("The model this player controls."),
    PropMeta::reference("LastCharacter", "Model").runtime().hidden(),
    PropMeta::boolean("Joined", false).runtime().hidden().desc("Whether the player has joined the game (they join as it starts)."),
    PropMeta::integer("UserId", 1).group("Player").order(1).desc("The player's account number."),
    PropMeta::string("DisplayName", "").group("Player").order(2).desc("The name shown for the player (its Name when not set).").computed(get_display_name, set_display_name),
    PropMeta::string("DisplayNameOverride", "").hidden(),
    PropMeta::integer("AccountAge", 0).group("Account").order(0).readonly().desc("Days since the account was made (always 0: there is no account)."),
    PropMeta::enumeration("MembershipType", "MembershipType", "None").group("Account").order(1).readonly().hidden(),
    PropMeta::boolean("CanLoadCharacterAppearance", true).group("Account").order(2).hidden(),
    PropMeta::integer("CharacterAppearanceId", 0).group("Account").order(3).hidden(),
    PropMeta::integer("FollowUserId", 0).group("Account").order(4).hidden().readonly(),
    PropMeta::string("LocaleId", "en-us").group("Account").order(5).readonly().hidden(),
    PropMeta::enumeration("CameraMode", "CameraMode", "Classic").group("Camera").order(0).hidden(),
    PropMeta::number("CameraMaxZoomDistance", 128.0).group("Camera").order(1).hidden(),
    PropMeta::number("CameraMinZoomDistance", 0.5).group("Camera").order(2).hidden(),
    PropMeta::boolean("GameplayPaused", false).runtime().hidden(),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("CharacterAdded").desc("The player's Character was set to a model; passes it."),
    SignalDesc::new("CharacterRemoving").desc("The player's character is about to go; passes it."),
    SignalDesc::new("CharacterAppearanceLoaded").desc("The character's clothes and accessories were loaded (never fired: there are none)."),
    SignalDesc::new("Chatted").desc("The player said something (never fired: there is no chat)."),
    SignalDesc::new("Idled").desc("The player has not done anything for a while (never fired)."),
];

pub static ENUMS: &[EnumDesc] = &[EnumDesc::new("MembershipType", &["None", "BuildersClub", "TurboBuildersClub", "OutrageousBuildersClub", "Premium"])];

// ---- methods ------------------------------------------------------------------------------------

/// Ends the game: with one player, kicking the player is closing the game.
fn kick(c: &mut MethodCall) -> R {
    c.host.quit();
    Ok(ScriptValue::Nil)
}

fn distance_from_character(c: &mut MethodCall) -> R {
    let point = c.vec3(0)?;
    let root = c.dm.reference(c.this, "Character").and_then(|m| c.dm.reference(m, "PrimaryPart").or_else(|| c.dm.subtree(m).into_iter().find(|p| c.dm.is_a(*p, "BasePart"))));
    Ok(ScriptValue::Num(root.map_or(0.0, |r| f64::from(c.dm.vec3(r, "Position").distance(point)))))
}

fn friends_with(c: &mut MethodCall) -> R {
    c.number(0)?;
    Ok(ScriptValue::Bool(false))
}

fn in_group(c: &mut MethodCall) -> R {
    c.number(0)?;
    Ok(ScriptValue::Bool(false))
}

fn rank_in_group(c: &mut MethodCall) -> R {
    c.number(0)?;
    Ok(ScriptValue::Num(0.0))
}

fn role_in_group(c: &mut MethodCall) -> R {
    c.number(0)?;
    Ok(ScriptValue::Str("Guest".into()))
}

fn true_(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Bool(true))
}

fn nothing(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Nil)
}

fn zero(_c: &mut MethodCall) -> R {
    Ok(ScriptValue::Num(0.0))
}

fn get_friends_online(_c: &mut MethodCall) -> R {
    unsupported("Player:GetFriendsOnline")
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("Kick", kick).desc("Removes the player from the game; with one player that closes the game."),
    MethodDesc::new("DistanceFromCharacter", distance_from_character).desc("How far a point is from the character's primary part (0 when there is no character)."),
    MethodDesc::new("IsFriendsWith", friends_with).desc("Whether the player is friends with a user (always false: there are no accounts)."),
    MethodDesc::new("IsInGroup", in_group).desc("Always false: there are no groups."),
    MethodDesc::new("GetRankInGroup", rank_in_group).desc("Always 0: there are no groups."),
    MethodDesc::new("GetRoleInGroup", role_in_group).desc("Always \"Guest\": there are no groups."),
    MethodDesc::new("HasAppearanceLoaded", true_).desc("Always true: characters have no appearance to load."),
    MethodDesc::new("LoadCharacterAppearance", nothing).desc("Does nothing: characters have no appearance."),
    MethodDesc::new("ClearCharacterAppearance", nothing).desc("Does nothing: characters have no appearance."),
    MethodDesc::new("RequestStreamAroundAsync", nothing).desc("Does nothing: the whole world is always loaded."),
    MethodDesc::new("GetNetworkPing", zero).desc("Always 0: there is no network."),
    MethodDesc::new("GetFriendsOnline", get_friends_online).desc("Not supported (needs Roblox's friends service)."),
];

// ---- behaviour ----------------------------------------------------------------------------------

/// The containers every player has, and `CharacterAdded` / `CharacterRemoving` when `Character` changes.
struct PlayerBehavior;

impl ClassBehavior for PlayerBehavior {
    fn on_created(&self, dm: &mut DataModel, id: Id) {
        for (class, name) in [("PlayerGui", "PlayerGui"), ("PlayerScripts", "PlayerScripts")] {
            if let Some(class) = crate::core::class(class) {
                let child = dm.spawn_class(class);
                dm.set_name(child, name);
                let _ = dm.set_parent(child, Some(id));
            }
        }
    }

    fn on_prop_changed(&self, dm: &mut DataModel, id: Id, prop: &PropMeta) {
        if prop.name != "Character" {
            return;
        }
        let (old, new) = (dm.reference(id, "LastCharacter"), dm.reference(id, "Character"));
        if old == new {
            return;
        }
        if let Some(old) = old {
            dm.fire(id, "CharacterRemoving", || vec![ScriptValue::Instance(old)]);
        }
        let _ = dm.set_prop(id, "LastCharacter", Value::Ref(new));
        if let Some(new) = new {
            dm.fire(id, "CharacterAdded", || vec![ScriptValue::Instance(new)]);
        }
    }

    /// A player that goes (destroyed, or moved out of the Players service) is announced by the service.
    fn on_destroying(&self, dm: &mut DataModel, id: Id) {
        if let Some(character) = dm.reference(id, "Character") {
            dm.fire(id, "CharacterRemoving", || vec![ScriptValue::Instance(character)]);
        }
        if let Some(service) = dm.parent(id).filter(|p| dm.is_a(*p, "Players")) {
            dm.fire(service, "PlayerRemoving", || vec![ScriptValue::Instance(id)]);
        }
    }

    fn on_parent_changed(&self, dm: &mut DataModel, id: Id, old: Option<Id>, new: Option<Id>) {
        let is_service = |dm: &DataModel, p: Option<Id>| p.is_some_and(|p| dm.is_a(p, "Players"));
        if is_service(dm, old) && !is_service(dm, new) {
            if let Some(service) = old {
                dm.fire(service, "PlayerRemoving", || vec![ScriptValue::Instance(id)]);
            }
        }
    }
}

// ---- class --------------------------------------------------------------------------------------

pub static CLASS: ClassDesc = ClassDesc::new("Player", Some("Instance"))
    .describe("A player.")
    .groups(&["Player", "Account", "Camera"])
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS)
    .luau(include_str!("player.luau"))
    .behavior(&PlayerBehavior);
