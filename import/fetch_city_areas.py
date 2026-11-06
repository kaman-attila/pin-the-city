import hashlib
import json
import math
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from shapely.geometry import LineString, MultiPolygon, Point, Polygon
from shapely.ops import polygonize, unary_union

from fetch_fame import run

OVERPASS_SERVERS = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
]
CACHE = "data/overpass_cache"
working_server = 0
CACHED_ONLY = "--cached-only" in sys.argv
USER_AGENT = "PinTheCity-import/1.0 (https://pinthecity.me; contact@pinthecity.me)"
CITIES = "out/largest_cities.tsv"
TARGET = "out/city_areas.sql"
OVERPASS_BATCH = 20
MAX_AREA_KM2 = 1600
MIN_AREA_KM2 = 5
POINT_TOLERANCE_KM = 2
SIMPLIFY_DEGREES = 0.0005
SCALE = 10_000


def read_cities():
    cities = []
    with open(CITIES, encoding="utf-8") as f:
        for line in f:
            city_id, geoname_id, lat, lon, population, name = line.rstrip("\n").split("\t")
            cities.append(
                {"id": int(city_id), "gid": geoname_id, "lat": float(lat), "lon": float(lon), "name": name}
            )
    return cities


def wikidata_links(cities):
    values = " ".join(f'"{c["gid"]}"' for c in cities)
    rows = run(
        "SELECT ?gn ?item ?rel WHERE { "
        f"VALUES ?gn {{ {values} }} "
        "?item wdt:P1566 ?gn . OPTIONAL { ?item wdt:P402 ?rel . } }"
    )
    items, relations = {}, {}
    for r in rows:
        gid = r["gn"]["value"]
        items.setdefault(gid, r["item"]["value"].rsplit("/", 1)[1])
        if "rel" in r:
            relations.setdefault(gid, int(r["rel"]["value"]))
    return items, relations


def overpass(relation_ids=(), wikidata_ids=()):
    parts = []
    if relation_ids:
        parts.append(f"relation(id:{','.join(map(str, relation_ids))});")
    parts.extend(f'relation["wikidata"="{q}"]["boundary"="administrative"];' for q in wikidata_ids)
    query = f"[out:json][timeout:180];({''.join(parts)});out geom;"
    cached = os.path.join(CACHE, hashlib.sha256(query.encode()).hexdigest()[:24] + ".json")
    if os.path.exists(cached):
        with open(cached, encoding="utf-8") as f:
            return json.load(f)
    if CACHED_ONLY:
        return None
    global working_server
    body = urllib.parse.urlencode({"data": query}).encode()
    for attempt in range(12):
        index = (working_server + attempt) % len(OVERPASS_SERVERS)
        server = OVERPASS_SERVERS[index]
        request = urllib.request.Request(server, data=body, headers={"User-Agent": USER_AGENT})
        try:
            with urllib.request.urlopen(request, timeout=300) as response:
                elements = json.load(response)["elements"]
            working_server = index
            os.makedirs(CACHE, exist_ok=True)
            with open(cached, "w", encoding="utf-8") as f:
                json.dump(elements, f)
            return elements
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
            wait = 20 * (attempt // len(OVERPASS_SERVERS) + 1)
            print(f"  {server.split('/')[2]} failed ({e}), trying the next server in {wait}s")
            time.sleep(wait)
    raise RuntimeError("overpass kept failing")


def relation_shape(element):
    lines = {"outer": [], "inner": []}
    for member in element.get("members", []):
        if member.get("type") != "way" or "geometry" not in member:
            continue
        role = "inner" if member.get("role") == "inner" else "outer"
        points = [(p["lon"], p["lat"]) for p in member["geometry"]]
        if len(points) >= 2:
            lines[role].append(LineString(points))
    outer = unary_union(list(polygonize(lines["outer"])))
    if outer.is_empty:
        return None
    inner = unary_union(list(polygonize(lines["inner"])))
    shape = outer.difference(inner) if not inner.is_empty else outer
    return shape if not shape.is_empty else None


def area_km2(shape, lat):
    km_per_lon = 111.32 * math.cos(math.radians(lat))
    return shape.area * km_per_lon * 110.57


def km_between(shape, lon, lat):
    km_per_lon = 111.32 * math.cos(math.radians(lat))
    return shape.distance(Point(lon, lat)) * min(km_per_lon, 110.57)


def varint(value):
    out = bytearray()
    while value >= 0x80:
        out.append((value & 0x7F) | 0x80)
        value >>= 7
    out.append(value)
    return out


def signed(value):
    return varint((value << 1) ^ (value >> 63))


def encode(shape):
    polygons = list(shape.geoms) if isinstance(shape, MultiPolygon) else [shape]
    rings = []
    for polygon in polygons:
        if isinstance(polygon, Polygon):
            rings.append(polygon.exterior.coords)
            rings.extend(interior.coords for interior in polygon.interiors)
    out = varint(len(rings))
    for ring in rings:
        points = list(ring)
        out += varint(len(points))
        x = y = 0
        for lon, lat in points:
            nx, ny = round(lon * SCALE), round(lat * SCALE)
            out += signed(nx - x) + signed(ny - y)
            x, y = nx, ny
    return bytes(out)


def main():
    cities = read_cities()
    items, relations = {}, {}
    for start in range(0, len(cities), 300):
        batch_items, batch_relations = wikidata_links(cities[start : start + 300])
        items.update(batch_items)
        relations.update(batch_relations)
        time.sleep(1)
    print(
        f"{len(cities)} cities, {len(items)} on Wikidata, "
        f"{len(relations)} with a boundary link, the rest looked up by Wikidata tag"
    )

    wanted = [c for c in cities if c["gid"] in items]
    shapes = {}
    by_item = {}
    downloaded = 0
    for start in range(0, len(wanted), OVERPASS_BATCH):
        batch = wanted[start : start + OVERPASS_BATCH]
        ids = sorted({relations[c["gid"]] for c in batch if c["gid"] in relations})
        tagged = sorted({items[c["gid"]] for c in batch if c["gid"] not in relations})
        elements = overpass(ids, tagged)
        if elements is None:
            break
        for element in elements:
            if element.get("type") != "relation":
                continue
            shapes[element["id"]] = relation_shape(element)
            q = element.get("tags", {}).get("wikidata")
            if q:
                by_item.setdefault(q, []).append(element["id"])
        downloaded = min(start + OVERPASS_BATCH, len(wanted))
        print(f"  {downloaded}/{len(wanted)} cities looked up")
        if not CACHED_ONLY:
            time.sleep(5)

    if CACHED_ONLY:
        wanted = wanted[:downloaded]
        print(f"using the {len(wanted)} cities already downloaded")
    rows, reasons = [], {}
    for c in wanted:
        if c["gid"] in relations:
            candidates = [relations[c["gid"]]]
        else:
            candidates = by_item.get(items[c["gid"]], [])
        usable = [shapes[r] for r in candidates if shapes.get(r) is not None]
        usable = [s for s in usable if km_between(s, c["lon"], c["lat"]) <= POINT_TOLERANCE_KM]
        shape = min(usable, key=lambda s: s.area) if usable else (shapes.get(candidates[0]) if candidates else None)
        if shape is None:
            reason = "no usable shape"
        elif km_between(shape, c["lon"], c["lat"]) > POINT_TOLERANCE_KM:
            reason = "city point outside the boundary"
        elif area_km2(shape, c["lat"]) > MAX_AREA_KM2:
            reason = f"bigger than {MAX_AREA_KM2} km2"
        elif area_km2(shape, c["lat"]) < MIN_AREA_KM2:
            reason = f"smaller than {MIN_AREA_KM2} km2"
        else:
            simple = shape.simplify(SIMPLIFY_DEGREES, preserve_topology=True)
            rows.append((c["id"], encode(simple), c["name"], round(area_km2(shape, c["lat"]))))
            continue
        reasons.setdefault(reason, []).append(c["name"])

    with open(TARGET, "w", encoding="utf-8") as f:
        f.write("DELETE FROM city_areas;\n")
        for city_id, data, _, _ in rows:
            f.write(f"INSERT INTO city_areas (city_id, shape) VALUES ({city_id}, X'{data.hex()}');\n")

    total = sum(len(d) for _, d, _, _ in rows)
    print(f"kept {len(rows)} city areas, {total // 1024} KB in total, written to {TARGET}")
    for reason, names in reasons.items():
        print(f"left out ({reason}): {len(names)}, e.g. {', '.join(names[:12])}")
    print("examples:", ", ".join(f"{n} {a} km2" for _, _, n, a in rows[:15]))


if __name__ == "__main__":
    main()
