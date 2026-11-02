use serde_json::json;
use soundcraft_engine::Engine;
use soundcraft_model::{ChannelFormat, Clip, ClipId, Session, SourceId, TrackKind};
use soundcraft_time::SampleRate;

/// One 1000-sample audio clip at 0 at 120 bpm, conformed once (the first conform records its
/// tempo and length), then the session tempo doubles to 240 and it is conformed again.
fn conformed_at_double_tempo() -> (Engine, ClipId) {
    let mut s = Session::new("Conform", SampleRate::new(48_000).unwrap());
    let t = s.add_track(TrackKind::Audio, ChannelFormat::Mono, None);
    let id = s.new_clip_id();
    s.track_mut(t).unwrap().playlist_mut().unwrap().clips = vec![Clip::audio(id, "c", SourceId(9), 0, 0, 1000)];
    s.edit.selected_tracks = vec![t];
    let mut e = Engine::new(s);
    e.execute("clip.conform_to_tempo", &json!({"clip": id.0})).unwrap();
    e.execute("event.tempo", &json!({"bpm": 240.0})).unwrap();
    e.execute("clip.conform_to_tempo", &json!({"clip": id.0})).unwrap();
    (e, id)
}

#[test]
fn conforming_again_without_an_edit_keeps_the_conformed_length() {
    let (mut e, id) = conformed_at_double_tempo();
    let c = e.session().find_clip(id).unwrap().1.clone();
    assert_eq!(c.length, 500);
    assert!((c.stretch - 0.5).abs() < 1e-9);
    e.execute("clip.conform_to_tempo", &json!({"clip": id.0})).unwrap();
    let c = e.session().find_clip(id).unwrap().1.clone();
    assert_eq!(c.length, 500);
    assert!((c.stretch - 0.5).abs() < 1e-9);
}

#[test]
fn conforming_after_a_trim_follows_the_trimmed_length() {
    let (mut e, id) = conformed_at_double_tempo();
    e.session_mut().find_clip_mut(id).unwrap().length = 400;
    e.execute("clip.conform_to_tempo", &json!({"clip": id.0})).unwrap();
    let c = e.session().find_clip(id).unwrap().1.clone();
    assert_eq!(c.length, 400);
    assert!((c.stretch - 0.5).abs() < 1e-9);
}
