//! Object: the root of every class. Anything that can have properties, methods and
//! a `Changed` signal.

use crate::core::*;

fn class_name(dm: &crate::dm::DataModel, id: Id) -> Option<Value> {
    Some(Value::Str(dm.class_name(id).to_owned()))
}

fn is_a(c: &mut MethodCall) -> Result<ScriptValue, String> {
    let class = c.string(0)?;
    Ok(ScriptValue::Bool(c.dm.is_a(c.this, &class)))
}

// Scripts read it; the Properties panel names the class in its heading instead.
static PROPS: &[PropMeta] = &[PropMeta::string("ClassName", "").hidden().no_save().desc("The kind of object this is.").computed_ro(class_name)];

static METHODS: &[MethodDesc] = &[MethodDesc::new("IsA", is_a).desc("True if this object is of the given class or inherits from it.")];

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("Changed").desc("Fires after a property changes; passes the property name.")];

pub static CLASS: ClassDesc = ClassDesc::new("Object", None)
    .describe("The root of all classes.")
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS);
