//! MarketplaceService: Roblox's store. Forge has no store: what ownership questions can be answered ("does the player
//! own this pass?" - no) are answered, and everything that would need the marketplace says it is not supported.

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("PromptPurchaseFinished").desc("A purchase prompt closed (never fired)."),
    SignalDesc::new("PromptProductPurchaseFinished").desc("A developer product prompt closed (never fired)."),
    SignalDesc::new("PromptGamePassPurchaseFinished").desc("A game pass prompt closed (never fired)."),
    SignalDesc::new("PromptBundlePurchaseFinished").desc("A bundle prompt closed (never fired)."),
];

pub static CLASS: ClassDesc = ClassDesc::new("MarketplaceService", Some("Instance"))
    .describe("Roblox's store (not available in Forge).")
    .service(22)
    .hidden_in_explorer()
    .signals(SIGNALS)
    .callbacks(&["ProcessReceipt"])
    .unsupported(&[
        "PromptPurchase", "PromptProductPurchase", "PromptGamePassPurchase", "PromptBundlePurchase", "PromptPremiumPurchase", "PromptSubscriptionPurchase", "PromptRobuxPurchase", "PerformPurchase",
        "GetProductInfo", "GetDeveloperProductsAsync", "GetUserSubscriptionStatusAsync", "GetRobuxBalance",
    ])
    .luau(include_str!("marketplace_service.luau"));