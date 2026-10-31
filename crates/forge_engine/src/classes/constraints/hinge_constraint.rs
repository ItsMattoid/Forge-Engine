//! HingeConstraint: the two parts turn about one axis (the X axis of the joint's frame), like a door on its hinge.
//! With the limits on the angle between them stays within `LowerAngle` .. `UpperAngle`; with the motor on it is
//! driven at `MotorSpeed` (radians per second), with at most `MotorMaxTorque`.

use crate::{core::*, dm::DataModel};

/// Roblox' `ActuatorType`: None, or Motor (the motor is on). Servo is not supported and is read as Motor.
fn get_actuator(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Enum(if dm.flag(id, "MotorEnabled") { "Motor" } else { "None" }.to_owned()))
}

fn set_actuator(dm: &mut DataModel, id: Id, v: Value) -> Result<(), String> {
    let Value::Enum(kind) = v else { return Err("ActuatorType must be an Enum.ActuatorType".into()) };
    dm.set_prop(id, "MotorEnabled", Value::Bool(kind != "None"))
}

static PROPS: &[PropMeta] = &[
    PropMeta::boolean("LimitsEnabled", false).group("Hinge").order(0).flags(F_PHYS).desc("Whether the angle between the parts is limited."),
    PropMeta::number("LowerAngle", -45.0).group("Hinge").order(1).flags(F_PHYS).slider(-178.0, 178.0).when_true("LimitsEnabled").desc("The smallest angle, in degrees."),
    PropMeta::number("UpperAngle", 45.0).group("Hinge").order(2).flags(F_PHYS).slider(-178.0, 178.0).when_true("LimitsEnabled").desc("The largest angle, in degrees."),
    PropMeta::boolean("MotorEnabled", false).group("Hinge").order(3).flags(F_PHYS).desc("Whether a motor turns the hinge."),
    PropMeta::enumeration("ActuatorType", "ActuatorType", "None").group("Hinge").order(6).no_save().hidden().desc("Roblox's way to say MotorEnabled: None or Motor.").computed(get_actuator, set_actuator),
    PropMeta::number("MotorSpeed", 0.0)
        .group("Hinge")
        .order(4)
        .flags(F_PHYS)
        .drag(0.1)
        .clamp(-10_000.0, 10_000.0)
        .aliases(&["AngularVelocity"])
        .when_true("MotorEnabled")
        .desc("The speed the motor aims for, in radians per second."),
    PropMeta::number("MotorMaxTorque", 10_000.0).group("Hinge").order(5).flags(F_PHYS).drag(10.0).clamp(0.0, 1.0e9).when_true("MotorEnabled").desc("The most torque the motor may use."),
];

pub static CLASS: ClassDesc = ClassDesc::new("HingeConstraint", Some("Constraint"))
    .describe("Lets two parts turn about one axis, like a door.")
    .category("Constraints")
    .creatable()
    .groups(&["Hinge"])
    .components(&["JointFrames"])
    .props(PROPS);
