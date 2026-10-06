use kronello_time::{Duration, ProtectedMiddleMode, Time, TimeMap, TimeMapPoint};
#[test]
fn simulation_canonical_clock_inverts_exact_affine_piecewise_and_protected() {
    let affine = TimeMap::linear(Time::new(-1, 3).unwrap(), Time::new(5, 2).unwrap()).unwrap();
    let piecewise = TimeMap::piecewise_linear(vec![
        TimeMapPoint {
            parent: Time::ZERO,
            local: Time::ZERO,
        },
        TimeMapPoint {
            parent: Time::ONE,
            local: Time::new(2, 1).unwrap(),
        },
        TimeMapPoint {
            parent: Time::new(3, 1).unwrap(),
            local: Time::new(5, 1).unwrap(),
        },
    ])
    .unwrap();
    for map in [affine, piecewise] {
        for parent in [
            Time::ZERO,
            Time::new(1, 2).unwrap(),
            Time::ONE,
            Time::new(3, 1).unwrap(),
        ] {
            let q = map.map(parent).unwrap();
            assert_eq!(map.inverse_canonical(q).unwrap(), parent);
        }
    }
    for mode in [ProtectedMiddleMode::Loop, ProtectedMiddleMode::Hold] {
        let map = TimeMap::protected(
            Duration::new(Time::new(4, 1).unwrap()).unwrap(),
            Duration::new(Time::new(8, 1).unwrap()).unwrap(),
            Duration::new(Time::ONE).unwrap(),
            Duration::new(Time::ONE).unwrap(),
            mode,
        )
        .unwrap();
        let canonical = map.canonical_source_clock().unwrap();
        for q in [Time::ZERO, Time::ONE, Time::new(3, 1).unwrap()] {
            assert_eq!(canonical.map(q).unwrap(), q);
            assert_eq!(map.inverse_canonical(q).unwrap(), q);
        }
    }
}
