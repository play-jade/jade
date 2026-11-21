import type { SupportedLocale } from '~/utils/i18n'

/** Side plates beside the board: the large side name and the pieces a side has lost, as on macOS. */
export type Side = 'red' | 'black'

const SIDE_NAMES: Record<Side, { en: string; hans: string; hant: string }> = {
  red: { en: 'Red', hans: '红方', hant: '紅方' },
  black: { en: 'Black', hans: '黑方', hant: '黑方' },
}

export function sideName(side: Side, locale: SupportedLocale): string {
  const names = SIDE_NAMES[side]
  if (locale === 'zh-Hans-CN') return names.hans
  if (locale.startsWith('zh')) return names.hant
  return names.en
}

const STARTING_COUNTS: Record<string, number> = {
  chariot: 2, horse: 2, cannon: 2, elephant: 2, advisor: 2, soldier: 5, general: 1,
}

const FACES: Record<'simplified' | 'traditional', Record<Side, Record<string, string>>> = {
  simplified: {
    red: { chariot: '车', horse: '马', cannon: '炮', elephant: '相', advisor: '仕', soldier: '兵', general: '帅' },
    black: { chariot: '车', horse: '马', cannon: '炮', elephant: '象', advisor: '士', soldier: '卒', general: '将' },
  },
  traditional: {
    red: { chariot: '車', horse: '馬', cannon: '炮', elephant: '相', advisor: '仕', soldier: '兵', general: '帥' },
    black: { chariot: '車', horse: '馬', cannon: '砲', elephant: '象', advisor: '士', soldier: '卒', general: '將' },
  },
}

/** Faces of the pieces `side` has lost, most valuable first. */
export function capturedFaces(
  pieces: ReadonlyArray<{ side: string; role: string }>,
  side: Side,
  traditional: boolean,
): string[] {
  const faces = FACES[traditional ? 'traditional' : 'simplified'][side]
  const out: string[] = []
  for (const role of Object.keys(STARTING_COUNTS)) {
    const left = pieces.filter((p) => p.side === side && p.role === role).length
    for (let i = left; i < STARTING_COUNTS[role]; i++) out.push(faces[role])
  }
  return out
}

/** Plates top to bottom for the board's orientation. */
export function plateOrder(redBelow: boolean): Side[] {
  return redBelow ? ['black', 'red'] : ['red', 'black']
}
