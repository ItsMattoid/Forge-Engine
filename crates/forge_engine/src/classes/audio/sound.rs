//! Sound: audio that plays from a file of the project (wav, mp3, ogg, flac, m4a).
//!
//! A `Sound` is described entirely by its properties; the engine's audio mirror (`audio::sync`) plays it while a game
//! runs. `Playing`, `Volume`, `PlaybackSpeed`, `Looped`, `TimePosition` and the three `RollOff` settings all act on the
//! sound as it plays. A sound inside a part or an attachment is positioned there (louder when near, towards the ear it is
//! on, shifted in pitch when it moves, see `audio::spatial`); a sound anywhere else is heard everywhere equally. Sound
//! effects (`ReverbSoundEffect` ...) inside the sound change how it is heard, and a `SoundGroup` scales its volume.
//!
//! The signals are fired in one place, [`SoundBehavior`], as `Playing` changes: the methods and a script that sets
//! `Playing` go through the same path. A sound that stopped *because it was paused* is told apart by the hidden
//! `PauseState` flag, and one that stopped *because it reached its end* by the hidden `Finished` flag (the mirror sets both
//! before it turns `Playing` off): that is `Ended`, not `Stopped`.

use crate::{core::*, dm::DataModel};

type R = Result<ScriptValue, String>;

fn play(c: &mut MethodCall) -> R {
    c.dm.set_prop(c.this, "PauseState", Value::Bool(false))?;
    if c.dm.flag(c.this, "Playing") {
        // Play() on a sound that is playing starts it over.
        c.dm.set_prop(c.this, "TimePosition", Value::Num(0.0))?;
        let (sound, id) = (c.this, c.dm.string(c.this, "SoundId"));
        c.dm.fire(sound, "Played", || vec![ScriptValue::Str(id)]);
    } else {
        c.dm.set_prop(c.this, "Playing", Value::Bool(true))?;
    }
    Ok(ScriptValue::Nil)
}

fn stop(c: &mut MethodCall) -> R {
    let was_paused = c.dm.flag(c.this, "PauseState");
    c.dm.set_prop(c.this, "PauseState", Value::Bool(false))?;
    c.dm.set_prop(c.this, "TimePosition", Value::Num(0.0))?;
    if was_paused {
        // Not playing already, so clearing Playing changes nothing: say it by hand.
        let (sound, id) = (c.this, c.dm.string(c.this, "SoundId"));
        c.dm.fire(sound, "Stopped", || vec![ScriptValue::Str(id)]);
    } else {
        c.dm.set_prop(c.this, "Playing", Value::Bool(false))?;
    }
    Ok(ScriptValue::Nil)
}

fn pause(c: &mut MethodCall) -> R {
    if c.dm.flag(c.this, "Playing") {
        c.dm.set_prop(c.this, "PauseState", Value::Bool(true))?;
        c.dm.set_prop(c.this, "Playing", Value::Bool(false))?;
    }
    Ok(ScriptValue::Nil)
}

fn resume(c: &mut MethodCall) -> R {
    if c.dm.flag(c.this, "PauseState") {
        c.dm.set_prop(c.this, "Playing", Value::Bool(true))?;
    }
    Ok(ScriptValue::Nil)
}

fn get_is_playing(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Bool(dm.flag(id, "Playing")))
}

fn get_is_paused(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Bool(dm.flag(id, "PauseState")))
}

/// How long the file is, in seconds (0 until it has a readable file).
fn get_time_length(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Num(dm.assets().and_then(|a| a.audio_length(dm.str_ref(id, "SoundId"))).unwrap_or(0.0)))
}

/// Whether the file can be played.
fn get_is_loaded(dm: &DataModel, id: Id) -> Option<Value> {
    Some(Value::Bool(dm.assets().is_some_and(|a| a.clip(dm.str_ref(id, "SoundId")).is_ok())))
}

static PROPS: &[PropMeta] = &[
    PropMeta::asset("SoundId", "ogg").group("Sound").order(0).desc("The audio file (a project file: wav, mp3, ogg, flac, m4a): drop one here or pick one."),
    PropMeta::number("Volume", 0.5).group("Sound").order(1).slider(0.0, 10.0).desc("How loud the sound is (1 is the file as it is)."),
    PropMeta::number("PlaybackSpeed", 1.0).group("Sound").order(2).drag(0.05).clamp(0.05, 20.0).desc("How fast it plays (1 is normal; 2 is twice as fast and an octave higher)."),
    PropMeta::boolean("Looped", false).group("Sound").order(3).desc("Whether it starts again when it ends."),
    PropMeta::boolean("Playing", false).group("Sound").order(4).desc("Whether it is playing. Setting it plays or stops (pausing: Pause). A sound saved as Playing starts when the game does."),
    PropMeta::boolean("PauseState", false).runtime().hidden().desc("Whether the sound is paused (not just stopped)."),
    PropMeta::boolean("Finished", false).runtime().hidden().desc("Set for the moment the sound is turned off because it reached its end."),
    PropMeta::number("TimePosition", 0.0).group("Sound").order(5).runtime().drag(0.1).clamp(0.0, 1.0e7).desc("Where in the sound it is, in seconds. Set it to jump."),
    PropMeta::number("TimeLength", 0.0).group("Sound").order(6).no_save().readonly().computed_ro(get_time_length).desc("How long the sound is, in seconds (0 until the file can be read)."),
    PropMeta::boolean("IsLoaded", false).group("Sound").order(7).no_save().readonly().computed_ro(get_is_loaded).desc("Whether the file can be read and played."),
    PropMeta::boolean("IsPlaying", false).group("Sound").order(8).no_save().hidden().computed_ro(get_is_playing).desc("Whether it is playing right now."),
    PropMeta::boolean("IsPaused", false).group("Sound").order(9).no_save().hidden().computed_ro(get_is_paused).desc("Whether it is paused."),
    PropMeta::boolean("PlayOnRemove", false).group("Sound").order(10).desc("Plays once more where it was when the sound is removed from the game (a pop, a pickup chime)."),
    PropMeta::enumeration("RollOffMode", "RollOffMode", "Inverse").group("3D").order(0).desc("How the volume falls with distance, for a sound inside a part or attachment."),
    PropMeta::number("RollOffMinDistance", 10.0).group("3D").order(1).drag(1.0).clamp(0.0, 1.0e6).desc("The distance up to which the sound is at full volume."),
    PropMeta::number("RollOffMaxDistance", 10000.0).group("3D").order(2).drag(10.0).clamp(0.0, 1.0e6).desc("The distance beyond which the sound is not heard."),
    PropMeta::reference("SoundGroup", "SoundGroup").group("Sound").order(11).desc("The group whose volume and effects the sound follows."),
];

static METHODS: &[MethodDesc] = &[
    MethodDesc::new("Play", play).desc("Starts the sound (from where it was paused or stopped)."),
    MethodDesc::new("Stop", stop).desc("Stops the sound and rewinds it."),
    MethodDesc::new("Pause", pause).desc("Pauses the sound."),
    MethodDesc::new("Resume", resume).desc("Continues a paused sound."),
];

static SIGNALS: &[SignalDesc] = &[
    SignalDesc::new("Played").desc("The sound started; passes its SoundId."),
    SignalDesc::new("Paused").desc("The sound was paused; passes its SoundId."),
    SignalDesc::new("Resumed").desc("The sound was resumed; passes its SoundId."),
    SignalDesc::new("Stopped").desc("The sound was stopped; passes its SoundId."),
    SignalDesc::new("Ended").desc("The sound played to its end; passes its SoundId."),
    SignalDesc::new("Loaded").desc("The file loaded; passes its SoundId (never fired: files are read when a sound starts, and IsLoaded says whether one can be)."),
    SignalDesc::new("DidLoop").desc("A looped sound started over; passes its SoundId."),
];

/// `Playing` turning on is `Played` (or `Resumed` after a pause); turning off is `Paused` (while paused), `Ended` (it
/// reached its end) or `Stopped`.
struct SoundBehavior;

impl ClassBehavior for SoundBehavior {
    fn on_prop_changed(&self, dm: &mut DataModel, id: Id, prop: &PropMeta) {
        if prop.name != "Playing" {
            return;
        }
        let sound_id = dm.string(id, "SoundId");
        let paused = dm.flag(id, "PauseState");
        if dm.flag(id, "Playing") {
            let signal = if paused { "Resumed" } else { "Played" };
            let _ = dm.set_prop(id, "PauseState", Value::Bool(false));
            dm.fire(id, signal, || vec![ScriptValue::Str(sound_id)]);
        } else {
            let signal = if paused { "Paused" } else if dm.flag(id, "Finished") { "Ended" } else { "Stopped" };
            dm.fire(id, signal, || vec![ScriptValue::Str(sound_id)]);
        }
    }
}

pub static COMPONENT: ComponentDesc = ComponentDesc::new("SoundEmitter").describe("Plays an audio file while a game runs (see audio::sync).");

pub static CLASS: ClassDesc = ClassDesc::new("Sound", Some("Instance"))
    .describe("Audio from a file: music, effects, voices; positioned in 3D when it is inside a part.")
    .category("Audio")
    .creatable()
    .parent_rule(ParentRule::SelectedOrService("SoundService"))
    .groups(&["Sound", "3D"])
    .components(&["SoundEmitter"])
    .props(PROPS)
    .methods(METHODS)
    .signals(SIGNALS)
    .behavior(&SoundBehavior);
