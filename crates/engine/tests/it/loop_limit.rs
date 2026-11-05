use serde_json::json;
use soundcraft_engine::Engine;
use soundcraft_model::{ChannelFormat, Clip, ClipId, Session, SourceId, TrackId, TrackKind};

/// One audio clip at (0, 100) on a fresh track.
fn one_clip() -> (Engine, TrackId, ClipId) {
    let mut session = Session::default();
    let track = session.add_track(TrackKind::Audio, ChannelFormat::Mono, None);
    let id = session.new_clip_id();
    session.track_mut(track).unwrap().playlist_mut().unwrap().clips = vec![Clip::audio(id, "c", SourceId(9), 0, 0, 100)];
    session.edit.selected_tracks = vec![track];
    (Engine::new(session), track, id)
}

#[test]
fn count_1001_makes_1000_loops_and_count_5000_is_clamped_to_the_same() {
    let (mut e, t, id) = one_clip();
    let r = e.execute("clip.loop", &json!({"clip": id.0, "count": 1001})).unwrap();
    assert_eq!(r, json!({"loops": 1000}));
    let clips = e.session().track(t).unwrap().clips();
    assert_eq!(clips.len(), 1001);
    assert_eq!(clips.last().unwrap().start, 100_000);
    assert!(e.undo());
    let r = e.execute("clip.loop", &json!({"clip": id.0, "count": 5000})).unwrap();
    assert_eq!(r, json!({"loops": 1000}));
}

#[test]
fn count_512_still_makes_511_loops() {
    let (mut e, _, id) = one_clip();
    let r = e.execute("clip.loop", &json!({"clip": id.0, "count": 512})).unwrap();
    assert_eq!(r, json!({"loops": 511}));
}

#[test]
fn count_0_is_clamped_to_1_and_makes_no_loops() {
    let (mut e, _, id) = one_clip();
    let r = e.execute("clip.loop", &json!({"clip": id.0, "count": 0})).unwrap();
    assert_eq!(r, json!({"loops": 0}));
}
