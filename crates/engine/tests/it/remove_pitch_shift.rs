use serde_json::json;
use soundcraft_audio_io::{AudioBuffer, FileFormat};
use soundcraft_engine::{Engine, io};
use soundcraft_model::{ChannelFormat, Clip, ClipId, Session, SourceId, TrackKind};
use soundcraft_time::SampleRate;

/// Two stretched (1.5) audio clips of one source on one track: `a` at 0, `b` at 40_000 with the
/// given length; `b` is edit-locked when `lock_b` is set. Returns (engine, a, b, source).
fn fixture(b_length: i64, lock_b: bool) -> (Engine, ClipId, ClipId, SourceId) {
    let mut s = Session::new("Stretched", SampleRate::new(48_000).unwrap());
    let t = s.add_track(TrackKind::Audio, ChannelFormat::Mono, None);
    let tone: Vec<f32> = (0..12_000).map(|i| (i as f32 * 0.07).sin() * 0.5).collect();
    let src = io::add_source(&mut s, "tone", AudioBuffer { sample_rate: 48_000, channels: vec![tone] }, None, FileFormat::Wav);
    let a = s.new_clip_id();
    let b = s.new_clip_id();
    let mut ca = Clip::audio(a, "a", src, 0, 0, 18_000);
    ca.stretch = 1.5;
    let mut cb = Clip::audio(b, "b", src, 0, 40_000, b_length);
    cb.stretch = 1.5;
    cb.edit_locked = lock_b;
    s.track_mut(t).unwrap().playlist_mut().unwrap().clips = vec![ca, cb];
    s.edit.selected_tracks = vec![t];
    (Engine::new(s), a, b, src)
}

#[test]
fn a_locked_clip_is_left_unchanged() {
    let (mut e, a, b, src) = fixture(18_000, true);
    let r = e.execute("clip.remove_pitch_shift", &json!({"clips": [a.0, b.0]})).unwrap();
    assert_eq!(r, json!({"clips": 1}));
    let ca = e.session().find_clip(a).unwrap().1.clone();
    assert_eq!(ca.stretch, 1.0);
    assert_eq!(ca.length, 18_000);
    assert_ne!(ca.source(), Some(src));
    let cb = e.session().find_clip(b).unwrap().1.clone();
    assert_eq!(cb.stretch, 1.5);
    assert_eq!(cb.length, 18_000);
    assert_eq!(cb.source(), Some(src));
}

#[test]
fn a_clip_over_the_render_limit_is_skipped_and_the_other_clip_still_processes() {
    // One hour at 48 kHz is 172_800_000 samples; 200_000_000 is over it.
    let (mut e, a, b, src) = fixture(200_000_000, false);
    let r = e.execute("clip.remove_pitch_shift", &json!({"clips": [a.0, b.0]})).unwrap();
    assert_eq!(r, json!({"clips": 1}));
    let ca = e.session().find_clip(a).unwrap().1.clone();
    assert_eq!(ca.stretch, 1.0);
    assert_ne!(ca.source(), Some(src));
    let cb = e.session().find_clip(b).unwrap().1.clone();
    assert_eq!(cb.stretch, 1.5);
    assert_eq!(cb.length, 200_000_000);
    assert_eq!(cb.source(), Some(src));
}
