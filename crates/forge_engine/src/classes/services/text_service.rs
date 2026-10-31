//! TextService: measuring and filtering text. Without a font renderer the size is an estimate (each character is half
//! the text size wide, lines are as tall as the text size); filtering lets everything through.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("TextService", Some("Instance"))
    .describe("Measuring and filtering text.")
    .service(31)
    .hidden_in_explorer()
    .unsupported(&["GetFamilyInfoAsync"])
    .luau(include_str!("text_service.luau"));