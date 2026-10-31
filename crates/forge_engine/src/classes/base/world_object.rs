//! WorldObject: an instance that can be placed in the 3D world - parts, models,
//! lights and cameras. Provides the shared pivot API (`PivotTo` / `GetPivot`).

use crate::core::*;

type R = Result<ScriptValue, String>;

fn pivot_to(c: &mut MethodCall) -> R {
    let cf = c.cframe(0)?;
    c.dm.pivot_to(c.this, cf);
    Ok(ScriptValue::Nil)
}

fn get_pivot(c: &mut MethodCall) -> R {
    match c.dm.get_pivot(c.this) {
        Some((p, q)) => Ok(ScriptValue::CFrame(p, q)),
        None => Err("this object has no pivot".into()),
    }
}

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("PivotTo", pivot_to).desc("Moves the object (and everything inside it) so its pivot is at the CFrame."),
    MethodDesc::new("GetPivot", get_pivot).desc("The CFrame of the object's pivot."),
];

static PROPS: &[PropMeta] = &[PropMeta::vector3("PivotOffset", 0.0, 0.0, 0.0)
    .group("Transform")
    .order(5)
    .drag(0.1)
    .desc("Where the object turns and moves from, relative to its own centre (in its own axes). The Pivot tool edits it in the viewport.")];

pub static CLASS: ClassDesc = ClassDesc::new("WorldObject", Some("PVInstance"))
    .describe("An object that exists in the 3D world.")
    .groups(&["Transform"])
    .props(PROPS)
    .methods(METHODS);
