//! Optional per-class hooks. Most classes need none: data (properties, defaults,
//! components, methods, signals) is declared in the class's `ClassDesc`.

use super::{meta::PropMeta, value::Id};
use crate::dm::DataModel;

pub trait ClassBehavior: Sync + Send {
    /// Right after an instance of the class was created.
    fn on_created(&self, _dm: &mut DataModel, _id: Id) {}
    /// After a scene was loaded (called for services).
    fn on_loaded(&self, _dm: &mut DataModel, _id: Id) {}
    /// After a property of an instance of this class changed.
    fn on_prop_changed(&self, _dm: &mut DataModel, _id: Id, _prop: &PropMeta) {}
    /// After an instance of this class was moved from under `old` to under `new` (either may be none). Only the
    /// instance that was given a new parent is reported, not the ones inside it.
    fn on_parent_changed(&self, _dm: &mut DataModel, _id: Id, _old: Option<Id>, _new: Option<Id>) {}
    /// Just before an instance of this class is destroyed (it and its parent still exist).
    fn on_destroying(&self, _dm: &mut DataModel, _id: Id) {}
    /// When the game starts running (the scripting host calls it once for every service, right after the scripts
    /// of the scene were started; and again if the scene is replaced).
    fn on_started(&self, _dm: &mut DataModel, _id: Id) {}
}

pub struct NoBehavior;
impl ClassBehavior for NoBehavior {}

pub const NO_BEHAVIOR: &dyn ClassBehavior = &NoBehavior;
