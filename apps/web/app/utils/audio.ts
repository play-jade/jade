import type { MaterialFamilyId } from './i18n'

export type PercussionEvent = 'place' | 'capture' | 'check'

function audioDirForMaterial(material: MaterialFamilyId): string {
  switch (material) {
    case 'pale-jade':
      return 'jade-stone'
    case 'boxwood':
      return 'boxwood'
    case 'ivory-stone':
      return 'soft-felt'
  }
}

class JadePercussionPlayer {
  private ctx: AudioContext | null = null
  private buffers = new Map<string, AudioBuffer>()
  private placeToggle = 0

  async play(material: MaterialFamilyId, event: PercussionEvent, enabled: boolean, isScrubbing = false): Promise<void> {
    if (!enabled || isScrubbing || typeof window === 'undefined') return
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') return

    const AudioCtx = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
    if (!AudioCtx) return

    if (!this.ctx) {
      this.ctx = new AudioCtx()
    }
    if (this.ctx.state === 'suspended') {
      try {
        await this.ctx.resume()
      } catch {
        return
      }
    }

    const family = audioDirForMaterial(material)
    const slug =
      event === 'place'
        ? (() => {
            this.placeToggle = (this.placeToggle + 1) % 2
            return this.placeToggle === 0 ? 'place-1' : 'place-2'
          })()
        : event
    const url = `/assets/audio/${family}/${slug}.wav`

    try {
      let buf = this.buffers.get(url)
      if (!buf) {
        const res = await fetch(url)
        if (!res.ok) return
        const arr = await res.arrayBuffer()
        buf = await this.ctx.decodeAudioData(arr)
        this.buffers.set(url, buf)
      }
      const src = this.ctx.createBufferSource()
      const gain = this.ctx.createGain()
      gain.gain.value = 0.72
      src.buffer = buf
      src.connect(gain)
      gain.connect(this.ctx.destination)
      src.start(0)
    } catch {
      // Audio decode/playback failure is non-fatal
    }
  }
}

export const percussionPlayer = new JadePercussionPlayer()
