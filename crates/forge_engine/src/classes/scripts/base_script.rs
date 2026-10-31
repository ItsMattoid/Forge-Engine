//! BaseScript: a LuaSourceContainer that the game runs by itself.
//!
//! `Enabled` is Roblox's switch for a script; `Disabled` (the older name) is the same switch the other way round.

use crate::{core::*, dm::DataModel};

fn get_disabled(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Bool(!dm.flag(id, "Enabled")))
}

fn set_disabled(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Bool(off) = v else { return Err("Disabled must be a boolean".into()) };
    dm.set_prop(id, "Enabled", Value::Bool(!off))
}

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("Enabled", true).group("Script").order(1).flags(F_SCRIPT).desc("Scripts that are disabled do not run."),
    PropMeta::boolean("Disabled", false).group("Script").order(2).no_save().hidden().desc("The opposite of Enabled (the older name).").computed(get_disabled, set_disabled),
];

pub static CLASS: ClassDesc = ClassDesc::new("BaseScript", Some("LuaSourceContainer"))
    .describe("A script that runs on its own.")
    .components(&["ScriptRunner"])
    .props(PROPS);