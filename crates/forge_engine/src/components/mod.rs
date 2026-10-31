//! Reusable components: bundles of properties and systems that several classes
//! compose.
//!
//! A component file exports `pub static COMPONENT: ComponentDesc`. Classes list the
//! components they use (`ClassDesc::components(&["Transform", "Collider"])`); the
//! engine's systems then run each component's hooks (render, physics, collider,
//! on-changed, editor gizmo) for every instance that has it.
//!
//! Only put a component here when more than one class uses it, or when it is a
//! building block other components depend on (`Transform`). Behaviour that belongs
//! to a single class - a light's Bevy renderer, the humanoid's character
//! controller - lives in that class's file instead.
//!
//! | component      | provides                                                       |
//! |----------------|----------------------------------------------------------------|
//! | `Transform`    | `CFrame` / `Position` / `Orientation`                          |
//! | `Appearance`   | `Color` / `Material` / `Transparency` / `CastShadow`           |
//! | `Collider`     | `CanCollide` / `CanTouch` / `CanQuery` / `CollisionGroup`, the collision volume |
//! | `Rigidbody`    | the physical properties of a body: `Anchored`, `Density`, `Friction` ... |
//! | `Joint`        | what every constraint has: `Part0` / `Part1` / `Enabled`                    |
//! | `JointFrames`  | where a constraint joins its parts: `Anchor0` / `Rotation0` / `C0` ...      |
//! | `ValueObject`  | marks `*Value` instances                                       |
//! | `LuaSource`    | marks instances with Luau source                               |
//! | `ScriptRunner` | marks instances the script host runs                           |

include!(concat!(env!("OUT_DIR"), "/components_generated.rs"));
