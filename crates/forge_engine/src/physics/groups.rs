//! Collision groups: which kinds of parts collide with which.
//!
//! A part belongs to one group (its `CollisionGroup` property); every pair of groups either collides or does not
//! (everything collides by default). Box3D filters with a category and a mask per shape, so group `i` is the
//! category bit `1 << i` and its mask holds the bit of every group it collides with: two shapes collide when each
//! one's mask contains the other's category.
//!
//! The groups are saved with the scene as a small text on `PhysicsService.CollisionGroups`, so this type can be
//! written to and read from a string. Reading is forgiving (a hand-edited file may hold anything); writing through
//! the methods below is strict.

/// The most groups there can be (one category bit each; Roblox allows the same number).
pub const MAX_GROUPS: usize = 32;
/// The group every part starts in. It always exists.
pub const DEFAULT_GROUP: &str = "Default";
/// The longest a group name may be.
pub const MAX_NAME_LEN: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionGroups {
    names: Vec<String>,
    /// `blocked[i]` has bit `j` set when groups `i` and `j` do not collide (kept symmetric).
    blocked: Vec<u32>,
}

impl Default for CollisionGroups {
    fn default() -> Self {
        CollisionGroups { names: vec![DEFAULT_GROUP.to_owned()], blocked: vec![0] }
    }
}

/// Why a name cannot be a group name, or `None` if it can.
pub fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("a collision group needs a name".into());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Some(format!("a collision group name can be at most {MAX_NAME_LEN} characters"));
    }
    if name.chars().any(char::is_control) {
        return Some("a collision group name cannot contain control characters".into());
    }
    if name != name.trim() {
        return Some("a collision group name cannot start or end with a space".into());
    }
    None
}

impl CollisionGroups {
    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        false // "Default" is always there
    }

    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index(name).is_some()
    }

    fn need(&self, name: &str) -> Result<usize, String> {
        self.index(name).ok_or_else(|| format!("'{name}' is not a registered collision group"))
    }

    /// Adds a group (it collides with every group, itself included, until told otherwise). Returns its number.
    pub fn register(&mut self, name: &str) -> Result<usize, String> {
        if let Some(problem) = name_problem(name) {
            return Err(problem);
        }
        if self.contains(name) {
            return Err(format!("the collision group '{name}' is already registered"));
        }
        if self.names.len() >= MAX_GROUPS {
            return Err(format!("there can be at most {MAX_GROUPS} collision groups"));
        }
        self.names.push(name.to_owned());
        self.blocked.push(0);
        Ok(self.names.len() - 1)
    }

    /// Removes a group. The parts in it are back in `Default` (their property is reset by the caller).
    pub fn unregister(&mut self, name: &str) -> Result<(), String> {
        let i = self.need(name)?;
        if i == 0 {
            return Err(format!("the '{DEFAULT_GROUP}' collision group cannot be removed"));
        }
        self.names.remove(i);
        self.blocked.remove(i);
        // Groups after it moved down one place: the bits above `i` shift down, the bit `i` is dropped.
        let low = (1u32 << i) - 1;
        for b in &mut self.blocked {
            *b = (*b & low) | ((*b >> 1) & !low);
        }
        Ok(())
    }

    pub fn set_collidable(&mut self, a: &str, b: &str, collide: bool) -> Result<(), String> {
        let (i, j) = (self.need(a)?, self.need(b)?);
        for (x, y) in [(i, j), (j, i)] {
            if collide {
                self.blocked[x] &= !(1 << y);
            } else {
                self.blocked[x] |= 1 << y;
            }
        }
        Ok(())
    }

    pub fn collidable(&self, a: &str, b: &str) -> Result<bool, String> {
        let (i, j) = (self.need(a)?, self.need(b)?);
        Ok(self.blocked[i] & (1 << j) == 0)
    }

    /// The Box3D category of a group.
    pub fn category(&self, index: usize) -> u64 {
        1u64 << index.min(MAX_GROUPS - 1)
    }

    /// The Box3D mask of a group: the categories it collides with. Bits of groups that do not exist (yet) are
    /// set, so a group registered later collides with this one as it does with every group by default - and
    /// `set_collidable` before it was registered cannot have said otherwise.
    pub fn mask(&self, index: usize) -> u64 {
        let blocked = self.blocked.get(index).copied().unwrap_or(0);
        let all = u64::MAX;
        all & !u64::from(blocked)
    }

    /// The category and mask for a part in the group called `name`; a name that is not registered behaves as
    /// `Default` (the caller reports it).
    pub fn filter_for(&self, name: &str) -> (u64, u64, bool) {
        match self.index(name) {
            Some(i) => (self.category(i), self.mask(i), true),
            None => (self.category(0), self.mask(0), false),
        }
    }

    // ---- saving ------------------------------------------------------------------------------------

    /// The text saved with the scene: one `G<TAB>name` line per group after `Default`, then one `B<TAB>a<TAB>b`
    /// line per pair that does not collide.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for name in self.names.iter().skip(1) {
            out.push_str("G\t");
            out.push_str(name);
            out.push('\n');
        }
        for i in 0..self.names.len() {
            for j in i..self.names.len() {
                if self.blocked[i] & (1 << j) != 0 {
                    out.push_str(&format!("B\t{}\t{}\n", self.names[i], self.names[j]));
                }
            }
        }
        out
    }

    /// Reads the text back. Lines that make no sense (an unknown group, a name that is not allowed, too many
    /// groups) are skipped and listed in the second value, so the rest still loads.
    pub fn decode(text: &str) -> (CollisionGroups, Vec<String>) {
        let mut groups = CollisionGroups::default();
        let mut problems = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = line.split('\t').collect();
            let result = match parts.as_slice() {
                ["G", name] if *name == DEFAULT_GROUP => Ok(()),
                ["G", name] => groups.register(name).map(|_| ()),
                ["B", a, b] => groups.set_collidable(a, b, false),
                _ => Err(format!("cannot read the line '{line}'")),
            };
            if let Err(e) = result {
                problems.push(e);
            }
        }
        (groups, problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_set_has_only_default_and_everything_collides() {
        let g = CollisionGroups::default();
        assert_eq!(g.names(), ["Default"]);
        assert_eq!(g.collidable("Default", "Default"), Ok(true));
        assert_eq!(g.mask(0), u64::MAX);
    }

    #[test]
    fn groups_register_once_with_valid_names_and_up_to_the_limit() {
        let mut g = CollisionGroups::default();
        assert_eq!(g.register("Players"), Ok(1));
        assert!(g.register("Players").unwrap_err().contains("already registered"));
        for bad in ["", " lead", "trail ", "tab\tin", "new\nline"] {
            assert!(g.register(bad).is_err(), "{bad:?}");
        }
        assert!(g.register(&"x".repeat(MAX_NAME_LEN + 1)).is_err());
        for i in g.len()..MAX_GROUPS {
            g.register(&format!("g{i}")).unwrap();
        }
        assert!(g.register("one too many").unwrap_err().contains("at most 32"));
    }

    #[test]
    fn collidability_is_symmetric_and_can_be_switched_back() {
        let mut g = CollisionGroups::default();
        g.register("A").unwrap();
        g.register("B").unwrap();
        g.set_collidable("A", "B", false).unwrap();
        assert_eq!((g.collidable("A", "B"), g.collidable("B", "A")), (Ok(false), Ok(false)));
        assert_eq!(g.collidable("A", "A"), Ok(true), "a group still collides with itself");
        assert_eq!(g.mask(1) & g.category(2), 0);
        assert_ne!(g.mask(1) & g.category(0), 0);
        g.set_collidable("B", "A", true).unwrap();
        assert_eq!(g.collidable("A", "B"), Ok(true));
        g.set_collidable("A", "A", false).unwrap();
        assert_eq!(g.collidable("A", "A"), Ok(false));
        assert!(g.set_collidable("A", "Nope", false).unwrap_err().contains("not a registered"));
    }

    #[test]
    fn removing_a_group_keeps_the_rules_of_the_others() {
        let mut g = CollisionGroups::default();
        for n in ["A", "B", "C"] {
            g.register(n).unwrap();
        }
        g.set_collidable("A", "C", false).unwrap();
        g.set_collidable("B", "C", false).unwrap();
        g.set_collidable("A", "B", false).unwrap();
        g.unregister("B").unwrap();
        assert_eq!(g.names(), ["Default", "A", "C"]);
        assert_eq!(g.collidable("A", "C"), Ok(false), "A and C still do not collide");
        assert_eq!(g.collidable("Default", "C"), Ok(true));
        assert!(g.unregister("Default").is_err());
        assert!(g.unregister("B").is_err());
    }

    #[test]
    fn the_text_form_round_trips_and_bad_lines_do_not_stop_the_rest() {
        let mut g = CollisionGroups::default();
        g.register("Player Team").unwrap();
        g.register("Enemies").unwrap();
        g.set_collidable("Player Team", "Enemies", false).unwrap();
        g.set_collidable("Enemies", "Enemies", false).unwrap();
        let (back, problems) = CollisionGroups::decode(&g.encode());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back, g);

        let (partial, problems) = CollisionGroups::decode("G\tGood\nG\n???\nB\tGood\tMissing\nG\tGood\nB\tGood\tDefault\n");
        assert_eq!(partial.names(), ["Default", "Good"]);
        assert_eq!(partial.collidable("Good", "Default"), Ok(false), "the lines after the bad ones were read");
        assert_eq!(problems.len(), 4, "{problems:?}");
        let (empty, problems) = CollisionGroups::decode("");
        assert_eq!((empty, problems.len()), (CollisionGroups::default(), 0));
    }

    #[test]
    fn an_unregistered_name_behaves_as_default_and_says_so() {
        let mut g = CollisionGroups::default();
        g.register("A").unwrap();
        g.set_collidable("A", "Default", false).unwrap();
        assert_eq!(g.filter_for("A"), (2, g.mask(1), true));
        assert_eq!(g.filter_for("Gone"), (1, g.mask(0), false));
    }
}
