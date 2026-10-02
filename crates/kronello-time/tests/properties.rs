use kronello_time::{FrameRate, Rational, SampleRate, Time, TimeMap, TimeMapPoint, TimeRange};
use proptest::prelude::*;

fn rational() -> impl Strategy<Value = Rational> {
    (-10_000_i64..10_000, 1_i64..100).prop_map(|(n, d)| Rational::new(n, d).unwrap())
}

fn r(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
fn t(n: i64) -> Time {
    Time::from_integer(n)
}

fn continuity(num: i64, den: i64, frame: i64, count: i64) -> Result<(), TestCaseError> {
    let rate = FrameRate::new(num, den).unwrap();
    let audio = SampleRate::HZ_48000;
    let mut boundary = audio.samples_for_frame(rate, frame).unwrap().start;
    let start = boundary;
    let mut total = 0;
    for index in frame..frame + count {
        let range = audio.samples_for_frame(rate, index).unwrap();
        prop_assert_eq!(range.start, boundary);
        prop_assert!(range.end >= range.start);
        // An independent integer oracle computes each absolute boundary.
        let expected_start =
            (i128::from(index) * i128::from(den) * 48_000).div_euclid(i128::from(num));
        let expected_end =
            (i128::from(index + 1) * i128::from(den) * 48_000).div_euclid(i128::from(num));
        prop_assert_eq!(i128::from(range.start), expected_start);
        prop_assert_eq!(i128::from(range.end), expected_end);
        let floor_length = (48_000 * den) / num;
        prop_assert!(
            range.end - range.start == floor_length || range.end - range.start == floor_length + 1
        );
        total += range.end - range.start;
        boundary = range.end;
    }
    prop_assert_eq!(total, boundary - start);
    prop_assert_eq!(
        boundary,
        audio
            .sample_floor(rate.frame_to_time(frame + count).unwrap())
            .unwrap()
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn audio_30000_1001_48khz_continuity(frame in -1_000_000_000_i64..1_000_000_000, count in 1_i64..200) {
        continuity(30_000, 1001, frame, count)?;
    }

    #[test]
    fn audio_24_25_30_60000_1001_48khz_continuity(frame in -1_000_000_000_i64..1_000_000_000, count in 1_i64..200) {
        for (num, den) in [(24, 1), (25, 1), (30, 1), (60_000, 1001)] {
            continuity(num, den, frame, count)?;
        }
    }

    #[test]
    fn rational_normalization_and_json_roundtrip(num in any::<i64>(), den in 1_i64..=i64::MAX) {
        let value = r(num, den);
        prop_assert!(value.denominator() > 0);
        prop_assert_eq!(i128::from(value.numerator()) * i128::from(den), i128::from(num) * i128::from(value.denominator()));
        let mut a = value.numerator().unsigned_abs();
        let mut b = value.denominator() as u64;
        while b != 0 { (a, b) = (b, a % b); }
        prop_assert_eq!(a, 1);
        if num == 0 { prop_assert_eq!(value, Rational::ZERO); }
        prop_assert_eq!(serde_json::from_str::<Rational>(&serde_json::to_string(&value).unwrap()).unwrap(), value);
    }

    #[test]
    fn rational_algebra(a in rational(), b in rational(), c in rational()) {
        prop_assert_eq!(a.checked_add(b).unwrap(), b.checked_add(a).unwrap());
        prop_assert_eq!(a.checked_add(b).unwrap().checked_sub(b).unwrap(), a);
        prop_assert_eq!(a.checked_mul(b).unwrap(), b.checked_mul(a).unwrap());
        prop_assert_eq!(a.checked_add(b).unwrap().checked_add(c).unwrap(), a.checked_add(b.checked_add(c).unwrap()).unwrap());
        prop_assert_eq!(a.checked_mul(b.checked_add(c).unwrap()).unwrap(), a.checked_mul(b).unwrap().checked_add(a.checked_mul(c).unwrap()).unwrap());
        prop_assert_eq!(a.checked_neg().unwrap().checked_neg().unwrap(), a);
        if b != Rational::ZERO { prop_assert_eq!(a.checked_mul(b).unwrap().checked_div(b).unwrap(), a); }
    }

    #[test]
    fn rational_order_and_floor_are_exact(a in rational(), b in rational()) {
        prop_assert_eq!(a.cmp(&b), (i128::from(a.numerator()) * i128::from(b.denominator())).cmp(&(i128::from(b.numerator()) * i128::from(a.denominator()))));
        prop_assert!(t(a.floor()) <= a);
        prop_assert!(a < t(a.floor() + 1));
    }

    #[test]
    fn frame_roundtrip_and_subframe_floor(frame in -1_000_000_000_i64..1_000_000_000, part in 0_i64..1000) {
        for (num, den) in [(24, 1), (25, 1), (30, 1), (30_000, 1001), (60_000, 1001)] {
            let rate = FrameRate::new(num, den).unwrap();
            let start = rate.frame_to_time(frame).unwrap();
            let end = rate.frame_to_time(frame + 1).unwrap();
            prop_assert_eq!(rate.time_to_frame(start).unwrap(), t(frame));
            prop_assert_eq!(rate.frame_floor(start).unwrap(), frame);
            let subframe = start.checked_add(end.checked_sub(start).unwrap().checked_mul(r(part, 1000)).unwrap()).unwrap();
            prop_assert_eq!(rate.frame_floor(subframe).unwrap(), frame);
            prop_assert!(rate.frame_range(frame).unwrap().contains(subframe));
            prop_assert!(!rate.frame_range(frame).unwrap().contains(end));
        }
    }

    #[test]
    fn sample_roundtrip(sample in -1_000_000_000_000_i64..1_000_000_000_000, hz in 1_u32..192_001) {
        let rate = SampleRate::new(hz).unwrap();
        prop_assert_eq!(rate.sample_floor(rate.sample_to_time(sample).unwrap()).unwrap(), sample);
    }

    #[test]
    fn range_intersection_is_commutative(a in rational(), b in rational(), c in rational(), d in rational()) {
        let left = TimeRange::new(a.min(b), a.max(b)).unwrap();
        let right = TimeRange::new(c.min(d), c.max(d)).unwrap();
        prop_assert_eq!(left.intersection(right), right.intersection(left));
        prop_assert_eq!(left.is_adjacent_to(right), right.is_adjacent_to(left));
        if let Some(intersection) = left.intersection(right) {
            prop_assert!(left.contains(intersection.start()));
            prop_assert!(right.contains(intersection.start()));
            prop_assert!(!intersection.contains(intersection.end()));
            prop_assert!(intersection.duration().unwrap().as_time() > Rational::ZERO);
        }
    }

    #[test]
    fn linear_map_affinity_and_composition(offset in rational(), speed in 1_i64..100, a in rational(), b in rational()) {
        let map = TimeMap::linear(offset, r(speed, 3)).unwrap();
        let delta = map.map(b).unwrap().checked_sub(map.map(a).unwrap()).unwrap();
        prop_assert_eq!(delta, b.checked_sub(a).unwrap().checked_mul(r(speed, 3)).unwrap());
        let second = TimeMap::linear(t(-3), r(2, 5)).unwrap();
        let composed = TimeMap::linear(t(-3).checked_add(offset.checked_mul(r(2, 5)).unwrap()).unwrap(), r(speed, 3).checked_mul(r(2, 5)).unwrap()).unwrap();
        prop_assert_eq!(second.map(map.map(a).unwrap()).unwrap(), composed.map(a).unwrap());
    }

    #[test]
    fn piecewise_matches_linear_knots_and_interpolation(offset in -1000_i64..1000, speed in 1_i64..100, parent in -10_000_i64..=10_000) {
        let linear = TimeMap::linear(t(offset), r(speed, 3)).unwrap();
        let points = [-10, -3, 0, 7, 10].map(|p| TimeMapPoint { parent: t(p), local: linear.map(t(p)).unwrap() });
        let map = TimeMap::piecewise_linear(points.to_vec()).unwrap();
        for point in points { prop_assert_eq!(map.map(point.parent).unwrap(), point.local); }
        prop_assert_eq!(map.map(r(parent, 1000)).unwrap(), linear.map(r(parent, 1000)).unwrap());
    }

    #[test]
    fn piecewise_continuity_monotonicity_and_order_independence(x in 1_i64..1000, y in 1_i64..1000, z in 1_i64..1000, query in prop::collection::vec(0_i64..=1000, 1..50)) {
        let map = TimeMap::piecewise_linear(vec![
            TimeMapPoint { parent: t(0), local: t(-100) },
            TimeMapPoint { parent: t(1), local: t(-100 + x) },
            TimeMapPoint { parent: t(2), local: t(-100 + x + y) },
            TimeMapPoint { parent: t(3), local: t(-100 + x + y + z) },
        ]).unwrap();
        // Approach each knot from both sides with exact rational deltas.
        for (knot, value, left_speed, right_speed) in [(1, -100 + x, x, y), (2, -100 + x + y, y, z)] {
            prop_assert_eq!(map.map(t(knot).checked_sub(r(1, 1000)).unwrap()).unwrap(), t(value).checked_sub(r(left_speed, 1000)).unwrap());
            prop_assert_eq!(map.map(t(knot).checked_add(r(1, 1000)).unwrap()).unwrap(), t(value).checked_add(r(right_speed, 1000)).unwrap());
        }
        let mut ordered = query.clone();
        ordered.sort_unstable();
        let values: Vec<_> = ordered.iter().map(|q| map.map(r(3 * q, 1000)).unwrap()).collect();
        prop_assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
        for (q, expected) in ordered.iter().zip(&values).rev() { prop_assert_eq!(map.map(r(3 * q, 1000)).unwrap(), *expected); }
        // Generated query order is independent of sorted/reverse evaluation.
        for q in query { prop_assert_eq!(map.map(r(3 * q, 1000)).unwrap(), values[ordered.iter().position(|value| *value == q).unwrap()]); }
    }
}
