//! LocalizationService: translations. Forge has none; it reports one locale.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::string("RobloxLocaleId", "en-us").runtime().hidden().readonly(),
    PropMeta::string("SystemLocaleId", "en-us").runtime().hidden().readonly(),
];

pub static CLASS: ClassDesc = ClassDesc::new("LocalizationService", Some("Instance"))
    .describe("Translations (Forge reports one locale and translates nothing).")
    .service(26)
    .hidden_in_explorer()
    .props(PROPS)
    .unsupported(&["GetTranslatorForPlayerAsync", "GetTranslatorForLocaleAsync", "GetTranslatorForPlayer", "GetTranslatorForLocale"]);