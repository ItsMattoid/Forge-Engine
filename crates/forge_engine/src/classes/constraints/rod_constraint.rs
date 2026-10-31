//! RodConstraint: keeps the two frames exactly `Length` studs apart (the parts can still turn about the ends).

use crate::core::*;

type R = Result<ScriptValue, String>;

/// How far apart the two frames are right now, in studs (the length a rope, rod or spring starts at).
pub fn current_length(dm: &crate::dm::DataModel, id: Id) -> Option<f32> {
    crate::physics::joints::frames_distance(dm, id)
}

/// `CurrentLength`: [current_length] as a read-only property (rods, ropes and springs have it).
pub fn get_current_length(dm: &crate::dm::DataModel, id: Id) -> Option<Value> {
    current_length(dm, id).map(Value::Num)
}

/// `constraint:UseCurrentLength()`: sets the length to the distance the frames have now.
fn use_current_length(c: &mut MethodCall) -> R {
    let length = current_length(c.dm, c.this).ok_or("the constraint needs a Part0")?;
    let property = if c.dm.class_name(c.this) == "SpringConstraint" { "FreeLength" } else { "Length" };
    c.dm.set_prop(c.this, property, Value::Num(length.max(0.01)))?;
    Ok(ScriptValue::Nil)
}

pub static METHODS: &[MethodDesc] = &[MethodDesc::new("UseCurrentLength", use_current_length).desc("Sets the length to the distance between the two frames right now.")];

static PROPS: &[PropMeta] = &[
    PropMeta::number("Length", 5.0).group("Rod").order(0).flags(F_PHYS).drag(0.1).clamp(0.01, 1.0e6).desc("The distance the two frames are kept at, in studs."),
    PropMeta::number("CurrentLength", 0.0).group("Rod").order(1).no_save().hidden().computed_ro(get_current_length).desc("How far apart the two frames are right now."),
];

pub static CLASS: ClassDesc = ClassDesc::new("RodConstraint", Some("Constraint"))
    .describe("Keeps two parts a fixed distance apart.")
    .category("Constraints")
    .creatable()
    .groups(&["Rod"])
    .components(&["JointFrames"])
    .props(PROPS)
    .methods(METHODS);
