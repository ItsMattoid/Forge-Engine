//! Registry-level tests: the whole class tree must resolve consistently.

use super::*;
use crate::{
    core::{value_matches, Cond, Value},
    dm::DataModel,
};

fn c(name: &str) -> &'static ResolvedClass {
    class(name).unwrap_or_else(|| panic!("class {name} is not registered"))
}

#[test]
fn every_class_resolves_with_a_consistent_description() {
    let reg = registry();
    assert!(reg.classes().len() > 20);
    for class in reg.classes() {
        let mut seen = std::collections::HashSet::new();
        for rp in &class.props {
            assert!(seen.insert(rp.meta.name), "{}: duplicate property {}", class.name, rp.meta.name);
            if rp.meta.getter.is_none() {
                assert!(value_matches(rp.meta.ty, &rp.meta.default.to_value()), "{}.{}: default does not match the property type", class.name, rp.meta.name);
            }
            if let Some(cond) = rp.meta.visible_if {
                let (Cond::Eq(p, _) | Cond::Ne(p, _) | Cond::True(p) | Cond::False(p)) = cond;
                assert!(class.prop(p).is_some(), "{}.{}: visible_if refers to unknown property {p}", class.name, rp.meta.name);
            }
        }
        if class.desc.creatable {
            assert!(!class.desc.description.is_empty(), "{} needs a description (shown in the Insert menu)", class.name);
        }
    }
}

#[test]
fn inheritance_and_property_ownership() {
    let part = c("Part");
    let chain: Vec<_> = part.chain.iter().map(|d| d.name).collect();
    assert_eq!(chain, ["Part", "BasePart", "WorldObject", "PVInstance", "Instance", "Object"]);
    assert!(c("MeshPart").is_a("BasePart") && c("PointLight").is_a("Light") && c("AreaLight").is_a("WorldObject"));

    let point = c("PointLight");
    assert_eq!(point.prop("Color").unwrap().owner, "Light", "shared light properties come from Light");
    assert_eq!(point.prop("Range").unwrap().owner, "PointLight");
    assert!(point.prop("InnerAngle").is_none());
    let spot = c("SpotLight");
    assert!(spot.prop("InnerAngle").is_some() && spot.prop("OuterAngle").is_some() && spot.prop("Intensity").is_some());
    assert!(c("AreaLight").prop("Width").is_some());

    // Properties contributed by components are inherited like any other.
    assert_eq!(c("MeshPart").prop("Anchored").unwrap().owner, "Rigidbody");
    assert!(c("Part").has_component("Collider") && c("MeshPart").has_component("MeshRenderer") && !c("MeshPart").has_component("PrimitiveRenderer"));
}

#[test]
fn inspector_order_follows_group_and_order() {
    let point = c("PointLight");
    let index = |name: &str| point.props.iter().position(|p| p.meta.name == name).unwrap();
    assert!(index("Color") < index("Intensity") && index("Intensity") < index("Range"));
    let groups = point.groups();
    assert_eq!(groups[0], "General", "General (Name, Parent) comes first");
    assert!(groups.iter().position(|g| *g == "Transform") < groups.iter().position(|g| *g == "Light"));
}

/// The Properties panel's sections and the properties in them, in order (hidden ones left out).
fn sections(class: &str) -> Vec<(&'static str, Vec<&'static str>)> {
    let class = c(class);
    class
        .groups()
        .into_iter()
        .map(|g| (g, class.props.iter().filter(|p| p.meta.visible && p.meta.display_group() == g).map(|p| p.meta.display_label()).collect::<Vec<_>>()))
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

#[test]
fn properties_are_split_into_sections_in_a_fixed_order() {
    let general = ("General", vec!["Name", "Parent"]);
    assert_eq!(sections("Folder"), [general.clone()]);

    let base_part = vec![
        general.clone(),
        ("Transform", vec!["Position", "Rotation", "Size", "PivotOffset"]),
        ("Appearance", vec!["Color", "Transparency", "Reflectance", "Material", "Material Asset", "Texture", "CastShadow"]),
        ("Physics", vec!["Anchored", "Kinematic", "Density", "Mass", "Friction", "Elasticity", "AssemblyLinearVelocity", "AssemblyAngularVelocity", "GravityScale", "LinearDamping", "AngularDamping", "CanSleep", "Sleeping", "ContinuousCollision", "FixedRotation"]),
        ("Collision", vec!["CanCollide", "CanTouch", "CanQuery", "CollisionGroup"]),
    ];
    assert_eq!(sections("Part"), [base_part.clone(), vec![("Geometry", vec!["Shape"])]].concat());
    assert_eq!(sections("MeshPart"), [base_part, vec![("Geometry", vec!["Mesh", "MeshSize", "UseImportedMaterials"])]].concat());

    assert_eq!(sections("Model"), [general.clone(), ("Transform", vec!["Position", "Rotation", "Scale", "PivotOffset"]), ("Model", vec!["PrimaryPart"])]);

    let light = vec![
        general,
        ("Transform", vec!["Position", "Rotation", "PivotOffset"]),
        ("Light", vec!["Enabled", "Color", "Brightness", "Range"]),
        ("Shadows", vec!["Shadows"]),
    ];
    assert_eq!(sections("PointLight"), light);
    assert_eq!(sections("SpotLight"), [light.clone(), vec![("Spotlight", vec!["Angle", "InnerAngle", "Face"])]].concat());
    assert_eq!(sections("AreaLight"), [light, vec![("Area", vec!["Width", "Height"])]].concat());
}

#[test]
fn a_model_moves_and_turns_everything_inside_it_through_its_properties() {
    use bevy::math::Vec3;
    let mut dm = DataModel::new();
    let ws = dm.workspace;
    let model = dm.create("Model").unwrap();
    dm.set_parent(model, Some(ws)).unwrap();
    let part = dm.create("Part").unwrap();
    dm.set_prop(part, "Position", Value::V3(Vec3::new(1.0, 0.0, 0.0))).unwrap();
    dm.set_parent(part, Some(model)).unwrap();

    dm.set_prop(model, "Position", Value::V3(Vec3::new(11.0, 0.0, 0.0))).unwrap();
    assert_eq!(dm.get_prop(model, "Position"), Some(Value::V3(Vec3::new(11.0, 0.0, 0.0))));
    assert!((dm.vec3(part, "Position") - Vec3::new(11.0, 0.0, 0.0)).length() < 1e-4, "the part went with the model");
    assert!(!c("Model").prop("Position").unwrap().meta.saved(), "a model's pivot is derived, never written to scene files");
}

#[test]
fn old_property_names_still_work() {
    let mut dm = DataModel::new();
    let light = dm.create("PointLight").unwrap();
    dm.set_prop(light, "Brightness", Value::Num(3.0)).unwrap();
    assert_eq!(dm.get_prop(light, "Intensity"), Some(Value::Num(3.0)));
    dm.set_prop(light, "Shadows", Value::Bool(true)).unwrap();
    assert!(dm.flag(light, "CastShadows"));
    let spot = dm.create("SpotLight").unwrap();
    dm.set_prop(spot, "Angle", Value::Num(60.0)).unwrap();
    assert_eq!(dm.get_prop(spot, "OuterAngle"), Some(Value::Num(60.0)));
}

#[test]
fn services_are_created_from_class_flags() {
    let dm = DataModel::new();
    for svc in registry().services() {
        assert!(dm.service(svc.name).is_some(), "service {} was not created", svc.name);
    }
    assert!(dm.get(dm.workspace).is_some_and(|i| i.class.name == "Workspace"));
    assert!(dm.reference(dm.workspace, "CurrentCamera").is_some(), "the workspace always has a camera");
    let players = dm.service("Players").unwrap();
    assert!(dm.get_prop(players, "LocalPlayer").is_some_and(|v| matches!(v, Value::Ref(Some(_)))));
}

#[test]
fn computed_properties_and_components_work_through_the_generic_path() {
    let mut dm = DataModel::new();
    let part = dm.create("Part").unwrap();
    dm.set_prop(part, "Position", Value::V3(bevy::math::Vec3::new(1.0, 2.0, 3.0))).unwrap();
    assert_eq!(dm.get_prop(part, "Name"), Some(Value::Str("Part".into())));
    assert_eq!(dm.get_prop(part, "ClassName"), Some(Value::Str("Part".into())));
    assert!(dm.set_prop(part, "ClassName", Value::Str("X".into())).is_err(), "read-only");
    assert!(dm.with_component("Collider").contains(&part));
    dm.destroy(part);
    assert!(!dm.with_component("Collider").contains(&part), "component index is cleaned up");
}

#[test]
fn lights_attach_to_their_parent_part() {
    use bevy::math::Vec3;
    let mut dm = DataModel::new();
    let ws = dm.workspace;
    let part = dm.create("Part").unwrap();
    dm.set_prop(part, "Position", Value::V3(Vec3::new(10.0, 0.0, 0.0))).unwrap();
    dm.set_parent(part, Some(ws)).unwrap();
    let light = dm.create("PointLight").unwrap();
    dm.set_prop(light, "Position", Value::V3(Vec3::new(0.0, 2.0, 0.0))).unwrap();
    dm.set_parent(light, Some(part)).unwrap();
    assert_eq!(dm.world_cframe(light).unwrap().0, Vec3::new(10.0, 2.0, 0.0));
    dm.set_prop(part, "Position", Value::V3(Vec3::new(0.0, 0.0, 5.0))).unwrap();
    assert_eq!(dm.world_cframe(light).unwrap().0, Vec3::new(0.0, 2.0, 5.0), "the light follows its part");
}

#[test]
fn each_light_type_owns_its_renderer_and_range() {
    for (class_name, emitter) in [("PointLight", "PointLightEmitter"), ("SpotLight", "SpotLightEmitter"), ("AreaLight", "AreaLightEmitter"), ("DirectionalLight", "DirectionalLightEmitter")] {
        let class = c(class_name);
        assert!(class.is_a("Light"), "{class_name} is a Light");
        let component = class.components.iter().find(|comp| comp.name == emitter).unwrap_or_else(|| panic!("{class_name} should use {emitter}"));
        assert!(component.render.is_some(), "{emitter} renders the light");
        // Colour, intensity, shadows and the on/off switch come from Light for every type.
        for shared in ["Color", "Intensity", "CastShadows", "Enabled"] {
            assert_eq!(class.prop(shared).unwrap().owner, "Light", "{class_name}.{shared}");
        }
    }
    // Range is opt-in: the lights that fade with distance have it, the sun-like one does not.
    for class_name in ["PointLight", "SpotLight", "AreaLight"] {
        assert!(c(class_name).prop("Range").is_some(), "{class_name} has a Range");
    }
    assert!(c("DirectionalLight").prop("Range").is_none());
}

#[test]
fn parts_and_characters_own_their_behaviour() {
    assert!(c("Part").has_component("PrimitiveRenderer") && !c("Part").has_component("MeshRenderer"));
    assert!(c("MeshPart").has_component("MeshRenderer"));
    assert!(c("Camera").has_component("CameraView"));
    assert!(c("Lighting").has_component("Environment"));
    // The rigidbody component declares the physical properties a script-driven player body needs.
    assert_eq!(c("Part").prop("FixedRotation").unwrap().owner, "Rigidbody");
}

#[test]
fn the_built_in_player_classes_are_not_registered_and_nothing_names_them() {
    // Player controllers are scripts: the classes that made up the engine's own player architecture are gone.
    for gone in ["Humanoid", "Accessory", "Backpack", "Team", "Tool", "StarterGear", "Teams", "StarterPack", "ForceField"] {
        assert!(crate::core::class(gone).is_none(), "{gone} must not be a class");
    }
    for class in crate::core::registry().classes() {
        assert!(!class.has_component("CharacterController") && !class.has_component("PlayerRespawn"), "{} still has a built-in character component", class.name);
        for rp in &class.props {
            if let crate::core::PropType::Ref(want) = rp.meta.ty {
                assert!(!["Humanoid", "Tool", "Team"].contains(&want), "{}.{} points at a removed class", class.name, rp.meta.name);
            }
        }
    }
}

#[test]
fn conditional_visibility_follows_other_properties() {
    let mut dm = DataModel::new();
    let part = dm.create("Part").unwrap();
    let shown_when_anchored = Cond::True("Anchored");
    assert!(!shown_when_anchored.evaluate(&dm, part));
    dm.set_prop(part, "Anchored", Value::Bool(true)).unwrap();
    assert!(shown_when_anchored.evaluate(&dm, part));
    assert!(Cond::Eq("Shape", crate::core::D::E("Block")).evaluate(&dm, part));
    assert!(Cond::Ne("Shape", crate::core::D::E("Ball")).evaluate(&dm, part));
}

#[test]
fn every_enum_a_property_refers_to_is_registered() {
    for class in registry().classes() {
        for rp in &class.props {
            if let crate::core::PropType::Enum(name) = rp.meta.ty {
                assert!(!enum_names(name).is_empty(), "{}.{} uses the unregistered enum {name}", class.name, rp.meta.name);
            }
        }
    }
}
