use std::sync::OnceLock;

const DATA: &[u8] = include_bytes!("../data/borders.bin");
const SCALE: f64 = 10_000.0;
const COAST_KM: f64 = 15.0;
const KM_PER_LAT: f64 = 110.57;

struct Polygon {
    code: [u8; 2],
    min: (f64, f64),
    max: (f64, f64),
    rings: Vec<Vec<(f64, f64)>>,
}

impl Polygon {
    fn contains(&self, lon: f64, lat: f64) -> bool {
        if lon < self.min.0 || lon > self.max.0 || lat < self.min.1 || lat > self.max.1 {
            return false;
        }
        self.rings
            .iter()
            .filter(|ring| inside_ring(lon, lat, ring))
            .count()
            % 2
            == 1
    }
}

fn inside_ring(lon: f64, lat: f64, ring: &[(f64, f64)]) -> bool {
    let mut hit = false;
    let mut j = ring.len().wrapping_sub(1);
    for (i, &(xi, yi)) in ring.iter().enumerate() {
        let (xj, yj) = ring[j];
        if (yi > lat) != (yj > lat) && lon < (xj - xi) * (lat - yi) / (yj - yi) + xi {
            hit = !hit;
        }
        j = i;
    }
    hit
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        let slice = self.data.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(slice)
    }

    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.bytes(1)?.first()?;
            value |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn signed(&mut self) -> Option<i64> {
        let v = self.varint()?;
        Some((v >> 1) as i64 ^ -((v & 1) as i64))
    }
}

type Rings = Vec<Vec<(f64, f64)>>;

fn read_rings(r: &mut Reader) -> Option<Rings> {
    let ring_count = r.varint()?;
    let mut rings = Vec::new();
    for _ in 0..ring_count {
        let points = r.varint()?;
        let mut ring = Vec::with_capacity(points.min(100_000) as usize);
        let (mut x, mut y) = (0i64, 0i64);
        for _ in 0..points {
            x += r.signed()?;
            y += r.signed()?;
            ring.push((x as f64 / SCALE, y as f64 / SCALE));
        }
        rings.push(ring);
    }
    Some(rings)
}

pub fn distance_to_area_m(lat: f64, lon: f64, area: &[u8]) -> Option<f64> {
    let mut r = Reader { data: area, pos: 0 };
    let rings = read_rings(&mut r)?;
    if r.pos != area.len() || rings.is_empty() {
        return None;
    }
    let inside = rings
        .iter()
        .filter(|ring| inside_ring(lon, lat, ring))
        .count()
        % 2
        == 1;
    if inside {
        return Some(0.0);
    }
    rings
        .iter()
        .flatten()
        .map(|&(x, y)| crate::game::distance_in_meters(lat, lon, y, x))
        .min_by(f64::total_cmp)
}

fn parse(data: &[u8]) -> Option<Vec<Polygon>> {
    let mut r = Reader { data, pos: 0 };
    if r.bytes(4)? != b"BRD2" {
        return None;
    }
    let count = u16::from_le_bytes(r.bytes(2)?.try_into().ok()?);
    let mut polygons = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let code: [u8; 2] = r.bytes(2)?.try_into().ok()?;
        let rings = read_rings(&mut r)?;
        let (mut min, mut max) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for &point in rings.iter().flatten() {
            min = (min.0.min(point.0), min.1.min(point.1));
            max = (max.0.max(point.0), max.1.max(point.1));
        }
        polygons.push(Polygon {
            code,
            min,
            max,
            rings,
        });
    }
    (r.pos == data.len()).then_some(polygons)
}

fn polygons() -> &'static [Polygon] {
    static POLYGONS: OnceLock<Vec<Polygon>> = OnceLock::new();
    POLYGONS.get_or_init(|| parse(DATA).unwrap_or_default())
}

fn distance_to_segment_km(lon: f64, lat: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    let km_per_lon = 111.32 * lat.to_radians().cos();
    let (px, py) = (lon * km_per_lon, lat * KM_PER_LAT);
    let (ax, ay) = (a.0 * km_per_lon, a.1 * KM_PER_LAT);
    let (bx, by) = (b.0 * km_per_lon, b.1 * KM_PER_LAT);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
}

fn nearest_within(lon: f64, lat: f64, max_km: f64) -> Option<&'static Polygon> {
    let margin = max_km / KM_PER_LAT * 2.0;
    polygons()
        .iter()
        .filter(|p| {
            lon >= p.min.0 - margin
                && lon <= p.max.0 + margin
                && lat >= p.min.1 - margin
                && lat <= p.max.1 + margin
        })
        .filter_map(|p| {
            let distance = p
                .rings
                .iter()
                .flat_map(|ring| ring.windows(2))
                .map(|edge| distance_to_segment_km(lon, lat, edge[0], edge[1]))
                .fold(f64::MAX, f64::min);
            (distance <= max_km).then_some((p, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

pub fn country_at(lat: f64, lon: f64) -> Option<&'static str> {
    polygons()
        .iter()
        .find(|p| p.contains(lon, lat))
        .or_else(|| nearest_within(lon, lat, COAST_KM))
        .and_then(|p| std::str::from_utf8(&p.code).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borders_file_parses_completely() {
        assert!(parse(DATA).is_some_and(|p| p.len() > 1000));
    }

    #[test]
    fn finds_the_country_of_well_known_places() {
        for (lat, lon, code) in [
            (47.4979, 19.0402, "HU"),
            (48.8566, 2.3522, "FR"),
            (40.7128, -74.0060, "US"),
            (55.7558, 37.6173, "RU"),
            (-33.8688, 151.2093, "AU"),
            (-29.3151, 27.4869, "LS"),
            (-26.2041, 28.0473, "ZA"),
            (4.9224, -52.3135, "GF"),
            (-20.8823, 55.4504, "RE"),
            (44.6166, 33.5254, "UA"),
            (35.1856, 33.3823, "CY"),
            (9.5600, 44.0650, "SO"),
        ] {
            assert_eq!(country_at(lat, lon), Some(code), "({lat}, {lon})");
        }
    }

    #[test]
    fn coastal_spots_just_outside_the_simplified_border_still_count() {
        assert_eq!(country_at(40.7000, -74.0150), Some("US"));
        assert_eq!(country_at(43.2950, 5.3600), Some("FR"));
    }

    #[test]
    #[ignore]
    fn cities_are_found_in_their_own_country() {
        let text = std::fs::read_to_string("../import/data/cities5000.txt").unwrap();
        let (mut total, mut right, mut nowhere) = (0, 0, 0);
        for line in text.lines() {
            let c: Vec<&str> = line.split('\t').collect();
            if c[7] == "PPLX" {
                continue;
            }
            total += 1;
            match country_at(c[4].parse().unwrap(), c[5].parse().unwrap()) {
                Some(code) if code == c[8] => right += 1,
                None => nowhere += 1,
                _ => {}
            }
        }
        println!("{right}/{total} in their own country, {nowhere} outside every border");
        assert!(right as f64 / total as f64 > 0.98);
    }

    #[test]
    fn open_sea_has_no_country() {
        assert_eq!(country_at(-30.0, -20.0), None);
        assert_eq!(country_at(35.0, -40.0), None);
    }
}
