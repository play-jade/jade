import { onBeforeUnmount, ref } from 'vue'
import { BrowserWorkerSearchAdapter } from '@jade/wasm-worker'

export function useLocalEngine() {
  const isReady = ref(false)
  const isAnalyzing = ref(false)
  const engineName = ref('Jade Search Kernel (WASM)')
  const depth = ref(0)
  const nodes = ref(0)
  const nps = ref(0)
  const scoreRedCp = ref<number | null>(null)
  const mateRed = ref<number | null>(null)
  const pvUci = ref<string[]>([])
  const bestMoveUci = ref<string | null>(null)
  const errorMsg = ref<string | null>(null)

  let adapter: BrowserWorkerSearchAdapter | null = null
  let searchSeq = 0

  async function ensureAdapter(): Promise<BrowserWorkerSearchAdapter> {
    if (!adapter) {
      adapter = new BrowserWorkerSearchAdapter()
      const ready = await adapter.init()
      engineName.value = `${ready.manifest.engine_id} (${ready.manifest.rules_profile})`
      await adapter.configure({ hash_mb: 16, threads: 1, multi_pv: 1 })
      isReady.value = true
    }
    return adapter
  }

  async function startAnalysis(params: { fen: string; movesUci?: string[]; stateId: string; movetimeMs?: number }) {
    errorMsg.value = null
    try {
      const eng = await ensureAdapter()
      if (isAnalyzing.value) {
        await eng.cancel()
      }
      isAnalyzing.value = true
      depth.value = 0
      pvUci.value = []
      bestMoveUci.value = null

      const searchId = `web-search-${++searchSeq}`
      const done = await eng.search({
        search_id: searchId,
        state_id: params.stateId,
        fen: params.fen,
        moves: params.movesUci ?? [],
        limits: {
          depth: 4,
          movetime_ms: params.movetimeMs ?? 800,
        },
        onInfo: (info) => {
          depth.value = info.depth
          nodes.value = info.nodes
          nps.value = info.nps
          pvUci.value = info.pv
          if (info.score.kind === 'cp') {
            scoreRedCp.value = info.score.red_value
            mateRed.value = null
          } else {
            mateRed.value = info.score.red_value
            scoreRedCp.value = null
          }
        },
      })

      bestMoveUci.value = done.best_move
      if (done.pv.length > 0) {
        pvUci.value = done.pv
      }
      if (done.score.kind === 'cp') {
        scoreRedCp.value = done.score.red_value
        mateRed.value = null
      } else {
        mateRed.value = done.score.red_value
        scoreRedCp.value = null
      }
    } catch (err) {
      errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      isAnalyzing.value = false
    }
  }

  async function stopAnalysis() {
    if (adapter && isAnalyzing.value) {
      await adapter.cancel()
      isAnalyzing.value = false
    }
  }

  onBeforeUnmount(() => {
    void adapter?.dispose()
    adapter = null
  })

  return {
    isReady,
    isAnalyzing,
    engineName,
    depth,
    nodes,
    nps,
    scoreRedCp,
    mateRed,
    pvUci,
    bestMoveUci,
    errorMsg,
    startAnalysis,
    stopAnalysis,
  }
}
