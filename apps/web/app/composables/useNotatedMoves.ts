import { ref, watch, type Ref } from 'vue'
import { notateMoves } from '~/utils/core-wasm'

/**
 * Move labels in the reader's notation dialect (`usePreferences().notationDialect`). Coordinates show while the core
 * computes the notation, and remain for any move after a record stops replaying, so a label is never empty.
 */
export function useNotatedMoves(
  movesUci: Ref<string[]>,
  fen?: Ref<string | undefined>,
  initial?: Ref<string[] | undefined>,
  /**
   * The settings each `initial` was notated with, and where its record stopped replaying. When a new one arrives with
   * the current settings, its labels are used as they are instead of notating the same moves again.
   */
  initialSettings?: Ref<{ dialect: string; glyphs: boolean; invalidPly: number | null } | undefined>,
  /** The position and moves the seed was computed for; a break is reused only for exactly these. */
  initialFor?: Ref<{ fen: string; movesUci: string[] } | undefined>,
) {
  const { notationDialect, traditionalGlyphs } = usePreferences()
  // The server renders `initial` (or coordinates), and the client starts from the same labels, so hydration matches;
  // the core then notates on the client only.
  /**
   * Whether the seed was computed for these moves: for the exact position and move sequence when the caller names
   * them, and otherwise for a line of the same length. Every reuse of seeded labels or of a seeded break checks it.
   */
  const seedFits = (moves: string[]): boolean => {
    const seeded = initial?.value
    if (!seeded || seeded.length !== moves.length) return false
    // A caller that names no inputs keeps the length check; one that names them but has none yet gives no evidence.
    if (initialFor === undefined) return true
    const inputs = initialFor.value
    if (!inputs) return false
    return (
      inputs.fen === (fen?.value ?? '') &&
      inputs.movesUci.length === moves.length &&
      inputs.movesUci.every((uci, i) => uci === moves[i])
    )
  }
  /** The seeded break, when the seed fits these moves. */
  const seededBreak = (moves: string[]): number | null | undefined =>
    initialSettings?.value && seedFits(moves) ? initialSettings.value.invalidPly : undefined
  const startsSeeded = seedFits(movesUci.value)
  const labels = ref<string[]>([...(startsSeeded ? initial!.value! : movesUci.value)])
  // A seeded record's break is known before any browser pass, so the server render reports it too.
  const invalidPly = ref<number | null>((startsSeeded ? initialSettings?.value?.invalidPly : null) ?? null)
  let request = 0
  let usedSeed: object | undefined

  if (import.meta.server) return { labels, invalidPly }

  watch(
    [movesUci, () => fen?.value, notationDialect, traditionalGlyphs, () => initialSettings?.value, () => initialFor?.value],
    async () => {
      const moves = [...movesUci.value]
      const id = ++request
      const seeded = initialSettings?.value
      const seededLabels = initial?.value
      if (
        seeded !== undefined &&
        seeded !== usedSeed &&
        seeded.dialect === notationDialect.value &&
        seeded.glyphs === traditionalGlyphs.value &&
        seededLabels !== undefined &&
        seedFits(moves)
      ) {
        usedSeed = seeded
        labels.value = [...seededLabels]
        invalidPly.value = seeded.invalidPly
        return
      }
      if (moves.length === 0 || notationDialect.value === 'uci') {
        labels.value = moves
        // Coordinates do not make a broken record replay; a seeded break for these exact moves still holds.
        invalidPly.value = seededBreak(moves) ?? null
        return
      }
      try {
        const res = await notateMoves({
          fen: fen?.value,
          movesUci: moves,
          wxf: notationDialect.value === 'wxf',
          traditionalGlyphs: traditionalGlyphs.value,
        })
        if (id !== request) return
        labels.value = moves.map((u, i) => res.notations[i] ?? u)
        invalidPly.value = res.invalid_ply
      } catch {
        if (id !== request) return
        // Failing to format labels says nothing about whether the record replays; a matching seed still holds.
        labels.value = moves
        invalidPly.value = seededBreak(moves) ?? null
      }
    },
    { immediate: true, deep: true },
  )

  return { labels, invalidPly }
}
