import { computed, onMounted, watch } from 'vue'
import {
  resolveTraditionalScript,
  tTerm,
  type MaterialFamilyId,
  type NotationDisplayMode,
  type PieceScriptMode,
  type SupportedLocale,
} from '~/utils/i18n'

const STORAGE_KEY = 'jade_prefs_v1'

export function usePreferences() {
  const locale = useState<SupportedLocale>('jade.locale', () => 'en-US')
  const pieceScript = useState<PieceScriptMode>('jade.pieceScript', () => 'auto')
  const material = useState<MaterialFamilyId>('jade.material', () => 'pale-jade')
  const darkSurface = useState<boolean>('jade.darkSurface', () => false)
  const notationMode = useState<NotationDisplayMode>('jade.notationMode', () => 'chinese')
  const soundEnabled = useState<boolean>('jade.soundEnabled', () => true)
  const reducedMotion = useState<boolean>('jade.reducedMotion', () => false)

  const traditionalGlyphs = computed(() => resolveTraditionalScript(locale.value, pieceScript.value))
  const t = (key: string) => tTerm(key, locale.value)

  onMounted(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY)
      if (raw) {
        const parsed = JSON.parse(raw)
        if (parsed.locale) locale.value = parsed.locale
        if (parsed.pieceScript) pieceScript.value = parsed.pieceScript
        if (parsed.material) material.value = parsed.material
        if (typeof parsed.darkSurface === 'boolean') darkSurface.value = parsed.darkSurface
        if (parsed.notationMode) notationMode.value = parsed.notationMode
        if (typeof parsed.soundEnabled === 'boolean') soundEnabled.value = parsed.soundEnabled
      }
      if (typeof window !== 'undefined' && window.matchMedia) {
        reducedMotion.value = window.matchMedia('(prefers-reduced-motion: reduce)').matches
      }
    } catch {
      // Ignore storage access errors
    }

    watch(
      [locale, pieceScript, material, darkSurface, notationMode, soundEnabled],
      () => {
        try {
          localStorage.setItem(
            STORAGE_KEY,
            JSON.stringify({
              locale: locale.value,
              pieceScript: pieceScript.value,
              material: material.value,
              darkSurface: darkSurface.value,
              notationMode: notationMode.value,
              soundEnabled: soundEnabled.value,
            }),
          )
        } catch {
          // Ignore storage quota errors
        }
      },
      { deep: true },
    )
  })

  return {
    locale,
    pieceScript,
    material,
    darkSurface,
    notationMode,
    soundEnabled,
    reducedMotion,
    traditionalGlyphs,
    t,
  }
}
