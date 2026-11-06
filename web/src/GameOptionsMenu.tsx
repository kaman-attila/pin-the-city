import { useEffect, useRef, useState } from 'react'
import { SlidersIcon } from './icons'
import {
  bestKind,
  describeOptions,
  describeOptionsShort,
  DIFFICULTIES,
  regionLabel,
  REGIONS,
  type GameOptions,
} from './gameOptions'
import './GameOptionsMenu.css'

type Props = {
  options: GameOptions
  onChange: (change: Partial<GameOptions>) => void
  locked: boolean
}

export default function GameOptionsMenu({ options, onChange, locked }: Props) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)

  if (open && locked) setOpen(false)

  useEffect(() => {
    if (!open) return

    function onPointerDown(e: PointerEvent) {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false)
    }

    function onKeyDown(e: KeyboardEvent) {
      if (e.key === 'Escape') {
        setOpen(false)
        buttonRef.current?.focus()
      }
    }

    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [open])

  const kind = bestKind(options)
  const custom = options.region !== 'world' || options.level !== null

  return (
    <div className="game-options" ref={rootRef}>
      <button
        ref={buttonRef}
        className={custom ? 'game-options-btn custom' : 'game-options-btn'}
        aria-label={
          locked
            ? `Game options: ${describeOptions(options)}. Finish this game to change them.`
            : `Game options: ${describeOptions(options)}`
        }
        title={locked ? 'Finish this game to change the options' : undefined}
        aria-expanded={open}
        disabled={locked}
        onClick={() => setOpen((o) => !o)}
      >
        <SlidersIcon size={20} strokeWidth={2.2} />
        <span className="game-options-summary">{describeOptions(options)}</span>
        <span className="game-options-short">{describeOptionsShort(options)}</span>
      </button>

      {open && (
        <div className="game-options-panel">
          <h2 className="display">Game options</h2>

          <div className="option-group" role="group" aria-label="Region">
            <span className="label">Region</span>
            <div className="chips">
              {REGIONS.map((r) => (
                <button
                  key={r.value}
                  type="button"
                  className="chip"
                  aria-pressed={options.region === r.value}
                  onClick={() => onChange({ region: r.value })}
                >
                  {r.label}
                </button>
              ))}
            </div>
          </div>

          <div className="option-group" role="group" aria-label="Difficulty">
            <span className="label">Difficulty</span>
            <div className="chips">
              <button
                type="button"
                className="chip"
                aria-pressed={options.level === null}
                onClick={() => onChange({ level: null })}
              >
                Regular
              </button>
              {DIFFICULTIES.map((d, i) => (
                <button
                  key={d}
                  type="button"
                  className={`chip level-${i + 1}`}
                  aria-pressed={options.level === i + 1}
                  onClick={() => onChange({ level: i + 1 })}
                >
                  {d}
                </button>
              ))}
            </div>
          </div>

          <p className="game-options-note">
            {kind === 'best'
              ? 'Counts for your best score.'
              : kind === 'region'
                ? `Counts for your ${regionLabel(options.region)} best.`
                : 'A set difficulty does not count for any best score.'}
          </p>
        </div>
      )}
    </div>
  )
}
