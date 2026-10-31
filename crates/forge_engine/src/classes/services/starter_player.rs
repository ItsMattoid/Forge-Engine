//! StarterPlayer: where LocalScripts live and run. The engine has no character settings: a player controller is a script.

use crate::core::*;

static PROPS: &[PropMeta] = &[
    PropMeta::number("CameraMaxZoomDistance", 128.0).group("Camera").order(0).hidden(),
    PropMeta::number("CameraMinZoomDistance", 0.5).group("Camera").order(1).hidden(),
];

pub static CLASS: ClassDesc = ClassDesc::new("StarterPlayer", Some("Instance"))
    .describe("where LocalScripts live and run.")
    .service(8)
    .persisted()
    .runs_scripts()
    .container()
    .groups(&["Camera"])
    .props(PROPS);