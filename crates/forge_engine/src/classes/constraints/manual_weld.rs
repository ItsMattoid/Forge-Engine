//! ManualWeld: the old name of a Weld whose frames are set by the maker. Works as `Weld` does.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("ManualWeld", Some("Weld")).describe("A weld with frames set by hand.").category("Constraints").creatable();