//! VoiceChatService: voice chat. Forge has none.

use crate::core::*;

pub static CLASS: ClassDesc = ClassDesc::new("VoiceChatService", Some("Instance")).describe("Voice chat (none in Forge).").service(43).hidden_in_explorer().luau(include_str!("voice_chat_service.luau"));