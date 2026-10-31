//! ContentProvider: loading assets ahead of time. Everything a Forge game uses is loaded when it needs it, so
//! `PreloadAsync` returns at once (reporting each asset as loaded).

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::number("RequestQueueSize", 0.0).runtime().hidden().readonly()];

pub static ENUMS: &[EnumDesc] = &[EnumDesc::numbered("AssetFetchStatus", &["Success", "Failure", "TimedOut", "Loading", "None"], &[0, 1, 2, 3, 4])];

pub static CLASS: ClassDesc = ClassDesc::new("ContentProvider", Some("Instance"))
    .describe("Loading assets ahead of time.")
    .service(39)
    .hidden_in_explorer()
    .props(PROPS)
    .luau(include_str!("content_provider.luau"));