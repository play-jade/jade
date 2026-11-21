import glossaryData from '../../../../data/fixtures/i18n/glossary.json'

export type SupportedLocale = 'en-US' | 'en-GB' | 'zh-Hans-CN' | 'zh-Hant-TW' | 'zh-Hant-HK'
export type PieceScriptMode = 'auto' | 'simplified' | 'traditional'
export type MaterialFamilyId = 'pale-jade' | 'boxwood' | 'ivory-stone'
export type NotationDisplayMode = 'chinese' | 'uci'

export const SUPPORTED_LOCALES: { id: SupportedLocale; label: string }[] = [
  { id: 'en-US', label: 'English (US)' },
  { id: 'en-GB', label: 'English (UK)' },
  { id: 'zh-Hans-CN', label: '简体中文 (CN)' },
  { id: 'zh-Hant-TW', label: '繁體中文 (TW)' },
  { id: 'zh-Hant-HK', label: '繁體中文 (HK)' },
]

export function resolveTraditionalScript(locale: SupportedLocale, override: PieceScriptMode): boolean {
  if (override === 'traditional') return true
  if (override === 'simplified') return false
  return locale === 'zh-Hant-TW' || locale === 'zh-Hant-HK'
}

const EXTRA_UI_COPY: Record<string, Record<SupportedLocale, string>> = {
  'nav.play': {
    'en-US': 'Board',
    'en-GB': 'Board',
    'zh-Hans-CN': '棋枰',
    'zh-Hant-TW': '棋枰',
    'zh-Hant-HK': '棋枰',
  },
  'nav.archive': {
    'en-US': 'Archive',
    'en-GB': 'Archive',
    'zh-Hans-CN': '棋谱档案',
    'zh-Hant-TW': '棋譜檔案',
    'zh-Hant-HK': '棋譜檔案',
  },
  'clock.5_plus_3': {
    'en-US': '5 min + 3s / move',
    'en-GB': '5 min + 3s / move',
    'zh-Hans-CN': '5分钟 + 每步3秒',
    'zh-Hant-TW': '5分鐘 + 每步3秒',
    'zh-Hant-HK': '5分鐘 + 每步3秒',
  },
  'clock.10_plus_5': {
    'en-US': '10 min + 5s / move',
    'en-GB': '10 min + 5s / move',
    'zh-Hans-CN': '10分钟 + 每步5秒',
    'zh-Hant-TW': '10分鐘 + 每步5秒',
    'zh-Hant-HK': '10分鐘 + 每步5秒',
  },
  'clock.15_plus_10': {
    'en-US': '15 min + 10s / move',
    'en-GB': '15 min + 10s / move',
    'zh-Hans-CN': '15分钟 + 每步10秒',
    'zh-Hant-TW': '15分鐘 + 每步10秒',
    'zh-Hant-HK': '15分鐘 + 每步10秒',
  },
  'clock.untimed': {
    'en-US': 'Untimed',
    'en-GB': 'Untimed',
    'zh-Hans-CN': '不限时',
    'zh-Hant-TW': '不限時',
    'zh-Hant-HK': '不限時',
  },
  'export.pgn': {
    'en-US': 'Export PGN',
    'en-GB': 'Export PGN',
    'zh-Hans-CN': '导出棋谱 (PGN)',
    'zh-Hant-TW': '匯出棋譜 (PGN)',
    'zh-Hant-HK': '匯出棋譜 (PGN)',
  },
  'board.flip': {
    'en-US': 'Flip board',
    'en-GB': 'Flip board',
    'zh-Hans-CN': '翻转棋盘',
    'zh-Hant-TW': '翻轉棋盤',
    'zh-Hant-HK': '翻轉棋盤',
  },
  'board.undo': {
    'en-US': 'Take back',
    'en-GB': 'Take back',
    'zh-Hans-CN': '悔棋',
    'zh-Hant-TW': '悔棋',
    'zh-Hant-HK': '悔棋',
  },
  'board.new_local': {
    'en-US': 'Reset board',
    'en-GB': 'Reset board',
    'zh-Hans-CN': '重新摆棋',
    'zh-Hant-TW': '重新擺棋',
    'zh-Hant-HK': '重新擺棋',
  },
  'engine.title': {
    'en-US': 'Local engine analysis',
    'en-GB': 'Local engine analysis',
    'zh-Hans-CN': '本地引擎拆解',
    'zh-Hant-TW': '本機引擎拆解',
    'zh-Hant-HK': '本機引擎拆解',
  },
  'engine.start': {
    'en-US': 'Analyze position',
    'en-GB': 'Analyse position',
    'zh-Hans-CN': '启动分析',
    'zh-Hant-TW': '啟動分析',
    'zh-Hant-HK': '啟動分析',
  },
  'engine.stop': {
    'en-US': 'Stop engine',
    'en-GB': 'Stop engine',
    'zh-Hans-CN': '停止分析',
    'zh-Hant-TW': '停止分析',
    'zh-Hant-HK': '停止分析',
  },
  'turn.red': {
    'en-US': 'Red to move',
    'en-GB': 'Red to move',
    'zh-Hans-CN': '红方行棋',
    'zh-Hant-TW': '紅方行棋',
    'zh-Hant-HK': '紅方行棋',
  },
  'turn.black': {
    'en-US': 'Black to move',
    'en-GB': 'Black to move',
    'zh-Hans-CN': '黑方行棋',
    'zh-Hant-TW': '黑方行棋',
    'zh-Hant-HK': '黑方行棋',
  },
}

const termsMap = glossaryData.terms as Record<string, Record<SupportedLocale, string>>

export function tTerm(key: string, locale: SupportedLocale): string {
  return termsMap[key]?.[locale] ?? EXTRA_UI_COPY[key]?.[locale] ?? key
}
