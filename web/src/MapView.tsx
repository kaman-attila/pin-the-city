import { useEffect, useRef } from 'react'
import 'maplibre-gl/dist/maplibre-gl.css'
import { type LatLon, type GuessResult, type Units } from './api'
import type { Feature, FeatureCollection } from 'geojson'
import * as maplibregl from 'maplibre-gl'
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url'
import { MinusIcon, PlusIcon } from './icons'
import { formatDistance } from './settings'
import './MapView.css'

maplibregl.setWorkerUrl(workerUrl)

type Props = {
  guess: LatLon | null
  onMapClick: (lat: number, lon: number) => void
  result: GuessResult | null
  dark: boolean
  units: Units
}

type LineData = Feature | FeatureCollection

const EMPTY: FeatureCollection = { type: 'FeatureCollection', features: [] }
const STYLE_URL = 'https://tiles.openfreemap.org/styles/positron'

function unwrapLon(lon: number, nearLon: number): number {
  if (lon - nearLon > 180) return lon - 360
  if (lon - nearLon < -180) return lon + 360
  return lon
}

function addGuessLine(map: maplibregl.Map, data: LineData) {
  if (map.getSource('guess-line')) return
  map.addSource('guess-line', { type: 'geojson', data })
  map.addLayer({
    id: 'guess-line',
    type: 'line',
    source: 'guess-line',
    layout: { 'line-cap': 'round' },
    paint: { 'line-color': '#4fa318', 'line-width': 3, 'line-dasharray': [2, 2] },
  })
}

const BORDER_WIDTH: maplibregl.ExpressionSpecification = [
  'interpolate',
  ['linear'],
  ['zoom'],
  0,
  0.8,
  3,
  1.3,
  6,
  1.9,
  12,
  3.2,
]

type PaintArgs = Parameters<maplibregl.Map['setPaintProperty']>

function setPaint(map: maplibregl.Map, layer: string, property: PaintArgs[1], value: PaintArgs[2]) {
  if (map.getLayer(layer)) map.setPaintProperty(layer, property, value)
}

type PaintEntry = [layer: string, property: PaintArgs[1], value: PaintArgs[2]]

const LAND = '#1a2343'
const WATER = '#0c1229'
const ROAD = '#38446f'
const ROAD_MINOR = '#2a3459'
const ROAD_CASING = '#252f52'
const ROAD_SUBTLE = 'rgba(46,57,95,0.53)'
const TEXT_STRONG = '#e8ebf6'
const TEXT_SOFT = '#b3bad3'
const TEXT_FAINT = '#949cbb'
const WATER_HALO = 'rgba(12,18,41,0.7)'
const LIGHT_DOT = 'circle_11_black'
const DARK_DOT = 'city_dot_dark'

const ROAD_FADE: maplibregl.ExpressionSpecification = [
  'interpolate',
  ['linear'],
  ['zoom'],
  5.8,
  ROAD_SUBTLE,
  6,
  ROAD,
]

function textPaint(layers: string[], color: string, halo?: string): PaintEntry[] {
  return layers.flatMap((layer) => {
    const entries: PaintEntry[] = [[layer, 'text-color', color]]
    if (halo) entries.push([layer, 'text-halo-color', halo])
    return entries
  })
}

const DARK_PAINT: PaintEntry[] = [
  ['background', 'background-color', LAND],
  ['park', 'fill-color', '#1f2949'],
  ['water', 'fill-color', WATER],
  ['landcover_ice_shelf', 'fill-color', '#242e52'],
  ['landcover_glacier', 'fill-color', '#242e52'],
  ['landuse_residential', 'fill-color', '#1d2647'],
  ['landcover_wood', 'fill-color', '#222c50'],
  ['waterway', 'line-color', '#111a38'],
  ['building', 'fill-color', '#222b4d'],
  ['building', 'fill-outline-color', '#2a3459'],
  ['tunnel_motorway_casing', 'line-color', ROAD_CASING],
  ['tunnel_motorway_inner', 'line-color', ROAD_MINOR],
  ['aeroway-taxiway', 'line-color', ROAD_MINOR],
  ['aeroway-runway-casing', 'line-color', ROAD_MINOR],
  ['aeroway-area', 'fill-color', '#212a4b'],
  ['aeroway-runway', 'line-color', ROAD],
  ['road_area_pier', 'fill-color', LAND],
  ['road_pier', 'line-color', LAND],
  ['highway_path', 'line-color', ROAD_MINOR],
  ['highway_minor', 'line-color', ROAD_MINOR],
  ['highway_major_casing', 'line-color', ROAD_CASING],
  ['highway_major_inner', 'line-color', ROAD],
  ['highway_major_subtle', 'line-color', 'rgba(46,57,95,0.69)'],
  ['highway_motorway_casing', 'line-color', ROAD_CASING],
  ['highway_motorway_inner', 'line-color', ROAD_FADE],
  ['highway_motorway_subtle', 'line-color', ROAD_SUBTLE],
  ['highway_motorway_bridge_casing', 'line-color', ROAD_CASING],
  ['highway_motorway_bridge_inner', 'line-color', ROAD_FADE],
  ['railway_transit', 'line-color', '#2e3860'],
  ['railway_transit_dashline', 'line-color', LAND],
  ['railway_service', 'line-color', '#2e3860'],
  ['railway_service_dashline', 'line-color', LAND],
  ['railway', 'line-color', '#2e3860'],
  ['railway_dashline', 'line-color', LAND],
  ['boundary_3', 'line-color', '#4a5683'],
  ['boundary_2', 'line-color', '#7c88b3'],
  ['boundary_disputed', 'line-color', '#7c88b3'],
  ...textPaint(['waterway_line_label'], '#5f6f9c', WATER_HALO),
  ...textPaint(['water_name_point_label', 'water_name_line_label'], '#7088c0', WATER_HALO),
  ...textPaint(['highway-name-path', 'airport'], TEXT_FAINT, LAND),
  ...textPaint(['highway-name-minor', 'highway-name-major'], TEXT_FAINT),
  ...textPaint(['label_other', 'label_state'], TEXT_SOFT, LAND),
  ...textPaint(
    [
      'label_village',
      'label_town',
      'label_city',
      'label_city_capital',
      'label_country_3',
      'label_country_2',
      'label_country_1',
    ],
    TEXT_STRONG,
    LAND,
  ),
]

function dotImage(): ImageData {
  const size = 34
  const canvas = document.createElement('canvas')
  canvas.width = size
  canvas.height = size
  const ctx = canvas.getContext('2d')
  if (!ctx) return new ImageData(size, size)
  ctx.beginPath()
  ctx.arc(size / 2, size / 2, 10, 0, Math.PI * 2)
  ctx.fillStyle = TEXT_STRONG
  ctx.fill()
  ctx.lineWidth = 2
  ctx.strokeStyle = LAND
  ctx.stroke()
  return ctx.getImageData(0, 0, size, size)
}

const SCRIPT_STARTS: [number, string][] = [
  [0x0370, 'latin'],
  [0x0400, 'greek'],
  [0x0530, 'cyrillic'],
  [0x0590, 'armenian'],
  [0x0600, 'hebrew'],
  [0x0700, 'arabic'],
  [0x0780, 'syriac'],
  [0x0800, 'thaana'],
  [0x0900, 'other'],
  [0x0980, 'devanagari'],
  [0x0a00, 'bengali'],
  [0x0a80, 'gurmukhi'],
  [0x0b00, 'gujarati'],
  [0x0b80, 'oriya'],
  [0x0c00, 'tamil'],
  [0x0c80, 'telugu'],
  [0x0d00, 'kannada'],
  [0x0d80, 'malayalam'],
  [0x0e00, 'sinhala'],
  [0x0e80, 'thai'],
  [0x0f00, 'lao'],
  [0x1000, 'tibetan'],
  [0x10a0, 'myanmar'],
  [0x1100, 'georgian'],
  [0x1200, 'hangul'],
  [0x13a0, 'ethiopic'],
  [0x1780, 'other'],
  [0x1800, 'khmer'],
  [0x18b0, 'mongolian'],
  [0x1e00, 'other'],
  [0x1f00, 'latin'],
  [0x2000, 'greek'],
  [0x2d30, 'other'],
  [0x2d80, 'tifinagh'],
  [0x2e80, 'other'],
  [0xa000, 'cjk'],
  [0xac00, 'other'],
  [0xd7b0, 'hangul'],
  [0xf900, 'other'],
  [0xfb00, 'cjk'],
  [0xfb50, 'other'],
  [0xfe00, 'arabic'],
  [0xfe70, 'other'],
  [0xff00, 'arabic'],
]
const MAX_LABEL_WORDS = 6

function scriptOf(word: unknown): unknown {
  const first = ['slice', word, 0, 1]
  return [
    'case',
    ...SCRIPT_STARTS.flatMap(([end, script]) => [['<', first, String.fromCodePoint(end)], script]),
    'other',
  ]
}

function sameScriptWords(from: number): unknown {
  if (from >= MAX_LABEL_WORDS) return ''
  const word = ['at', from, ['var', 'words']]
  return [
    'case',
    ['>', ['length', ['var', 'words']], from],
    [
      'case',
      ['==', scriptOf(word), ['var', 'script']],
      ['concat', ' ', word, sameScriptWords(from + 1)],
      '',
    ],
    '',
  ]
}

const FIRST_LOCAL_SCRIPT = [
  'let',
  'words',
  ['split', ['get', 'name:nonlatin'], ' '],
  [
    'let',
    'script',
    scriptOf(['at', 0, ['var', 'words']]),
    ['concat', ['at', 0, ['var', 'words']], sameScriptWords(1)],
  ],
]

function limitLabelScripts(map: maplibregl.Map) {
  const local = JSON.stringify(['get', 'name:nonlatin'])
  for (const layer of map.getStyle().layers) {
    if (layer.type !== 'symbol') continue
    const field = JSON.stringify(layer.layout?.['text-field'] ?? null)
    if (!field.includes(local)) continue
    map.setLayoutProperty(
      layer.id,
      'text-field',
      JSON.parse(field.replaceAll(local, JSON.stringify(FIRST_LOCAL_SCRIPT))),
    )
  }
}

function tuneStyle(map: maplibregl.Map) {
  limitLabelScripts(map)
  for (const layer of ['boundary_2', 'boundary_disputed']) {
    setPaint(map, layer, 'line-color', '#7d869e')
    setPaint(map, layer, 'line-opacity', 1)
    setPaint(map, layer, 'line-width', BORDER_WIDTH)
  }
  if (map.getLayer('label_country_3') && map.getLayer('label_city_capital')) {
    map.moveLayer('label_country_3', 'label_city_capital')
  }
  if (!map.hasImage(DARK_DOT)) map.addImage(DARK_DOT, dotImage(), { pixelRatio: 2 })
}

function capturePaint(map: maplibregl.Map): PaintEntry[] {
  return DARK_PAINT.filter(([layer]) => map.getLayer(layer)).map(([layer, property]) => [
    layer,
    property,
    map.getPaintProperty(layer, property),
  ])
}

function swapIcon(map: maplibregl.Map, from: string, to: string) {
  for (const layer of map.getStyle().layers) {
    if (layer.type !== 'symbol') continue
    const icon = JSON.stringify(layer.layout?.['icon-image'] ?? null)
    if (!icon.includes(`"${from}"`)) continue
    map.setLayoutProperty(
      layer.id,
      'icon-image',
      JSON.parse(icon.replaceAll(`"${from}"`, `"${to}"`)),
    )
  }
}

function applyTheme(map: maplibregl.Map, dark: boolean, lightPaint: PaintEntry[]) {
  for (const [layer, property, value] of dark ? DARK_PAINT : lightPaint) {
    setPaint(map, layer, property, value)
  }
  if (dark) swapIcon(map, LIGHT_DOT, DARK_DOT)
  else swapIcon(map, DARK_DOT, LIGHT_DOT)
}

function pinElement(): HTMLElement {
  const el = document.createElement('div')
  el.className = 'map-pin'
  el.innerHTML =
    '<svg viewBox="0 0 40 48" width="40" height="48" aria-hidden="true">' +
    '<path d="M20 46s16-14.5 16-27A16 16 0 0 0 4 19c0 12.5 16 27 16 27z"/>' +
    '<circle cx="20" cy="19" r="6"/></svg>'
  return el
}

function cityDotElement(nearest: boolean): HTMLElement {
  const el = document.createElement('div')
  el.className = nearest ? 'city-dot city-dot-nearest' : 'city-dot'
  return el
}

function labelElement(text: string): HTMLElement {
  const el = document.createElement('div')
  el.className = 'line-label'
  el.textContent = text
  return el
}

export default function MapView({ guess, onMapClick, result, dark, units }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const mapRef = useRef<maplibregl.Map | null>(null)
  const markerRef = useRef<maplibregl.Marker | null>(null)
  const onMapClickRef = useRef(onMapClick)
  const cityMarkersRef = useRef<maplibregl.Marker[]>([])
  const darkRef = useRef(dark)
  const lightPaintRef = useRef<PaintEntry[] | null>(null)
  const lineDataRef = useRef<LineData>(EMPTY)

  useEffect(() => {
    onMapClickRef.current = onMapClick
  })

  useEffect(() => {
    darkRef.current = dark
    const map = mapRef.current
    if (map && lightPaintRef.current) applyTheme(map, dark, lightPaintRef.current)
  }, [dark])

  useEffect(() => {
    if (!containerRef.current) return

    const map = new maplibregl.Map({
      container: containerRef.current,
      center: [10, 30],
      zoom: 1.5,
      attributionControl: { compact: true },
    })
    map.setStyle(STYLE_URL, {
      transformStyle: (_, style) => ({ ...style, transition: { duration: 0, delay: 0 } }),
    })
    mapRef.current = map

    map.on('style.load', () => {
      tuneStyle(map)
      lightPaintRef.current = capturePaint(map)
      if (darkRef.current) applyTheme(map, true, lightPaintRef.current)
      addGuessLine(map, lineDataRef.current)
    })

    map.on('click', (e) => {
      const { lat, lng } = e.lngLat.wrap()
      onMapClickRef.current(lat, lng)
    })

    const resizeObserver = new ResizeObserver(() => map.resize())
    resizeObserver.observe(containerRef.current)

    return () => {
      resizeObserver.disconnect()
      markerRef.current?.remove()
      markerRef.current = null
      cityMarkersRef.current.forEach((m) => m.remove())
      cityMarkersRef.current = []
      map.remove()
      mapRef.current = null
      lightPaintRef.current = null
    }
  }, [])

  useEffect(() => {
    const map = mapRef.current
    if (!map) return

    if (!guess) {
      markerRef.current?.remove()
      markerRef.current = null
      return
    }

    if (!markerRef.current) {
      markerRef.current = new maplibregl.Marker({ element: pinElement(), anchor: 'bottom' })
        .setLngLat([guess.lon, guess.lat])
        .addTo(map)
    } else {
      markerRef.current.setLngLat([guess.lon, guess.lat])
    }
  }, [guess])

  useEffect(() => {
    const map = mapRef.current
    if (!map) return

    cityMarkersRef.current.forEach((m) => m.remove())
    cityMarkersRef.current = []
    const line = map.getSource('guess-line') as maplibregl.GeoJSONSource | undefined
    const setLine = (data: LineData) => {
      lineDataRef.current = data
      line?.setData(data)
    }
    setLine(EMPTY)

    if (!result || !guess) return

    result.cities.forEach((city, i) => {
      const marker = new maplibregl.Marker({ element: cityDotElement(i === result.closest) })
        .setLngLat([unwrapLon(city.lon, guess.lon), city.lat])
        .setPopup(new maplibregl.Popup({ offset: 18 }).setText(`${city.name}, ${city.country}`))
        .addTo(map)
      cityMarkersRef.current.push(marker)
    })

    const nearest = result.cities[result.closest]
    const nearestLon = unwrapLon(nearest.lon, guess.lon)

    setLine({
      type: 'Feature',
      properties: {},
      geometry: {
        type: 'LineString',
        coordinates: [
          [guess.lon, guess.lat],
          [nearestLon, nearest.lat],
        ],
      },
    })

    const label = new maplibregl.Marker({
      element: labelElement(formatDistance(result.distance_m, units)),
    })
      .setLngLat([(guess.lon + nearestLon) / 2, (guess.lat + nearest.lat) / 2])
      .addTo(map)
    cityMarkersRef.current.push(label)

    const wide = map.getContainer().clientWidth > 760
    const padding = wide
      ? { top: 90, right: 90, bottom: 90, left: 410 }
      : { top: 70, right: 40, bottom: 300, left: 40 }

    const bounds = new maplibregl.LngLatBounds()
    bounds.extend([guess.lon, guess.lat])
    bounds.extend([nearestLon, nearest.lat])
    map.fitBounds(bounds, { padding, maxZoom: 10 })
  }, [result, guess, units])

  return (
    <div className={result ? 'map-view' : 'map-view guessing'}>
      <div ref={containerRef} className="map-view-canvas" />
      <div className="map-zoom">
        <button aria-label="Zoom in" onClick={() => mapRef.current?.zoomIn()}>
          <PlusIcon strokeWidth={2.2} />
        </button>
        <div className="map-zoom-divider" />
        <button aria-label="Zoom out" onClick={() => mapRef.current?.zoomOut()}>
          <MinusIcon strokeWidth={2.2} />
        </button>
      </div>
    </div>
  )
}
