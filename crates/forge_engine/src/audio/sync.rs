//! The mirror between the data model's `Sound` objects and the mixer's voices.
//!
//! Once per frame (while a game runs) [`SoundMirror::update`]:
//!
//! 1. tells the data model what the mixer reports (a sound ended, a looping one went round);
//! 2. for every `Sound` (anything with the `SoundEmitter` component): starts, pauses, resumes or stops its voice to match
//!    `Playing` / `PauseState`, and keeps the voice's volume (`Volume` x its `SoundGroup`s), speed (`PlaybackSpeed`, Doppler)
//!    and looping (`Looped`) the way the properties say, its place between the ears and its loudness by distance (when it
//!    sits in a part or attachment), and its effects (the sound-effect children of the `Sound` and of its groups, then the
//!    service's ambient reverb);
//! 3. keeps `TimePosition` true: written back as the sound plays, and obeyed when a script sets it.
//!
//! Sounds do not play while nothing runs (the editor's edit mode), and a sound outside the places a game is heard from
//! (`Workspace`, `SoundService`, a player's GUI) is silent - as in Roblox.

use std::collections::{HashMap, HashSet};

use bevy::math::Vec3;

use super::{
    dsp::{EffectSpec, FilterType},
    engine::AudioEngine,
    mixer::{MixerEvent, VoiceId, VoiceParams},
    spatial::{self, Listener, RollOff, SpatialSettings},
};
use crate::{
    core::{Id, ScriptValue, Value},
    dm::DataModel,
};

/// How far `TimePosition` may drift from the voice before the voice is moved to it (a script set it).
const SEEK_TOLERANCE: f32 = 0.08;
/// How much the voice must have moved since `TimePosition` was last written before it is written again.
const WRITE_STEP: f32 = 0.05;
/// Group nesting that is followed (a loop of groups must not hang the game).
const MAX_GROUP_DEPTH: usize = 8;

struct Tracked {
    voice: VoiceId,
    /// What `TimePosition` was last made to say (by us), to tell a script's change from ours.
    written: f32,
    path: String,
}

/// What a sound with `PlayOnRemove` needs to be played once when it goes.
struct Snapshot {
    path: String,
    volume: f32,
    speed: f32,
    position: Option<Vec3>,
    spatial: SpatialSettings,
    effects: Vec<EffectSpec>,
}

#[derive(Default)]
pub struct SoundMirror {
    tracked: HashMap<Id, Tracked>,
    by_voice: HashMap<VoiceId, Id>,
    snapshots: HashMap<Id, Snapshot>,
    emitter_last: HashMap<Id, Vec3>,
    listener_last: Option<Vec3>,
    warned: HashSet<String>,
}

/// Whether anything counts as a place a game is heard from.
fn audible(dm: &DataModel, id: Id) -> bool {
    let mut cur = Some(id);
    while let Some(c) = cur {
        match dm.class_name(c) {
            "Workspace" | "SoundService" | "PlayerGui" | "StarterGui" => return true,
            _ => {}
        }
        cur = dm.parent(c);
    }
    false
}

/// Where a sound is, if it has a place: the part, attachment or model it is in.
fn emitter_position(dm: &DataModel, id: Id) -> Option<Vec3> {
    let parent = dm.parent(id)?;
    if dm.is_a(parent, "BasePart") || dm.is_a(parent, "Attachment") {
        return dm.world_cframe(parent).map(|(p, _)| p);
    }
    if dm.is_a(parent, "Model") {
        return dm.get_pivot(parent).map(|(p, _)| p);
    }
    None
}

fn service_settings(dm: &DataModel) -> (f32, f32, f32, Vec<EffectSpec>) {
    match dm.service("SoundService") {
        Some(s) => (dm.num(s, "RolloffScale", 1.0), dm.num(s, "DopplerScale", 1.0), dm.num(s, "DistanceFactor", 3.33), ambient_reverb(dm.str_ref(s, "AmbientReverb"))),
        None => (1.0, 1.0, 3.33, Vec::new()),
    }
}

/// `SoundService.AmbientReverb`: the room every sound is heard in.
pub fn ambient_reverb(name: &str) -> Vec<EffectSpec> {
    let room = |decay: f32, wet_db: f32| EffectSpec::Reverb { decay, density: 0.8, diffusion: 0.9, dry_db: 0.0, wet_db };
    match name {
        "GenericReverb" => vec![room(1.5, -12.0)],
        "PaddedCell" => vec![room(0.3, -18.0)],
        "Room" => vec![room(0.6, -12.0)],
        "Bathroom" => vec![room(1.2, -9.0)],
        "LivingRoom" => vec![room(0.5, -14.0)],
        "StoneRoom" => vec![room(1.6, -10.0)],
        "Auditorium" => vec![room(2.6, -9.0)],
        "ConcertHall" => vec![room(3.5, -9.0)],
        "Cave" => vec![room(3.0, -8.0)],
        "Arena" => vec![room(4.5, -10.0)],
        "Hangar" => vec![room(6.0, -9.0)],
        "CarpettedHallway" => vec![room(0.3, -16.0)],
        "Hallway" => vec![room(1.5, -10.0)],
        "StoneCorridor" => vec![room(2.2, -9.0)],
        "Alley" => vec![room(1.5, -12.0)],
        "Forest" => vec![room(0.7, -18.0)],
        "City" => vec![room(1.2, -16.0)],
        "Mountains" => vec![room(4.5, -20.0)],
        "Quarry" => vec![room(1.2, -16.0)],
        "Plain" => vec![room(0.5, -22.0)],
        "ParkingLot" => vec![room(2.5, -12.0)],
        "SewerPipe" => vec![room(3.0, -6.0)],
        "UnderWater" => vec![EffectSpec::Filter { kind: FilterType::Low, cutoff: 900.0, resonance: 0.8 }, room(1.8, -9.0)],
        _ => Vec::new(),
    }
}

/// The sound-effect children of an instance, in the order they are applied (`Priority`, then the order they sit in).
fn effects_of(dm: &DataModel, id: Id) -> Vec<EffectSpec> {
    let mut found: Vec<(i32, EffectSpec)> = Vec::new();
    for child in dm.children(id) {
        let Some(class) = dm.get(child).map(|i| i.class) else { continue };
        if !dm.flag(child, "Enabled") {
            continue;
        }
        for comp in &class.components {
            if let Some(spec) = comp.audio_effect.and_then(|f| f(dm, child)) {
                found.push((dm.num(child, "Priority", 0.0) as i32, spec));
            }
        }
    }
    found.sort_by_key(|(priority, _)| *priority);
    found.into_iter().map(|(_, spec)| spec).collect()
}

/// The volume a chain of sound groups lets through, and the effects they add.
fn group_chain(dm: &DataModel, sound: Id) -> (f32, Vec<EffectSpec>) {
    let (mut volume, mut effects) = (1.0, Vec::new());
    let mut group = dm.reference(sound, "SoundGroup");
    for _ in 0..MAX_GROUP_DEPTH {
        let Some(g) = group else { break };
        volume *= dm.num(g, "Volume", 1.0).max(0.0);
        effects.extend(effects_of(dm, g));
        group = dm.parent(g).filter(|p| dm.is_a(*p, "SoundGroup"));
    }
    (volume, effects)
}

impl SoundMirror {
    pub fn new() -> SoundMirror {
        SoundMirror::default()
    }

    /// How many sounds are being played.
    pub fn playing(&self) -> usize {
        self.tracked.len()
    }

    /// Stops the voices of the scene's sounds (the game stopped, or never started). Other sounds in the mixer - the editor's
    /// preview of a file - are left alone.
    pub fn stop_all(&mut self, engine: &mut AudioEngine) {
        if self.tracked.is_empty() && self.snapshots.is_empty() {
            return;
        }
        {
            let mut mixer = engine.mixer();
            for t in self.tracked.values() {
                mixer.stop(t.voice);
            }
        }
        self.tracked.clear();
        self.by_voice.clear();
        self.snapshots.clear();
        self.emitter_last.clear();
    }

    fn listener(&mut self, dm: &DataModel, dt: f32) -> Listener {
        let camera = dm.reference(dm.workspace, "CurrentCamera");
        let (pos, rot) = camera.and_then(|c| dm.world_cframe(c)).unwrap_or((Vec3::ZERO, bevy::math::Quat::IDENTITY));
        let velocity = match self.listener_last {
            Some(last) if dt > 1e-4 => (pos - last) / dt,
            _ => Vec3::ZERO,
        };
        self.listener_last = Some(pos);
        Listener { pos, rot, velocity }
    }

    fn warn_once(&mut self, key: &str, message: String) {
        if self.warned.insert(key.to_owned()) {
            bevy::log::warn!("{message}");
        }
    }

    /// One frame. `running`: a game is running (otherwise everything is stopped).
    pub fn update(&mut self, dm: &mut DataModel, engine: &mut AudioEngine, dt: f32, running: bool) {
        if !running {
            self.stop_all(engine);
            return;
        }
        let Some(info) = dm.asset_info() else { return };

        // 1. What the mixer reports.
        for event in engine.mixer().take_events() {
            match event {
                MixerEvent::Ended(v) => {
                    if let Some(id) = self.by_voice.remove(&v) {
                        self.tracked.remove(&id);
                        if dm.exists(id) {
                            let _ = dm.set_prop(id, "Finished", Value::Bool(true));
                            let _ = dm.set_prop(id, "Playing", Value::Bool(false));
                            let _ = dm.set_prop(id, "Finished", Value::Bool(false));
                            let _ = dm.set_prop(id, "TimePosition", Value::Num(0.0));
                        }
                    }
                }
                MixerEvent::Looped(v) => {
                    if let Some(id) = self.by_voice.get(&v).copied() {
                        let path = dm.string(id, "SoundId");
                        dm.fire(id, "DidLoop", || vec![ScriptValue::Str(path)]);
                        if let Some(t) = self.tracked.get_mut(&id) {
                            t.written = 0.0;
                        }
                        let _ = dm.set_prop(id, "TimePosition", Value::Num(0.0));
                    }
                }
            }
        }

        // 2. Every sound.
        let listener = self.listener(dm, dt);
        let (rolloff_scale, doppler_scale, studs_per_metre, ambient) = service_settings(dm);
        let sounds = dm.with_component("SoundEmitter");
        let mut seen: HashSet<Id> = HashSet::with_capacity(sounds.len());
        for id in sounds {
            seen.insert(id);
            let playing = dm.flag(id, "Playing");
            let paused = dm.flag(id, "PauseState");
            let here = audible(dm, id);
            let tracked = self.tracked.contains_key(&id);
            let path = dm.string(id, "SoundId");

            let position = emitter_position(dm, id);
            let spatial = SpatialSettings {
                mode: RollOff::from_name(dm.str_ref(id, "RollOffMode")),
                min_distance: dm.num(id, "RollOffMinDistance", 10.0),
                max_distance: dm.num(id, "RollOffMaxDistance", 10_000.0),
                rolloff_scale,
                doppler_scale,
                studs_per_metre,
            };
            let (group_volume, group_effects) = group_chain(dm, id);
            let volume = (dm.num(id, "Volume", 0.5).max(0.0) * group_volume).min(100.0);
            let speed = dm.num(id, "PlaybackSpeed", 1.0).clamp(0.05, 20.0);
            let looped = dm.flag(id, "Looped");

            let mut chain = effects_of(dm, id);
            chain.extend(group_effects);
            chain.extend(ambient.iter().copied());

            if dm.flag(id, "PlayOnRemove") {
                self.snapshots.insert(id, Snapshot { path: path.clone(), volume, speed, position: position.filter(|_| here), spatial: spatial.clone(), effects: chain.clone() });
            } else if self.snapshots.contains_key(&id) {
                self.snapshots.remove(&id);
            }

            let want = here && playing;
            let keep_paused = here && paused && tracked;
            if !want && !keep_paused {
                if let Some(t) = self.tracked.remove(&id) {
                    self.by_voice.remove(&t.voice);
                    engine.mixer().stop(t.voice);
                }
                continue;
            }

            // The placement: loudness and place between the ears, and Doppler.
            let (gain, pan, doppler) = match position {
                Some(p) => {
                    let velocity = match self.emitter_last.get(&id) {
                        Some(last) if dt > 1e-4 => (p - *last) / dt,
                        _ => Vec3::ZERO,
                    };
                    self.emitter_last.insert(id, p);
                    let placement = spatial::place(&listener, p, velocity, &spatial);
                    (placement.gain, placement.pan, placement.doppler)
                }
                None => (1.0, 0.0, 1.0),
            };
            let params = VoiceParams { volume: volume * gain, speed: speed * doppler, looped, pan };

            if !tracked {
                let clip = match info.clip(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        self.warn_once(&path, format!("Sound '{}' cannot play: {e}", dm.name(id)));
                        continue;
                    }
                };
                engine.prepare();
                let length = clip.seconds();
                let mut start = dm.num(id, "TimePosition", 0.0).max(0.0);
                if start >= length {
                    start = if looped && length > 0.0 { start % length } else { 0.0 };
                }
                let voice = engine.mixer().play(clip, start, params, &chain);
                self.by_voice.insert(voice, id);
                self.tracked.insert(id, Tracked { voice, written: start, path });
                continue;
            }

            let Some(t) = self.tracked.get_mut(&id) else { continue };
            if t.path != path {
                // The file was changed under a playing sound: start the new one over.
                engine.mixer().stop(t.voice);
                self.by_voice.remove(&t.voice);
                self.tracked.remove(&id);
                continue;
            }
            let mut mixer = engine.mixer();
            mixer.set_params(t.voice, params);
            mixer.set_effects(t.voice, &chain);
            mixer.set_paused(t.voice, !want);
            let time = dm.num(id, "TimePosition", 0.0);
            if (time - t.written).abs() > SEEK_TOLERANCE {
                mixer.seek(t.voice, time);
                t.written = time;
            } else if let Some(now) = mixer.position(t.voice) {
                if (now - t.written).abs() >= WRITE_STEP && want {
                    t.written = now;
                    drop(mixer);
                    let _ = dm.set_prop(id, "TimePosition", Value::Num(now));
                }
            }
        }

        // 3. Sounds that went away: a tracked voice stops; one with PlayOnRemove is played once.
        let gone: Vec<Id> = self.tracked.keys().filter(|id| !seen.contains(id)).copied().collect();
        for id in gone {
            if let Some(t) = self.tracked.remove(&id) {
                self.by_voice.remove(&t.voice);
                engine.mixer().stop(t.voice);
            }
        }
        let leaving: Vec<Id> = self.snapshots.keys().filter(|id| !seen.contains(id) || !dm.get(**id).is_some_and(|_| audible(dm, **id))).copied().collect();
        for id in leaving {
            let Some(s) = self.snapshots.remove(&id) else { continue };
            self.emitter_last.remove(&id);
            if let Ok(clip) = info.clip(&s.path) {
                let (gain, pan, doppler) = match s.position {
                    Some(p) => {
                        let placement = spatial::place(&listener, p, Vec3::ZERO, &s.spatial);
                        (placement.gain, placement.pan, 1.0)
                    }
                    None => (1.0, 0.0, 1.0),
                };
                engine.prepare();
                engine.mixer().play(clip, 0.0, VoiceParams { volume: s.volume * gain, speed: s.speed * doppler, looped: false, pan }, &s.effects);
            }
        }
        self.emitter_last.retain(|id, _| seen.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use std::{rc::Rc, sync::atomic::{AtomicUsize, Ordering}};

    use super::*;
    use crate::{
        assets::{info::AssetInfo, GameSource},
        audio::clip::wav_bytes,
    };

    struct Rig {
        dm: DataModel,
        engine: AudioEngine,
        mirror: SoundMirror,
        dir: std::path::PathBuf,
    }

    impl Rig {
        fn new() -> Rig {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("forge_audio_sync_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(dir.join("Assets")).unwrap();
            // Two seconds of a 220 Hz tone.
            let samples: Vec<i16> = (0..16000).map(|i| ((i as f32 / 8000.0 * 220.0 * std::f32::consts::TAU).sin() * 12000.0) as i16).collect();
            std::fs::write(dir.join("Assets/tone.wav"), wav_bytes(8000, 1, &samples)).unwrap();
            let mut dm = DataModel::new();
            dm.set_asset_info(Some(Rc::new(AssetInfo::new(GameSource::Dir(dir.clone())))));
            Rig { dm, engine: AudioEngine::offline(), mirror: SoundMirror::new(), dir }
        }

        /// The listener: the workspace's current camera, put somewhere known and turned to look along -Z.
        fn listener_at(&mut self, at: Vec3) -> Id {
            let cam = self.dm.reference(self.dm.workspace, "CurrentCamera").expect("a workspace has a camera");
            self.dm.set_cframe(cam, at, bevy::math::Quat::IDENTITY);
            cam
        }

        fn sound(&mut self, parent: Id) -> Id {
            let s = self.dm.create("Sound").unwrap();
            self.dm.set_prop(s, "SoundId", Value::Str("Assets/tone.wav".into())).unwrap();
            self.dm.set_parent(s, Some(parent)).unwrap();
            s
        }

        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.engine.update(1.0 / 60.0);
                self.mirror.update(&mut self.dm, &mut self.engine, 1.0 / 60.0, true);
            }
        }

        fn set(&mut self, id: Id, prop: &str, v: Value) {
            self.dm.set_prop(id, prop, v).unwrap();
        }

        fn voice_of(&self, s: Id) -> Option<VoiceId> {
            self.mirror.tracked.get(&s).map(|t| t.voice)
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    #[test]
    fn setting_playing_starts_a_voice_and_the_sound_ends_by_itself_with_the_property_following() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let s = r.sound(ws);
        r.frames(3);
        assert!(r.voice_of(s).is_none(), "nothing plays until asked");
        r.set(s, "Playing", Value::Bool(true));
        r.frames(2);
        let voice = r.voice_of(s).expect("playing");
        assert!(r.engine.mixer().is_playing(voice));
        r.frames(30);
        let tp = r.dm.num(s, "TimePosition", 0.0);
        assert!((tp - 0.5).abs() < 0.12, "TimePosition follows the sound: {tp}");
        r.frames(120);
        assert!(!r.dm.flag(s, "Playing"), "the sound ended: Playing is off");
        assert_eq!(r.dm.num(s, "TimePosition", 9.0), 0.0, "and it is rewound");
        assert!(r.voice_of(s).is_none() && !r.engine.mixer().is_playing(voice));
    }

    #[test]
    fn a_looped_sound_keeps_playing_and_reports_each_lap() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let s = r.sound(ws);
        r.set(s, "Looped", Value::Bool(true));
        r.set(s, "Playing", Value::Bool(true));
        r.frames(60 * 5);
        assert!(r.dm.flag(s, "Playing") && r.voice_of(s).is_some(), "still going after two and a half laps");
    }

    #[test]
    fn volume_speed_and_looping_follow_the_properties_while_the_sound_plays() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let s = r.sound(ws);
        r.set(s, "Playing", Value::Bool(true));
        r.frames(2);
        let v = r.voice_of(s).unwrap();
        let params = |r: &Rig| r.engine.mixer().params(v);
        assert_eq!(params(&r).unwrap().volume, 0.5, "the default Volume");
        r.set(s, "Volume", Value::Num(0.8));
        r.set(s, "PlaybackSpeed", Value::Num(1.5));
        r.set(s, "Looped", Value::Bool(true));
        r.frames(1);
        let p = params(&r).unwrap();
        assert_eq!((p.volume, p.speed, p.looped), (0.8, 1.5, true));
    }

    #[test]
    fn pause_keeps_the_place_and_resume_goes_on_from_it_and_stop_rewinds() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let s = r.sound(ws);
        r.set(s, "Playing", Value::Bool(true));
        r.frames(30);
        let before = r.dm.num(s, "TimePosition", 0.0);
        r.set(s, "PauseState", Value::Bool(true));
        r.set(s, "Playing", Value::Bool(false));
        r.frames(30);
        let v = r.voice_of(s).expect("a paused sound keeps its voice");
        let held = r.engine.mixer().position(v).unwrap();
        r.frames(30);
        assert_eq!(r.engine.mixer().position(v).unwrap(), held, "time stands still");
        assert!((held - before).abs() < 0.2);
        r.set(s, "PauseState", Value::Bool(false));
        r.set(s, "Playing", Value::Bool(true));
        r.frames(30);
        assert!(r.engine.mixer().position(v).unwrap() > held + 0.3, "and goes on");
        r.set(s, "TimePosition", Value::Num(0.0));
        r.set(s, "Playing", Value::Bool(false));
        r.frames(2);
        assert!(r.voice_of(s).is_none(), "stopped");
    }

    #[test]
    fn a_script_setting_time_position_moves_the_sound() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let s = r.sound(ws);
        r.set(s, "Playing", Value::Bool(true));
        r.frames(10);
        r.set(s, "TimePosition", Value::Num(1.5));
        r.frames(2);
        let v = r.voice_of(s).unwrap();
        assert!((r.engine.mixer().position(v).unwrap() - 1.5).abs() < 0.1);
        // Starting in the middle.
        let s2 = r.sound(ws);
        r.set(s2, "TimePosition", Value::Num(1.0));
        r.set(s2, "Playing", Value::Bool(true));
        r.frames(1);
        assert!((r.engine.mixer().position(r.voice_of(s2).unwrap()).unwrap() - 1.0).abs() < 0.1);
    }

    #[test]
    fn a_sound_in_a_part_gets_quieter_with_distance_and_pans_to_its_side() {
        let mut r = Rig::new();
        r.listener_at(Vec3::ZERO);
        let ws = r.dm.workspace;
        let part = r.dm.create("Part").unwrap();
        r.dm.set_parent(part, Some(ws)).unwrap();
        r.set(part, "Position", Value::V3(Vec3::new(5.0, 0.0, 0.0)));
        let s = r.sound(part);
        r.set(s, "Playing", Value::Bool(true));
        r.frames(2);
        let v = r.voice_of(s).unwrap();
        let near = r.engine.mixer().params(v).unwrap();
        assert!(near.pan > 0.9, "on the right: {}", near.pan);
        assert!((near.volume - 0.5).abs() < 0.01, "inside the minimum distance: full");
        r.set(part, "Position", Value::V3(Vec3::new(0.0, 0.0, -100.0)));
        r.frames(2);
        let far = r.engine.mixer().params(v).unwrap();
        assert!(far.volume < near.volume * 0.2, "100 studs away: {}", far.volume);
        assert!(far.pan.abs() < 0.01, "straight ahead");
        // The listener is the current camera: move it next to the sound.
        r.listener_at(Vec3::new(0.0, 0.0, -95.0));
        r.frames(2);
        assert!(r.engine.mixer().params(v).unwrap().volume > far.volume * 3.0, "the listener walked up to it");
        // A sound that is not in anything with a place is not positioned.
        let global = r.sound(ws);
        r.set(global, "Playing", Value::Bool(true));
        r.frames(2);
        let g = r.engine.mixer().params(r.voice_of(global).unwrap()).unwrap();
        assert_eq!((g.pan, g.volume), (0.0, 0.5));
    }

    #[test]
    fn rolloff_distances_are_the_sounds_own_and_the_service_can_scale_them() {
        let mut r = Rig::new();
        r.listener_at(Vec3::ZERO);
        let ws = r.dm.workspace;
        let part = r.dm.create("Part").unwrap();
        r.dm.set_parent(part, Some(ws)).unwrap();
        r.set(part, "Position", Value::V3(Vec3::new(0.0, 0.0, -40.0)));
        let s = r.sound(part);
        r.set(s, "RollOffMode", Value::Enum("Linear".into()));
        r.set(s, "RollOffMinDistance", Value::Num(10.0));
        r.set(s, "RollOffMaxDistance", Value::Num(50.0));
        r.set(s, "Playing", Value::Bool(true));
        r.frames(2);
        let v = r.voice_of(s).unwrap();
        assert!((r.engine.mixer().params(v).unwrap().volume - 0.5 * 0.25).abs() < 0.01, "three quarters of the way out, a quarter left");
        let service = r.dm.service("SoundService").unwrap();
        r.set(service, "RolloffScale", Value::Num(2.0));
        r.frames(2);
        assert!(r.engine.mixer().params(v).unwrap().volume < 0.001, "doubled distances: out of range");
    }

    #[test]
    fn sound_groups_scale_the_volume_and_add_their_effects() {
        let mut r = Rig::new();
        let service = r.dm.service("SoundService").unwrap();
        let group = r.dm.create("SoundGroup").unwrap();
        r.dm.set_parent(group, Some(service)).unwrap();
        r.set(group, "Volume", Value::Num(0.5));
        let s = r.sound(service);
        r.set(s, "SoundGroup", Value::Ref(Some(group)));
        r.set(s, "Volume", Value::Num(0.8));
        r.set(s, "Playing", Value::Bool(true));
        r.frames(2);
        let v = r.voice_of(s).unwrap();
        assert!((r.engine.mixer().params(v).unwrap().volume - 0.4).abs() < 1e-5);
        assert_eq!(r.engine.mixer().effect_count(v), Some(0));
        let echo = r.dm.create("EchoSoundEffect").unwrap();
        r.dm.set_parent(echo, Some(group)).unwrap();
        let own = r.dm.create("DistortionSoundEffect").unwrap();
        r.dm.set_parent(own, Some(s)).unwrap();
        r.frames(2);
        assert_eq!(r.engine.mixer().effect_count(v), Some(2), "its own and its group's");
        r.set(own, "Enabled", Value::Bool(false));
        r.frames(2);
        assert_eq!(r.engine.mixer().effect_count(v), Some(1), "a disabled effect is left out");
        r.dm.destroy(echo);
        r.frames(2);
        assert_eq!(r.engine.mixer().effect_count(v), Some(0));
    }

    #[test]
    fn effects_apply_in_priority_order_and_the_ambient_reverb_is_added() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let s = r.sound(ws);
        let (a, b) = (r.dm.create("TremoloSoundEffect").unwrap(), r.dm.create("FlangeSoundEffect").unwrap());
        r.dm.set_parent(a, Some(s)).unwrap();
        r.dm.set_parent(b, Some(s)).unwrap();
        r.set(a, "Priority", Value::Num(5.0));
        r.set(b, "Priority", Value::Num(1.0));
        assert_eq!(effects_of(&r.dm, s).iter().map(EffectSpec::kind).collect::<Vec<_>>(), ["flange", "tremolo"]);
        let service = r.dm.service("SoundService").unwrap();
        r.set(service, "AmbientReverb", Value::Enum("Cave".into()));
        r.set(s, "Playing", Value::Bool(true));
        r.frames(2);
        assert_eq!(r.engine.mixer().effect_count(r.voice_of(s).unwrap()), Some(3));
        assert!(ambient_reverb("NoReverb").is_empty() && ambient_reverb("UnderWater").len() == 2);
    }

    #[test]
    fn nothing_plays_outside_the_places_a_game_is_heard_from_or_when_the_game_is_not_running() {
        let mut r = Rig::new();
        let storage = r.dm.service("ReplicatedStorage").unwrap();
        let hidden = r.sound(storage);
        r.set(hidden, "Playing", Value::Bool(true));
        let ws = r.dm.workspace;
        let heard = r.sound(ws);
        r.set(heard, "Playing", Value::Bool(true));
        r.frames(3);
        assert!(r.voice_of(hidden).is_none(), "a sound in storage is silent");
        assert!(r.voice_of(heard).is_some());
        r.mirror.update(&mut r.dm, &mut r.engine, 0.016, false);
        assert!(r.voice_of(heard).is_none() && r.engine.mixer().voice_count() == 0, "stopping the game stops its sounds");
        // Moving a sound out of the world silences it, moving it back starts it again.
        r.frames(2);
        assert!(r.voice_of(heard).is_some());
        r.dm.set_parent(heard, Some(storage)).unwrap();
        r.frames(2);
        assert!(r.voice_of(heard).is_none());
    }

    #[test]
    fn a_sound_that_is_destroyed_stops_and_one_with_play_on_remove_is_heard_once_more() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let s = r.sound(ws);
        r.set(s, "Playing", Value::Bool(true));
        r.frames(3);
        let v = r.voice_of(s).unwrap();
        r.dm.destroy(s);
        r.frames(2);
        assert!(!r.engine.mixer().is_playing(v), "gone with its sound");
        assert_eq!(r.engine.mixer().voice_count(), 0);

        let boom = r.sound(ws);
        r.set(boom, "PlayOnRemove", Value::Bool(true));
        r.frames(3);
        assert_eq!(r.engine.mixer().voice_count(), 0, "it waits");
        r.dm.destroy(boom);
        r.frames(2);
        assert_eq!(r.engine.mixer().voice_count(), 1, "played once as it was removed");
        r.frames(200);
        assert_eq!(r.engine.mixer().voice_count(), 0, "and then it is over");
    }

    #[test]
    fn a_file_that_cannot_be_played_does_not_stop_the_other_sounds() {
        let mut r = Rig::new();
        let ws = r.dm.workspace;
        let bad = r.sound(ws);
        r.set(bad, "SoundId", Value::Str("Assets/missing.wav".into()));
        r.set(bad, "Playing", Value::Bool(true));
        let good = r.sound(ws);
        r.set(good, "Playing", Value::Bool(true));
        r.frames(5);
        assert!(r.voice_of(bad).is_none() && r.voice_of(good).is_some());
    }
}
