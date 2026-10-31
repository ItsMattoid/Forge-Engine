//! Tags (`CollectionService`): names an instance can carry, and the signals that tell scripts when one is given
//! or taken. They live in the data model, not in the scripting VM, so they follow an instance for as long as it exists.

use super::DataModel;
use crate::core::{Id, ScriptValue};

impl DataModel {
    /// Gives the instance a tag. Scripts watching that tag (`GetInstanceAddedSignal`) hear about it.
    pub fn add_tag(&mut self, id: Id, tag: &str) {
        if !self.exists(id) {
            return;
        }
        if self.tags.entry(tag.to_owned()).or_default().insert(id) {
            self.announce_tag(id, tag, "TagAdded");
        }
    }

    pub fn remove_tag(&mut self, id: Id, tag: &str) {
        let removed = self.tags.get_mut(tag).is_some_and(|set| set.remove(&id));
        if removed {
            self.announce_tag(id, tag, "TagRemoved");
        }
    }

    pub fn has_tag(&self, id: Id, tag: &str) -> bool {
        self.tags.get(tag).is_some_and(|set| set.contains(&id))
    }

    /// The tags of one instance, by name.
    pub fn tags_of(&self, id: Id) -> Vec<String> {
        let mut names: Vec<String> = self.tags.iter().filter(|(_, set)| set.contains(&id)).map(|(name, _)| name.clone()).collect();
        names.sort();
        names
    }

    /// Every tag that is on at least one instance, by name.
    pub fn all_tags(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tags.iter().filter(|(_, set)| !set.is_empty()).map(|(name, _)| name.clone()).collect();
        names.sort();
        names
    }

    /// The instances with the tag (those that still exist), in id order.
    pub fn tagged(&self, tag: &str) -> Vec<Id> {
        self.tags.get(tag).map(|set| set.iter().copied().filter(|i| self.exists(*i)).collect()).unwrap_or_default()
    }

    /// Takes every tag off an instance that is being destroyed, telling the scripts that watch them.
    pub(super) fn drop_tags(&mut self, id: Id) {
        for tag in self.tags_of(id) {
            self.remove_tag(id, &tag);
        }
    }

    fn announce_tag(&mut self, id: Id, tag: &str, what: &str) {
        let Some(service) = self.service("CollectionService") else { return };
        self.fire(service, &format!("{what}:{tag}"), || vec![ScriptValue::Instance(id)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_added_listed_removed_and_dropped_with_their_instance() {
        let mut dm = DataModel::new();
        let (a, b) = (dm.create("Part").unwrap(), dm.create("Part").unwrap());
        dm.add_tag(a, "Coin");
        dm.add_tag(a, "Coin");
        dm.add_tag(a, "Hazard");
        dm.add_tag(b, "Coin");
        assert_eq!(dm.tags_of(a), ["Coin", "Hazard"]);
        assert_eq!(dm.tagged("Coin"), [a, b]);
        assert_eq!(dm.all_tags(), ["Coin", "Hazard"]);
        assert!(dm.has_tag(b, "Coin") && !dm.has_tag(b, "Hazard"));
        dm.remove_tag(a, "Coin");
        assert_eq!(dm.tagged("Coin"), [b]);
        dm.destroy(b);
        assert!(dm.tagged("Coin").is_empty() && dm.all_tags() == ["Hazard"], "a destroyed instance loses its tags");
    }

    #[test]
    fn a_tag_signal_is_only_built_when_somebody_listens() {
        let mut dm = DataModel::new();
        let service = dm.service("CollectionService").unwrap();
        let part = dm.create("Part").unwrap();
        dm.add_tag(part, "Quiet");
        assert!(dm.events.is_empty(), "nobody listens to the service");
        dm.watch(dm.get(service).unwrap().uid);
        dm.add_tag(part, "Loud");
        dm.remove_tag(part, "Loud");
        let names: Vec<String> = dm.events.iter().filter_map(|e| if let crate::dm::Event::Signal(_, name, _) = e { Some(name.clone()) } else { None }).collect();
        assert_eq!(names, ["TagAdded:Loud", "TagRemoved:Loud"]);
    }
}
