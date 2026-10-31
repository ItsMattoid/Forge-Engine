//! Chat: Roblox's chat window and bubbles. Forge has no chat; `Chat:Chat` and the settings are accepted and ignored.

use crate::core::*;

static PROPS: &[PropMeta] = &[PropMeta::boolean("BubbleChatEnabled", false).hidden(), PropMeta::boolean("LoadDefaultChat", true).hidden()];

pub static CLASS: ClassDesc = ClassDesc::new("Chat", Some("Instance")).describe("Roblox's chat (none in Forge).").service(46).hidden_in_explorer().props(PROPS).luau(include_str!("chat.luau"));