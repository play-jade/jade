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
) {
  const { notationDialect, traditionalGlyphs } = usePreferences()
  // The server renders `initial` (or coordinates), and the client starts from the same labels, so hydration matches;
  // the core then notates on the client only.
  const labels = ref<string[]>([...(initial?.value ?? movesUci.value)])
  const invalidPly = ref<number | null>(null)
  let request = 0

  if (import.meta.server) return { labels, invalidPly }

  watch(
    [movesUci, () => fen?.value, notationDialect, traditionalGlyphs],
    async () => {
      const moves = [...movesUci.value]
      const id = ++request
      if (moves.length === 0 || notationDialect.value === 'uci') {
        labels.value = moves
        invalidPly.value = null
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
        labels.value = moves
        invalidPly.value = null
      }
    },
    { immediate: true, deep: true },
  )

  return { labels, invalidPly }
}
