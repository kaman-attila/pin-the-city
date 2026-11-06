import collections
import concurrent.futures
import difflib
import json
import math
import os
import re
import sys
import threading
import time
import unicodedata
import urllib.error
import urllib.request

import mapbox_vector_tile

TILEJSON = "https://tiles.openfreemap.org/planet"
USER_AGENT = "PinTheCity-import/1.0 (https://pinthecity.me; contact@pinthecity.me)"
CITIES = "out/round_cities.tsv"
GEONAMES = "data/cities5000.txt"
TARGET = "data/map_names.tsv"
RETRY_TARGET = "data/map_names_retry.tsv"
WORKERS = 4
PASSES = [(10, 12), (8, 25)]
MIN_SIMILARITY = 0.85
NEARBY_KM = 3
NEARBY_SIMILARITY = 0.6
NEARBY_CLASSES = {"city", "town", "village"}
FILLER_WORDS = {"city", "of", "village", "town", "municipality", "shi", "si", "gun"}


def tile_for(lat, lon, z):
    n = 2**z
    x = int((lon + 180) / 360 * n)
    y = int((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n)
    return x, y


def to_latlon(z, x, y, px, py, extent):
    n = 2**z
    lon = (x + px / extent) / n * 360 - 180
    lat = math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * (y + py / extent) / n))))
    return lat, lon


def km(a, b):
    p1, p2 = math.radians(a[0]), math.radians(b[0])
    dp, dl = p2 - p1, math.radians(b[1] - a[1])
    h = math.sin(dp / 2) ** 2 + math.cos(p1) * math.cos(p2) * math.sin(dl / 2) ** 2
    return 12742 * math.asin(math.sqrt(h))


def map_label(props):
    if props.get("name:nonlatin"):
        return props.get("name:latin")
    return props.get("name_en") or props.get("name")


def simplify(text):
    text = unicodedata.normalize("NFKD", text)
    text = "".join(ch for ch in text if not unicodedata.combining(ch)).lower()
    return re.sub(r"[^a-z]", "", text)


def words(text):
    text = unicodedata.normalize("NFKD", text)
    text = "".join(ch for ch in text if not unicodedata.combining(ch)).lower()
    return re.findall(r"[a-z]+", text)


def only_filler_dropped(ours, theirs):
    a, b = words(ours), words(theirs)
    for start in range(len(a) - len(b) + 1):
        if a[start : start + len(b)] == b:
            extra = a[:start] + a[start + len(b) :]
            return bool(extra) and all(w in FILLER_WORDS for w in extra)
    return False


def similarity(ours, theirs):
    a, b = simplify(ours), simplify(theirs)
    if not a or not b:
        return 0.0
    if a == b or only_filler_dropped(ours, theirs):
        return 1.0
    if edit_distance(a, b) <= max(1, int(0.2 * max(len(a), len(b)))):
        return 0.9
    if abs(len(a) - len(b)) > 0.25 * max(len(a), len(b)):
        return 0.0
    return difflib.SequenceMatcher(None, a, b).ratio()


def edit_distance(a, b):
    previous = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        current = [i]
        for j, cb in enumerate(b, 1):
            current.append(min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (ca != cb)))
        previous = current
    return previous[-1]


def fetch_places(url):
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    for attempt in range(6):
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                data = response.read()
            break
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError) as e:
            if isinstance(e, urllib.error.HTTPError) and e.code == 404:
                return None
            time.sleep(5 * (attempt + 1))
    else:
        raise RuntimeError(f"tile kept failing: {url}")
    layer = mapbox_vector_tile.decode(data, default_options={"y_coord_down": True}).get("place", {})
    return [
        (f["geometry"]["coordinates"], f["properties"])
        for f in layer.get("features", [])
        if f["geometry"]["type"] == "Point"
    ], layer.get("extent", 4096)


def best_match(city, places, radius):
    best = None
    nearest = None
    for (lat, lon), props in places:
        label = map_label(props)
        if not label:
            continue
        distance = km((city["lat"], city["lon"]), (lat, lon))
        if distance > radius:
            continue
        score = similarity(city["names"][0], label)
        if score >= MIN_SIMILARITY and (best is None or (score, -distance) > (best[0], -best[1])):
            best = (score, distance, label)
        if (
            city["nearby"]
            and distance <= NEARBY_KM
            and props.get("class") in NEARBY_CLASSES
            and (nearest is None or distance < nearest[1])
        ):
            alternate = max((similarity(name, label) for name in city["names"][1:]), default=0.0)
            nearest = (score, distance, label, alternate)
    if (
        best is None
        and nearest is not None
        and (nearest[0] >= NEARBY_SIMILARITY or nearest[3] >= MIN_SIMILARITY)
    ):
        return nearest
    return best


def alternate_names(ids):
    names = {}
    with open(GEONAMES, encoding="utf-8") as f:
        for line in f:
            columns = line.split("\t")
            if columns[0] in ids:
                names[columns[0]] = [n for n in columns[3].split(",") if n]
    return names


def main():
    with urllib.request.urlopen(urllib.request.Request(TILEJSON, headers={"User-Agent": USER_AGENT})) as r:
        template = json.load(r)["tiles"][0]

    retry = "--retry-empty" in sys.argv
    cities = []
    with open(CITIES, encoding="utf-8") as f:
        for line in f:
            gid, lat, lon, name = line.rstrip("\n").split("\t")
            cities.append(
                {"id": gid, "lat": float(lat), "lon": float(lon), "names": [name], "nearby": retry}
            )

    found = {}
    if os.path.exists(TARGET):
        with open(TARGET, encoding="utf-8") as f:
            found = dict(line.rstrip("\n").split("\t", 1) for line in f if line.strip())
    if retry:
        target = RETRY_TARGET
        done = set()
        if os.path.exists(RETRY_TARGET):
            with open(RETRY_TARGET, encoding="utf-8") as f:
                done = {line.split("\t", 1)[0] for line in f if line.strip()}
        todo = [c for c in cities if found.get(c["id"]) == "" and c["id"] not in done]
        alternates = alternate_names({c["id"] for c in todo})
        for c in todo:
            c["names"] += alternates.get(c["id"], [])
    else:
        target = TARGET
        todo = [c for c in cities if c["id"] not in found]
    print(f"{len(cities)} round cities, {len(todo)} still to check")

    lock = threading.Lock()
    out = open(target, "a", encoding="utf-8")

    for z, radius in PASSES:
        by_tile = collections.defaultdict(list)
        for c in todo:
            by_tile[tile_for(c["lat"], c["lon"], z)].append(c)
        last_pass = (z, radius) == PASSES[-1]
        unmatched = []

        def check(tile):
            x, y = tile
            result = fetch_places(template.format(z=z, x=x, y=y))
            places = []
            if result:
                features, extent = result
                places = [(to_latlon(z, x, y, px, py, extent), props) for (px, py), props in features]
            rows, missed = [], []
            for city in by_tile[tile]:
                match = best_match(city, places, radius)
                if match:
                    rows.append(f"{city['id']}\t{match[2]}\n")
                elif last_pass:
                    rows.append(f"{city['id']}\t\n")
                else:
                    missed.append(city)
            return rows, missed

        with concurrent.futures.ThreadPoolExecutor(WORKERS) as pool:
            for i, (rows, missed) in enumerate(pool.map(check, list(by_tile)), 1):
                with lock:
                    out.writelines(rows)
                    unmatched.extend(missed)
                if i % 500 == 0:
                    out.flush()
                    print(f"zoom {z}: {i}/{len(by_tile)} tiles")
        out.flush()
        print(f"zoom {z}: done, {len(unmatched)} cities left for the next pass")
        todo = unmatched

    out.close()
    print(f"wrote {target}")


if __name__ == "__main__":
    main()
