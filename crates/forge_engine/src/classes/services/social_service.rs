//! SocialService: invites to the game. There is nobody to invite.

use crate::core::*;

static SIGNALS: &[SignalDesc] = &[SignalDesc::new("GameInvitePromptClosed").desc("An invite prompt closed (never fired).")];

pub static CLASS: ClassDesc = ClassDesc::new("SocialService", Some("Instance"))
    .describe("Invites to the game (not available in Forge).")
    .service(30)
    .hidden_in_explorer()
    .signals(SIGNALS)
    .unsupported(&["PromptGameInvite", "PromptPhoneBook", "PromptFriendRequest"])
    .luau(include_str!("social_service.luau"));