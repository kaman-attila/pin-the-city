use sha2::{Digest, Sha256};

const EARTH_RADIUS_IN_METERS: f64 = 6371000.0;
const MAX_POINTS: f64 = 1000.0;
const FULL_POINTS_RADIUS_IN_METERS: f64 = 10000.0;
const DECAY_IN_METERS: f64 = 1500000.0;
pub const TOTAL_NAMES: u32 = 59297;
pub const BUCKET_SIZE: u32 = 250;
pub const BUCKETS: u32 = 20;
pub const ARCHIVE_START: &str = "2026-10-21";
pub const WORLD: &str = "world";
pub const REGIONS: [&str; 7] = [WORLD, "EU", "AS", "AF", "NA", "SA", "OC"];
pub const LEVELS: std::ops::RangeInclusive<u32> = 1..=5;
pub const FAME_START: &str = "2026-10-23";
pub const COUNTRY_BONUS_START: &str = "2026-10-23";
const COUNTRY_BONUS_SHARE: f64 = 0.3;

pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn score(distance: f64) -> u32 {
    if distance <= FULL_POINTS_RADIUS_IN_METERS {
        return MAX_POINTS as u32;
    }

    (MAX_POINTS * (-(distance - FULL_POINTS_RADIUS_IN_METERS) / DECAY_IN_METERS).exp()).round()
        as u32
}

pub fn with_country_bonus(score: u32) -> u32 {
    let gap = MAX_POINTS - score as f64;
    score + (gap * COUNTRY_BONUS_SHARE).round() as u32
}

pub fn distance_in_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let fi1 = lat1.to_radians();
    let fi2 = lat2.to_radians();
    let delta_fi: f64 = fi2 - fi1;
    let delta_lambda: f64 = lon2.to_radians() - lon1.to_radians();
    let a: f64 =
        (delta_fi / 2.0).sin().powi(2) + fi1.cos() * fi2.cos() * (delta_lambda / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_IN_METERS * a.sqrt().asin()
}

pub fn id_range_for_round(round: u32) -> Option<(u32, u32)> {
    match round {
        1 => Some((1, 200)),
        2 => Some((201, 1000)),
        3 => Some((1001, 5000)),
        4 => Some((5001, 10000)),
        5 => Some((10001, TOTAL_NAMES / 2)),
        _ => None,
    }
}

pub fn daily_seed(secret: &str, date: &str) -> u64 {
    let hash = Sha256::digest(format!("{secret}:{date}"));
    u64::from_le_bytes(hash[..8].try_into().unwrap())
}

pub fn daily_name_ids(seed: u64) -> [u32; 5] {
    let mut rng = SplitMix64::new(seed);
    let mut ids = [0u32; 5];
    for round in 1..=5 {
        let (min, max) = id_range_for_round(round).unwrap();
        let count = (max - min + 1) as u64;
        let id = loop {
            let id = min + (rng.next_u64() % count) as u32;
            if !ids.contains(&id) {
                break id;
            }
        };
        ids[round as usize - 1] = id;
    }
    ids
}

pub fn daily_draws(seed: u64) -> [f64; 5] {
    let mut rng = SplitMix64::new(seed);
    std::array::from_fn(|_| rng.next_f64())
}

pub fn parse_name_ids(text: &str) -> Option<[u32; 5]> {
    let ids: Vec<u32> = text
        .split(',')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    ids.try_into().ok()
}

pub fn format_name_ids(ids: &[u32; 5]) -> String {
    ids.map(|id| id.to_string()).join(",")
}

pub fn current_streak(on_time_dates_newest_first: &[&str], today: &str) -> u32 {
    let (Some(today), Some(first)) = (day_from_date(today), day_from_date(ARCHIVE_START)) else {
        return 0;
    };
    let days: Vec<i64> = on_time_dates_newest_first
        .iter()
        .filter_map(|d| day_from_date(d))
        .filter(|&day| day >= first)
        .collect();
    let Some(&newest) = days.first() else {
        return 0;
    };
    if newest < today - 1 {
        return 0;
    }
    let mut expected = newest;
    let mut streak = 0;
    for day in days {
        if day != expected {
            break;
        }
        streak += 1;
        expected -= 1;
    }
    streak
}

pub fn bucket_for(score: u32) -> u32 {
    (score / BUCKET_SIZE).min(BUCKETS - 1)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn date_from_day(day: i64) -> String {
    let z = day + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn day_from_date(date: &str) -> Option<i64> {
    let bytes = date.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year = date[..4].parse::<i64>().ok()?;
    let month = date[5..7].parse::<i64>().ok()?;
    let day = date[8..].parse::<i64>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    (date_from_day(days) == date).then_some(days)
}

pub fn daily_number(date: &str) -> Option<u32> {
    let day = day_from_date(date)?;
    let start = day_from_date(ARCHIVE_START)?;
    (day >= start).then(|| (day - start + 1) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vienna_to_budapest() {
        let d = distance_in_meters(48.2085, 16.3721, 47.4979, 19.0402);
        assert!((d - 214_200.0).abs() < 1_000.0, "got {d}");
    }

    #[test]
    fn same_point_is_zero() {
        assert!(distance_in_meters(10.0, 20.0, 10.0, 20.0) < 0.001);
    }

    #[test]
    fn full_points_inside_radius() {
        assert_eq!(score(0.0), 1000);
        assert_eq!(score(300.0), 1000);
    }

    #[test]
    fn country_bonus_closes_part_of_the_gap() {
        assert_eq!(with_country_bonus(1000), 1000);
        assert_eq!(with_country_bonus(0), 300);
        assert_eq!(with_country_bonus(136), 395);
        assert_eq!(with_country_bonus(911), 938);
        for score in 0..=1000 {
            let boosted = with_country_bonus(score);
            assert!(boosted >= score && boosted <= 1000, "{score} -> {boosted}");
        }
    }

    #[test]
    fn daily_name_ids_are_unique_and_in_range() {
        for seed in 0..1000 {
            let ids = daily_name_ids(seed);
            for (i, &id) in ids.iter().enumerate() {
                let (min, max) = id_range_for_round(i as u32 + 1).unwrap();
                assert!(
                    (min..=max).contains(&id),
                    "seed {seed}: id {id} out of range"
                );
                assert!(!ids[..i].contains(&id), "seed {seed}: duplicate id {id}");
            }
        }
    }

    #[test]
    fn round_ranges_get_harder_without_gaps() {
        let mut next_min = 1;
        for round in 1..=5 {
            let (min, max) = id_range_for_round(round).unwrap();
            assert_eq!(
                min,
                next_min,
                "round {round} should start right after round {}",
                round - 1
            );
            assert!(min <= max, "round {round} range is empty");
            next_min = max + 1;
        }
        assert!(next_min - 1 <= TOTAL_NAMES);
        assert_eq!(id_range_for_round(0), None);
        assert_eq!(id_range_for_round(6), None);
    }

    #[test]
    fn daily_name_ids_are_deterministic() {
        assert_eq!(daily_name_ids(42), daily_name_ids(42));
    }

    #[test]
    fn daily_draws_are_deterministic_and_in_range() {
        for seed in 0..1000 {
            let draws = daily_draws(seed);
            assert_eq!(draws, daily_draws(seed));
            assert!(
                draws.iter().all(|d| (0.0..1.0).contains(d)),
                "seed {seed}: {draws:?}"
            );
        }
    }

    #[test]
    fn name_ids_round_trip_as_text() {
        let ids = [12, 345, 6789, 10, 29000];
        assert_eq!(format_name_ids(&ids), "12,345,6789,10,29000");
        assert_eq!(parse_name_ids(&format_name_ids(&ids)), Some(ids));
        for bad in ["", "1,2,3,4", "1,2,3,4,5,6", "1,2,x,4,5", "1,,3,4,5"] {
            assert_eq!(parse_name_ids(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn streak_counts_days_in_a_row() {
        let start = day_from_date(ARCHIVE_START).unwrap();
        let day = |n: i64| date_from_day(start + n);
        let (d0, d1, d2, d3, d10) = (day(0), day(1), day(2), day(3), day(10));
        assert_eq!(current_streak(&[], &d3), 0);
        assert_eq!(current_streak(&[&d3], &d3), 1);
        assert_eq!(current_streak(&[&d3, &d2, &d1], &d3), 3);
        assert_eq!(current_streak(&[&d2, &d1], &d3), 2);
        assert_eq!(current_streak(&[&d1, &d0], &d3), 0);
        assert_eq!(current_streak(&[&d3, &d1, &d0], &d3), 1);
        assert_eq!(current_streak(&[&d10, &day(9), &day(8)], &d10), 3);
    }

    #[test]
    fn streak_ignores_days_before_the_first_daily() {
        let start = day_from_date(ARCHIVE_START).unwrap();
        let day = |n: i64| date_from_day(start + n);
        assert_eq!(
            current_streak(&[&day(1), &day(0), &day(-1), &day(-2)], &day(1)),
            2
        );
        assert_eq!(current_streak(&[&day(-1), &day(-2)], &day(0)), 0);
    }

    #[test]
    fn daily_seeds_are_different() {
        assert!(daily_seed("s", "2026-10-15") != daily_seed("s", "2026-10-16"));
    }

    #[test]
    fn dates_convert_both_ways() {
        assert_eq!(day_from_date("1970-01-01"), Some(0));
        assert_eq!(date_from_day(0), "1970-01-01");
        for date in [
            "2024-02-29",
            "2026-10-21",
            "2026-12-31",
            "2027-01-01",
            "2100-03-01",
        ] {
            let day = day_from_date(date).unwrap();
            assert_eq!(date_from_day(day), date);
        }
        assert_eq!(
            day_from_date("2027-01-01").unwrap() - day_from_date("2026-12-31").unwrap(),
            1
        );
    }

    #[test]
    fn broken_dates_are_rejected() {
        for date in [
            "",
            "2026-1-01",
            "2026-02-30",
            "2025-02-29",
            "2026-13-01",
            "abcd-ef-gh",
            "2026/10/21",
        ] {
            assert_eq!(day_from_date(date), None, "{date:?} should be rejected");
        }
    }

    #[test]
    fn daily_numbers_count_from_the_archive_start() {
        assert_eq!(daily_number(ARCHIVE_START), Some(1));
        let start = day_from_date(ARCHIVE_START).unwrap();
        assert_eq!(daily_number(&date_from_day(start + 9)), Some(10));
        assert_eq!(daily_number(&date_from_day(start - 1)), None);
    }
}
