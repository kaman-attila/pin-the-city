-- Migration number: 0014 	 2026-10-23T14:00:00.000Z
CREATE TABLE city_areas (
  city_id INTEGER PRIMARY KEY,
  shape   BLOB NOT NULL
);
