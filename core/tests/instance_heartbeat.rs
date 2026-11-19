use std::time::{Duration, SystemTime, UNIX_EPOCH};

use audiocurve_core::heartbeat::InstanceHeartbeat;

fn at(secs: f64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs_f64(secs)
}

fn beat(pid: u32, host: &str, active: bool, time: SystemTime) -> InstanceHeartbeat {
    InstanceHeartbeat { pid, host_bundle_id: host.into(), active, time }
}

#[test]
fn stale_and_inactive_beats_are_ignored() {
    let now = at(1000.0);
    let beats = [
        beat(1, "com.rogueamoeba.soundsource", true, at(998.0)),
        beat(2, "com.example.old", true, at(993.0)),
        beat(3, "com.example.idle", false, now),
        beat(4, "se.lindefors.audiocurve", true, now),
    ];
    let live: Vec<u32> = InstanceHeartbeat::live(&beats, now, 4).iter().map(|b| b.pid).collect();
    assert_eq!(live, [1]);
}

#[test]
fn live_beats_are_sorted_by_host() {
    let now = at(1000.0);
    let beats = [beat(1, "com.z", true, now), beat(2, "com.a", true, now)];
    let hosts: Vec<String> = InstanceHeartbeat::live(&beats, now, 9).into_iter().map(|b| b.host_bundle_id).collect();
    assert_eq!(hosts, ["com.a", "com.z"]);
}

#[path = "support/mod.rs"]
mod support;

#[test]
fn heartbeats_round_trip_through_the_store() {
    let (dir, store) = support::temp_store();
    let t = at(1000.0);
    store.write_heartbeat(&beat(10, "a", true, t));
    store.write_heartbeat(&beat(11, "b", false, t));
    store.remove_heartbeat(10);
    std::fs::write(dir.path().join("instances/garbage.json"), b"garbage").unwrap();
    assert_eq!(store.read_heartbeats(), [beat(11, "b", false, t)]);
}

#[test]
fn prune_removes_long_dead_beats() {
    let (_dir, store) = support::temp_store();
    let now = at(10_000.0);
    store.write_heartbeat(&beat(1, "a", true, at(10_000.0 - 61.0)));
    store.write_heartbeat(&beat(2, "b", true, at(10_000.0 - 10.0)));
    store.prune_heartbeats(now);
    assert_eq!(store.read_heartbeats().iter().map(|b| b.pid).collect::<Vec<_>>(), [2]);
}
