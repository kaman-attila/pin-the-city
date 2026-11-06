import { useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import {
  ArrowRightIcon,
  CheckIcon,
  ChevronDownIcon,
  CloseIcon,
  PinIcon,
  ShareIcon,
  StarIcon,
} from './icons'
import { formatDistance } from './settings'
import { type GuessResult, type Units } from './api'
import { shareOrCopy } from './share'
import './Game.css'

const MIN_HEADING_PX = 18

type ResultPanelProps = {
  result: GuessResult
  units: Units
  placeName: string
  nextLabel: string
  onNext: () => void
  regionName?: string | null
}

export function CityHeading({ name }: { name: string | null }) {
  const ref = useRef<HTMLHeadingElement>(null)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const heading = el

    function fit() {
      heading.style.fontSize = ''
      for (let i = 0; i < 3 && heading.scrollWidth > heading.clientWidth; i++) {
        const size = parseFloat(getComputedStyle(heading).fontSize)
        const shrunk = Math.floor((size * heading.clientWidth) / heading.scrollWidth)
        heading.style.fontSize = `${Math.max(MIN_HEADING_PX, shrunk)}px`
      }
    }

    fit()
    document.fonts?.ready.then(fit)
    const observer = new ResizeObserver(fit)
    if (heading.parentElement) observer.observe(heading.parentElement)
    return () => observer.disconnect()
  }, [name])

  return (
    <div className="city-heading">
      <h1 ref={ref} className="display city-name">
        {name ?? '\u00a0'}
      </h1>
    </div>
  )
}

export function ShareButton({ getText }: { getText: () => string }) {
  const [state, setState] = useState<'idle' | 'copied' | 'failed'>('idle')

  async function share() {
    try {
      const outcome = await shareOrCopy(getText())
      if (outcome === 'copied') {
        setState('copied')
        setTimeout(() => setState('idle'), 2500)
      }
    } catch {
      setState('failed')
    }
  }

  return (
    <>
      <button className="btn-pill share-btn" onClick={share} aria-live="polite">
        {state === 'copied' ? (
          <>
            <CheckIcon size={22} strokeWidth={2.6} />
            Copied! Paste it anywhere
          </>
        ) : (
          <>
            <ShareIcon size={22} strokeWidth={2.4} />
            Share your result
          </>
        )}
      </button>
      {state === 'failed' && (
        <p className="error share-error">Couldn't copy. Your browser blocked the clipboard.</p>
      )}
    </>
  )
}

export function NewBestBadge({
  previous,
  title = 'New personal best!',
}: {
  previous: number
  title?: string
}) {
  return (
    <div className="new-best" role="status">
      <StarIcon size={18} />
      <span>
        <strong>{title}</strong>
        <small>
          {previous === 0 ? 'Your first saved best score' : `Previous best: ${previous}`}
        </small>
      </span>
    </div>
  )
}

export function MapHint() {
  return (
    <div className="map-hint">
      <PinIcon size={18} className="pin-accent" />
      Tap the map to drop your pin
    </div>
  )
}

type ControlsProps = { hasPin: boolean; onClear: () => void; onSubmit: () => void }

export function GuessControls({ hasPin, onClear, onSubmit }: ControlsProps) {
  return (
    <div className="guess-controls">
      {hasPin && (
        <button className="icon-btn icon-btn-lg" aria-label="Remove pin" onClick={onClear}>
          <CloseIcon strokeWidth={2.4} />
        </button>
      )}
      <button
        className="fab"
        aria-label={hasPin ? 'Submit guess' : 'Submit guess (drop a pin first)'}
        disabled={!hasPin}
        onClick={onSubmit}
      >
        <CheckIcon size={34} strokeWidth={3} />
      </button>
    </div>
  )
}

export function CenterCard({ children }: { children: ReactNode }) {
  return (
    <div className="center-card-backdrop">
      <div className="center-card">{children}</div>
    </div>
  )
}

export function ResultPanel({
  result,
  units,
  placeName,
  nextLabel,
  onNext,
  regionName = null,
}: ResultPanelProps) {
  const nearest = result.cities[result.closest]
  const count = result.cities.length
  const [hidden, setHidden] = useState(false)

  if (hidden) {
    return (
      <div className="result-mini">
        <button
          className="result-mini-show"
          aria-label="Show the round result"
          onClick={() => setHidden(false)}
        >
          <span className="display result-mini-points">+{result.score}</span>
          <span className="mono">{formatDistance(result.distance_m, units)}</span>
          <ChevronDownIcon size={18} strokeWidth={2.6} className="result-mini-chevron" />
        </button>
        <button className="fab fab-xs" aria-label={nextLabel} onClick={onNext}>
          <ArrowRightIcon size={22} strokeWidth={2.8} />
        </button>
      </div>
    )
  }

  return (
    <div className="result-panel">
      <button
        className="result-hide"
        aria-label="Hide the round result"
        onClick={() => setHidden(true)}
      >
        <CloseIcon size={16} strokeWidth={2.6} />
      </button>
      <div className="result-top">
        <div>
          <div className="display result-points">+{result.score}</div>
          <div className="label result-points-label">Points this round</div>
        </div>
        <button className="fab fab-sm" aria-label={nextLabel} onClick={onNext}>
          <ArrowRightIcon size={28} strokeWidth={2.8} />
        </button>
      </div>

      <div className="meter result-meter">
        <div style={{ width: `${result.score / 10}%` }} />
      </div>

      <div className="result-distance">
        <span className="mono">{formatDistance(result.distance_m, units)}</span>
        <span>
          from {nearest.name}, {nearest.country}
        </span>
      </div>

      {result.city_limits && (
        <div className="result-inside">
          <CheckIcon size={16} strokeWidth={2.8} />
          {result.city_limits === 'inside'
            ? `Inside the city limits of ${nearest.name}`
            : result.city_limits === 'near'
              ? `Close to the city limits of ${nearest.name}`
              : `Points counted from the city limits of ${nearest.name}`}
        </div>
      )}

      {count > 1 && (
        <div className="result-note">
          <span className="legend-dot other" aria-hidden="true" />
          <span>
            {count} places are called {placeName}.{' '}
            {regionName ? `The nearest one in ${regionName} counts.` : 'The nearest one counts.'}
          </span>
        </div>
      )}
    </div>
  )
}
