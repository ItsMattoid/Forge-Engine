//! Warnings and errors, kept for the "Warnings & Errors" overlay.
//!
//! A tracing layer ([`layer`]) sees everything the engine, the renderer and scripts report through
//! the log and keeps the warnings and errors (the log can be filtered by level as usual; this
//! only looks at WARN and ERROR). Hosts that report problems some other way call [`push`].
//!
//! The record is process-wide: logging can happen on any thread, and the overlay only reads it a few
//! times per second.

use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
};

use bevy::{
    app::App,
    log::{
        tracing::{
            field::{Field, Visit},
            Event, Level, Subscriber,
        },
        tracing_subscriber::{layer::Context, Layer},
        BoxedLayer,
    },
};

/// How many recent entries are kept.
const KEEP: usize = 200;
/// Entries are cut to this many characters.
const MAX_TEXT: usize = 400;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub severity: Severity,
    /// Where it came from (the log target: `script`, `wgpu`, `forge_editor`, ...).
    pub source: String,
    pub text: String,
    /// How many identical entries in a row this stands for.
    pub repeats: u32,
}

#[derive(Default)]
struct Record {
    warnings: u64,
    errors: u64,
    recent: VecDeque<Entry>,
}

/// A copy of what was recorded.
#[derive(Clone, Debug, Default)]
pub struct Digest {
    pub warnings: u64,
    pub errors: u64,
    /// The latest entries, newest last.
    pub recent: Vec<Entry>,
}

fn record() -> &'static Mutex<Record> {
    static RECORD: OnceLock<Mutex<Record>> = OnceLock::new();
    RECORD.get_or_init(Mutex::default)
}

/// Records a warning or an error.
pub fn push(severity: Severity, source: &str, text: &str) {
    let Ok(mut r) = record().lock() else { return };
    match severity {
        Severity::Warning => r.warnings += 1,
        Severity::Error => r.errors += 1,
    }
    let text: String = text.chars().take(MAX_TEXT).collect();
    if let Some(last) = r.recent.back_mut().filter(|e| e.severity == severity && e.source == source && e.text == text) {
        last.repeats = last.repeats.saturating_add(1);
        return;
    }
    if r.recent.len() >= KEEP {
        r.recent.pop_front();
    }
    r.recent.push_back(Entry { severity, source: source.to_owned(), text, repeats: 1 });
}

/// What was recorded so far.
pub fn digest() -> Digest {
    match record().lock() {
        Ok(r) => Digest { warnings: r.warnings, errors: r.errors, recent: r.recent.iter().cloned().collect() },
        Err(_) => Digest::default(),
    }
}

/// Forgets everything recorded.
pub fn clear() {
    if let Ok(mut r) = record().lock() {
        *r = Record::default();
    }
}

/// The message of a tracing event.
#[derive(Default)]
struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_owned();
        }
    }
}

struct Capture;

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        // (In tracing a *more verbose* level compares as greater.)
        if level > Level::WARN {
            return;
        }
        let mut message = Message::default();
        event.record(&mut message);
        let severity = if level == Level::ERROR { Severity::Error } else { Severity::Warning };
        push(severity, event.metadata().target(), &message.0);
    }
}

/// For `LogPlugin::custom_layer`: records warnings and errors for the overlay.
pub fn layer(_app: &mut App) -> Option<BoxedLayer> {
    Some(Box::new(Capture))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record is shared by the whole process, so the tests that use it must not overlap.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn warnings_and_errors_are_counted_and_the_latest_are_kept() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        push(Severity::Warning, "test", "careful");
        push(Severity::Error, "test", "broken");
        push(Severity::Error, "test", "broken");
        push(Severity::Error, "test", "broken");
        let d = digest();
        assert_eq!((d.warnings, d.errors), (1, 3));
        assert_eq!(d.recent.len(), 2, "identical entries in a row are folded");
        assert_eq!(d.recent[1].repeats, 3);
        for i in 0..KEEP + 50 {
            push(Severity::Warning, "test", &format!("entry {i}"));
        }
        let d = digest();
        assert_eq!(d.recent.len(), KEEP, "only the latest are kept");
        assert!(d.recent.last().unwrap().text.ends_with(&format!("{}", KEEP + 49)));
        assert_eq!(d.warnings, 1 + (KEEP + 50) as u64, "the counts keep counting");
        clear();
        assert_eq!(digest().warnings, 0);
    }

    #[test]
    fn long_text_is_cut() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        push(Severity::Error, "test", &"x".repeat(5000));
        assert_eq!(digest().recent[0].text.chars().count(), MAX_TEXT);
        clear();
    }

    #[test]
    fn the_layer_catches_warn_and_error_events_only() {
        use bevy::log::tracing_subscriber::{layer::SubscriberExt, Registry};
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        clear();
        let subscriber = Registry::default().with(Capture);
        bevy::log::tracing::subscriber::with_default(subscriber, || {
            bevy::log::info!("fine");
            bevy::log::debug!("chatty");
            bevy::log::warn!("watch out {}", 7);
            bevy::log::error!(target: "script", "it broke");
        });
        let d = digest();
        assert_eq!((d.warnings, d.errors), (1, 1), "{d:?}");
        assert_eq!(d.recent[0].text, "watch out 7");
        assert_eq!((d.recent[1].source.as_str(), d.recent[1].text.as_str()), ("script", "it broke"));
        clear();
    }
}
