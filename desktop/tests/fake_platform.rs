use audiocurve_app::engine::{Engine, SharedEngine};
use audiocurve_app::platform::PlatformEvent;
use audiocurve_app::platform::fake::fake;
use audiocurve_dsp::Params;
use std::{cell::RefCell, rc::Rc, sync::Arc};

fn shared() -> SharedEngine {
    let e = Engine::new(48_000.0, 4096);
    e.set_params(&Params::unity());
    Rc::new(RefCell::new(Arc::new(e)))
}

#[test]
fn fake_lists_the_demo_outputs() {
    let (p, _c) = fake();
    let outs = p.audio.outputs();
    assert_eq!(outs.len(), 3);
    assert_eq!(outs[0].name, "AirPods Pro");
    assert_eq!(p.audio.current_output().unwrap().uid, "airpods");
    let uids: Vec<_> = outs.iter().map(|o| o.uid.as_str()).collect();
    assert_eq!(uids, ["airpods", "aids", "speakers"]);
}

#[test]
fn fake_route_builds_tears_down_and_fails_on_request() {
    let (p, c) = fake();
    let mut b = p.audio.route_builder(shared());
    let r = b.build("airpods").unwrap();
    assert_eq!(c.route_log(), ["build:airpods"]);
    r.tear_down();
    assert_eq!(c.route_log(), ["build:airpods", "teardown:airpods"]);
    c.fail_next_builds(1);
    assert!(b.build("airpods").is_err());
    assert!(b.build("airpods").is_ok());
}

#[test]
fn pumping_a_running_route_moves_the_meters() {
    let (p, c) = fake();
    let engine = shared();
    let arc = engine.borrow().clone();
    let mut b = p.audio.route_builder(engine);
    let _route = b.build("airpods").unwrap();
    c.pump(48_000);
    let m = arc.meters();
    assert!(m.in_rms_db[0] > -40.0, "{:?}", m.in_rms_db);
    assert!(m.out_rms_db[0] > -40.0, "{:?}", m.out_rms_db);
}

#[test]
fn emitted_events_are_polled_once() {
    let (mut p, c) = fake();
    c.emit(PlatformEvent::OutputsChanged);
    assert_eq!(p.audio.poll_events(), [PlatformEvent::OutputsChanged]);
    assert!(p.audio.poll_events().is_empty());
}
