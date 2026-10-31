//! SoundService: where the game's sounds and audio settings live. Its settings act on every sound the audio mirror plays
//! (`audio::sync`): `AmbientReverb` is the room all sounds are heard in, `RolloffScale` and `DistanceFactor` scale 3D
//! distances and the speed of sound, `DopplerScale` the pitch shift of moving sounds.

use crate::core::*;

pub static ENUMS: &[EnumDesc] = &[EnumDesc::new(
    "ReverbType",
    &["NoReverb", "GenericReverb", "PaddedCell", "Room", "Bathroom", "LivingRoom", "StoneRoom", "Auditorium", "ConcertHall", "Cave", "Arena", "Hangar", "CarpettedHallway", "Hallway", "StoneCorridor", "Alley", "Forest", "City", "Mountains", "Quarry", "Plain", "ParkingLot", "SewerPipe", "UnderWater"],
)];

static PROPS: &[PropMeta] = &[
    PropMeta::enumeration("AmbientReverb", "ReverbType", "NoReverb").group("Audio").order(0).desc("The room every sound is heard in (an echo added to all of them)."),
    PropMeta::number("DistanceFactor", 3.33).group("Audio").order(1).drag(0.01).clamp(0.01, 1000.0).desc("Studs per metre: turns the speed of sound into studs per second for the Doppler effect."),
    PropMeta::number("DopplerScale", 1.0).group("Audio").order(2).drag(0.01).clamp(0.0, 100.0).desc("How strongly moving sounds change pitch (0 turns the Doppler effect off)."),
    PropMeta::number("RolloffScale", 1.0).group("Audio").order(3).drag(0.01).clamp(0.0, 100.0).desc("How quickly 3D sounds fade with distance (2 doubles every distance)."),
    PropMeta::boolean("RespectFilteringEnabled", false).group("Audio").order(4).hidden(),
];

pub static CLASS: ClassDesc = ClassDesc::new("SoundService", Some("Instance"))
    .describe("where the game's sounds and audio settings live.")
    .service(9)
    .persisted()
    .container()
    .groups(&["Audio"])
    .props(PROPS);