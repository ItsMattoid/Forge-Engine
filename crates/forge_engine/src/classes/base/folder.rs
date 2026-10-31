//! Folder: organises other objects without any behaviour of its own.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("Folder", Some("Instance")).describe("A container for organising objects.").category("Parts").creatable().container();
