<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { hitTestBoard, type BoardViewSnapshot, type PieceViewRecord } from '~/utils/core-wasm'
import type { MaterialFamilyId, SupportedLocale } from '~/utils/i18n'
import { tTerm } from '~/utils/i18n'

const props = withDefaults(
  defineProps<{
    snapshot: BoardViewSnapshot
    material: MaterialFamilyId
    traditionalGlyphs: boolean
    darkSurface: boolean
    locale: SupportedLocale
    interactive?: boolean
    lastAnnouncement?: string
  }>(),
  {
    interactive: true,
    lastAnnouncement: '',
  },
)

const emit = defineEmits<{
  (e: 'selectSquare', squareUci: string | null): void
  (e: 'playMove', fromUci: string, toUci: string): void
}>()

const containerRef = ref<HTMLElement | null>(null)
const canvasRef = ref<HTMLCanvasElement | null>(null)
const canvasReady = ref(false)
const boardWidth = ref(480)
const boardHeight = computed(() => Math.round((boardWidth.value * 10) / 9))
const padding = computed(() => Math.max(14, Math.round(boardWidth.value * 0.045)))

// Drag state
const draggingPiece = ref<PieceViewRecord | null>(null)
const dragPos = ref<{ x: number; y: number } | null>(null)

// Keyboard roving focus (file 0..8, rank 0..9)
const focusSquare = ref('e0')

const spriteCache = new Map<string, HTMLImageElement>()
let resizeObserver: ResizeObserver | null = null

const redBelow = computed(() => props.snapshot.orientation === 'red_below')

function squareToGrid(file: number, rank: number): { col: number; row: number } {
  return redBelow.value
    ? { col: file, row: 9 - rank }
    : { col: 8 - file, row: rank }
}

function uciFromFileRank(file: number, rank: number): string {
  return `${String.fromCharCode(97 + file)}${rank}`
}

function parseSquareUci(uci: string): { file: number; rank: number } {
  return {
    file: uci.charCodeAt(0) - 97,
    rank: Number(uci[1]),
  }
}

const geom = computed(() => {
  const w = boardWidth.value
  const h = boardHeight.value
  const pad = padding.value
  const usableW = w - 2 * pad
  const usableH = h - 2 * pad
  const pitch = Math.min(usableW / 9, usableH / 10)
  const gridOriginX = (w - 8 * pitch) * 0.5
  const gridOriginY = (h - 9 * pitch) * 0.5
  return {
    w,
    h,
    pad,
    pitch,
    gridOriginX,
    gridOriginY,
    pieceDiameter: pitch * 0.88,
  }
})

function centerForFileRank(file: number, rank: number): { x: number; y: number } {
  const { col, row } = squareToGrid(file, rank)
  return {
    x: geom.value.gridOriginX + col * geom.value.pitch,
    y: geom.value.gridOriginY + row * geom.value.pitch,
  }
}

const pieceMap = computed(() => {
  const m = new Map<string, PieceViewRecord>()
  for (const p of props.snapshot.pieces) {
    m.set(p.square, p)
  }
  return m
})

const legalDestSet = computed(() => new Set(props.snapshot.legal_destinations))

function spriteUrl(piece: PieceViewRecord, hiDpi: boolean): string {
  const scriptDir = props.traditionalGlyphs ? 'traditional' : 'simplified'
  const suffix = hiDpi ? '@2x' : ''
  return `/assets/board/materials/${props.material}/${scriptDir}/${piece.side}-${piece.role}${suffix}.png`
}

function loadSprite(url: string): Promise<HTMLImageElement | null> {
  const existing = spriteCache.get(url)
  if (existing && existing.complete && existing.naturalWidth > 0) {
    return Promise.resolve(existing)
  }
  return new Promise((resolve) => {
    const img = new Image()
    img.decoding = 'async'
    img.onload = () => {
      spriteCache.set(url, img)
      resolve(img)
    }
    img.onerror = () => resolve(null)
    img.src = url
  })
}

function drawFiducial(ctx: CanvasRenderingContext2D, cx: number, cy: number, file: number, pitch: number) {
  const gap = Math.max(2.5, pitch * 0.08)
  const arm = Math.max(5, pitch * 0.18)
  const drawLeft = file > 0
  const drawRight = file < 8
  const quadrants: [number, number, boolean][] = [
    [-1, -1, drawLeft],
    [1, -1, drawRight],
    [-1, 1, drawLeft],
    [1, 1, drawRight],
  ]
  for (const [sx, sy, ok] of quadrants) {
    if (!ok) continue
    ctx.beginPath()
    ctx.moveTo(cx + sx * (gap + arm), cy + sy * gap)
    ctx.lineTo(cx + sx * gap, cy + sy * gap)
    ctx.lineTo(cx + sx * gap, cy + sy * (gap + arm))
    ctx.stroke()
  }
}

async function renderCanvas() {
  if (typeof window === 'undefined' || !canvasRef.value) return
  const canvas = canvasRef.value
  const dpr = Math.min(window.devicePixelRatio || 1, 2)
  const hiDpi = dpr > 1.25
  const { w, h, pitch, gridOriginX, gridOriginY, pieceDiameter } = geom.value

  canvas.width = Math.round(w * dpr)
  canvas.height = Math.round(h * dpr)
  const ctx = canvas.getContext('2d')
  if (!ctx) return

  // Preload active sprites & shadow
  const shadowUrl = `/assets/board/materials/contact-shadow${hiDpi ? '@2x' : ''}.png`
  const grainUrl = props.darkSurface
    ? '/assets/board/materials/dark-grain.png'
    : '/assets/board/materials/paper-grain.png'
  const urlsToLoad = new Set<string>([shadowUrl, grainUrl])
  for (const p of props.snapshot.pieces) {
    urlsToLoad.add(spriteUrl(p, hiDpi))
  }
  await Promise.all([...urlsToLoad].map(loadSprite))

  ctx.save()
  ctx.scale(dpr, dpr)

  // 1. Calm paper / dark jade board surface + subtle procedural grain tile
  ctx.fillStyle = props.darkSurface ? '#222B27' : '#EDE5D6'
  ctx.fillRect(0, 0, w, h)
  const grainImg = spriteCache.get(grainUrl)
  if (grainImg && grainImg.naturalWidth > 0) {
    const pattern = ctx.createPattern(grainImg, 'repeat')
    if (pattern) {
      ctx.save()
      ctx.globalAlpha = props.darkSurface ? 0.24 : 0.32
      ctx.fillStyle = pattern
      ctx.fillRect(0, 0, w, h)
      ctx.restore()
    }
  }

  // Outer double frame around 9x10 grid
  const gx0 = gridOriginX
  const gy0 = gridOriginY
  const gx1 = gridOriginX + 8 * pitch
  const gy1 = gridOriginY + 9 * pitch
  const lineInk = props.darkSurface ? 'rgba(220, 227, 206, 0.72)' : 'rgba(37, 42, 37, 0.76)'

  ctx.strokeStyle = lineInk
  ctx.lineWidth = 2.0
  ctx.strokeRect(gx0 - 4, gy0 - 4, gx1 - gx0 + 8, gy1 - gy0 + 8)

  // 2. Crisp 9x10 grid lines (files break across the river except outer files 0 and 8)
  ctx.lineWidth = 1.15
  for (let r = 0; r < 10; r++) {
    const y = gy0 + r * pitch
    ctx.beginPath()
    ctx.moveTo(gx0, y)
    ctx.lineTo(gx1, y)
    ctx.stroke()
  }
  for (let f = 0; f < 9; f++) {
    const x = gx0 + f * pitch
    if (f === 0 || f === 8) {
      ctx.beginPath()
      ctx.moveTo(x, gy0)
      ctx.lineTo(x, gy1)
      ctx.stroke()
    } else {
      ctx.beginPath()
      ctx.moveTo(x, gy0)
      ctx.lineTo(x, gy0 + 4 * pitch)
      ctx.moveTo(x, gy0 + 5 * pitch)
      ctx.lineTo(x, gy1)
      ctx.stroke()
    }
  }

  // Palace diagonals
  ctx.beginPath()
  ctx.moveTo(gx0 + 3 * pitch, gy0)
  ctx.lineTo(gx0 + 5 * pitch, gy0 + 2 * pitch)
  ctx.moveTo(gx0 + 5 * pitch, gy0)
  ctx.lineTo(gx0 + 3 * pitch, gy0 + 2 * pitch)
  ctx.moveTo(gx0 + 3 * pitch, gy0 + 7 * pitch)
  ctx.lineTo(gx0 + 5 * pitch, gy0 + 9 * pitch)
  ctx.moveTo(gx0 + 5 * pitch, gy0 + 7 * pitch)
  ctx.lineTo(gx0 + 3 * pitch, gy0 + 9 * pitch)
  ctx.stroke()

  // Fiducial marks on cannon & soldier start intersections
  const fiducialSquares: [number, number][] = [
    [1, 2], [7, 2], [0, 3], [2, 3], [4, 3], [6, 3], [8, 3],
    [1, 7], [7, 7], [0, 6], [2, 6], [4, 6], [6, 6], [8, 6],
  ]
  ctx.lineWidth = 1.1
  for (const [f, r] of fiducialSquares) {
    const pt = centerForFileRank(f, r)
    drawFiducial(ctx, pt.x, pt.y, redBelow.value ? f : 8 - f, pitch)
  }

  // River inscription (`楚河` / `漢界` or `汉界`)
  ctx.save()
  ctx.fillStyle = props.darkSurface ? 'rgba(220, 227, 206, 0.56)' : 'rgba(65, 84, 71, 0.68)'
  ctx.font = `600 ${Math.round(pitch * 0.44)}px "Noto Serif CJK SC", "Songti SC", Georgia, serif`
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  const riverY = gy0 + 4.5 * pitch
  ctx.fillText('楚 河', gx0 + 2 * pitch, riverY)
  ctx.fillText(props.traditionalGlyphs ? '漢 界' : '汉 界', gx0 + 6 * pitch, riverY)
  ctx.restore()

  // 3. Last-move corner brackets (shape cue, never color-only!)
  if (props.snapshot.last_move) {
    for (const sqUci of [props.snapshot.last_move.from, props.snapshot.last_move.to]) {
      const { file, rank } = parseSquareUci(sqUci)
      const pt = centerForFileRank(file, rank)
      const half = pieceDiameter * 0.52
      const b = Math.max(5, pitch * 0.16)
      ctx.strokeStyle = props.darkSurface ? '#DCE3CE' : '#415447'
      ctx.lineWidth = 2.0
      for (const [sx, sy] of [[-1, -1], [1, -1], [-1, 1], [1, 1]] as const) {
        ctx.beginPath()
        ctx.moveTo(pt.x + sx * (half - b), pt.y + sy * half)
        ctx.lineTo(pt.x + sx * half, pt.y + sy * half)
        ctx.lineTo(pt.x + sx * half, pt.y + sy * (half - b))
        ctx.stroke()
      }
    }
  }

  // 4. Check warning double ring + vermilion tick halo on threatened General
  if (props.snapshot.check_square) {
    const { file, rank } = parseSquareUci(props.snapshot.check_square)
    const pt = centerForFileRank(file, rank)
    ctx.save()
    ctx.strokeStyle = '#A54837'
    ctx.lineWidth = 2.6
    ctx.setLineDash([5, 3])
    ctx.beginPath()
    ctx.arc(pt.x, pt.y, pieceDiameter * 0.58, 0, Math.PI * 2)
    ctx.stroke()
    ctx.restore()
  }

  // 5. Pieces (contact shadow + baked orthographic 2.5D material sprite)
  const shadowImg = spriteCache.get(shadowUrl)
  const spriteBox = pieceDiameter / 0.88 // matches LOGICAL_SPRITE_SIZE disc radius ratio (0.44 * 2 = 0.88)

  for (const piece of props.snapshot.pieces) {
    if (draggingPiece.value?.piece_id === piece.piece_id && dragPos.value) {
      continue
    }
    const pt = centerForFileRank(piece.file, piece.rank)
    drawSinglePiece(ctx, piece, pt.x, pt.y, spriteBox, pieceDiameter, hiDpi, shadowImg, false)
  }

  // 6. Selected piece double ring + legal destination pips/capture rings (non-color geometry cues!)
  if (props.snapshot.selection) {
    const { file, rank } = parseSquareUci(props.snapshot.selection)
    const pt = centerForFileRank(file, rank)
    ctx.strokeStyle = props.darkSurface ? '#DCE3CE' : '#415447'
    ctx.lineWidth = 2.5
    ctx.beginPath()
    ctx.arc(pt.x, pt.y, pieceDiameter * 0.54, 0, Math.PI * 2)
    ctx.stroke()
  }

  for (const destUci of props.snapshot.legal_destinations) {
    const { file, rank } = parseSquareUci(destUci)
    const pt = centerForFileRank(file, rank)
    const hasCapture = pieceMap.value.has(destUci)
    if (hasCapture) {
      ctx.save()
      ctx.strokeStyle = '#A54837'
      ctx.lineWidth = 2.5
      ctx.beginPath()
      ctx.arc(pt.x, pt.y, pieceDiameter * 0.53, 0, Math.PI * 2)
      ctx.stroke()
      ctx.restore()
    } else {
      ctx.fillStyle = props.darkSurface ? 'rgba(220, 227, 206, 0.85)' : 'rgba(65, 84, 71, 0.78)'
      ctx.beginPath()
      ctx.arc(pt.x, pt.y, Math.max(4.5, pitch * 0.13), 0, Math.PI * 2)
      ctx.fill()
    }
  }

  // 7. Lifted dragged piece on top
  if (draggingPiece.value && dragPos.value) {
    drawSinglePiece(
      ctx,
      draggingPiece.value,
      dragPos.value.x,
      dragPos.value.y - 4,
      spriteBox * 1.05,
      pieceDiameter * 1.05,
      hiDpi,
      shadowImg,
      true,
    )
  }

  ctx.restore()
  canvasReady.value = true
}

function drawSinglePiece(
  ctx: CanvasRenderingContext2D,
  piece: PieceViewRecord,
  cx: number,
  cy: number,
  spriteBox: number,
  discDiam: number,
  hiDpi: boolean,
  shadowImg: HTMLImageElement | undefined,
  lifted: boolean,
) {
  if (shadowImg && shadowImg.naturalWidth > 0) {
    const liftOffset = lifted ? 4 : 0
    ctx.drawImage(
      shadowImg,
      cx - spriteBox * 0.5 + liftOffset * 0.6,
      cy - spriteBox * 0.5 + liftOffset,
      spriteBox,
      spriteBox,
    )
  }
  const img = spriteCache.get(spriteUrl(piece, hiDpi))
  if (img && img.naturalWidth > 0) {
    ctx.drawImage(img, cx - spriteBox * 0.5, cy - spriteBox * 0.5, spriteBox, spriteBox)
  } else {
    // Immediate vector fallback before sprite decode finishes
    ctx.fillStyle = props.material === 'pale-jade' ? '#DCE3CE' : props.material === 'boxwood' ? '#DFC394' : '#EAE4D7'
    ctx.beginPath()
    ctx.arc(cx, cy, discDiam * 0.5, 0, Math.PI * 2)
    ctx.fill()
    ctx.strokeStyle = piece.side === 'red' ? '#A54837' : '#252A25'
    ctx.lineWidth = 1.6
    ctx.stroke()
    ctx.fillStyle = piece.side === 'red' ? '#A54837' : '#252A25'
    ctx.font = `700 ${Math.round(discDiam * 0.54)}px "Noto Serif CJK SC", serif`
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    ctx.fillText(piece.glyph, cx, cy + 1)
  }
}

function localPoint(ev: PointerEvent): { x: number; y: number } | null {
  const el = canvasRef.value
  if (!el) return null
  const rect = el.getBoundingClientRect()
  return {
    x: ev.clientX - rect.left,
    y: ev.clientY - rect.top,
  }
}

async function onPointerDown(ev: PointerEvent) {
  if (!props.interactive) return
  const pt = localPoint(ev)
  if (!pt) return
  const sq = await hitTestBoard({
    width: boardWidth.value,
    height: boardHeight.value,
    padding: padding.value,
    redBelow: redBelow.value,
    px: pt.x,
    py: pt.y,
  })
  if (!sq) {
    emit('selectSquare', null)
    return
  }
  focusSquare.value = sq

  if (props.snapshot.selection && legalDestSet.value.has(sq)) {
    emit('playMove', props.snapshot.selection, sq)
    return
  }

  const piece = pieceMap.value.get(sq)
  if (piece && piece.side === props.snapshot.side_to_move) {
    emit('selectSquare', sq)
    draggingPiece.value = piece
    dragPos.value = pt
    ;(ev.currentTarget as HTMLElement | null)?.setPointerCapture?.(ev.pointerId)
    void renderCanvas()
  } else {
    emit('selectSquare', null)
  }
}

function onPointerMove(ev: PointerEvent) {
  if (!draggingPiece.value) return
  const pt = localPoint(ev)
  if (!pt) return
  dragPos.value = pt
  void renderCanvas()
}

async function onPointerUp(ev: PointerEvent) {
  if (!draggingPiece.value) return
  const originPiece = draggingPiece.value
  draggingPiece.value = null
  dragPos.value = null
  const pt = localPoint(ev)
  void renderCanvas()
  if (!pt) return

  const dropSq = await hitTestBoard({
    width: boardWidth.value,
    height: boardHeight.value,
    padding: padding.value,
    redBelow: redBelow.value,
    px: pt.x,
    py: pt.y,
  })
  if (dropSq && dropSq !== originPiece.square && legalDestSet.value.has(dropSq)) {
    emit('playMove', originPiece.square, dropSq)
  }
}

function ariaLabelForSquare(file: number, rank: number): string {
  const uci = uciFromFileRank(file, rank)
  const piece = pieceMap.value.get(uci)
  const parts = [`${uci.toUpperCase()}`]
  if (piece) {
    const name = tTerm(`piece.${piece.side}.${piece.role}`, props.locale)
    parts.push(`${name} (${piece.glyph})`)
  }
  if (props.snapshot.selection === uci) {
    parts.push('selected')
  } else if (legalDestSet.value.has(uci)) {
    parts.push('legal destination')
  }
  if (props.snapshot.check_square === uci) {
    parts.push(tTerm('state.check', props.locale))
  }
  return parts.join(', ')
}

function handleSquareActivate(uci: string) {
  if (!props.interactive) return
  focusSquare.value = uci
  if (props.snapshot.selection && legalDestSet.value.has(uci)) {
    emit('playMove', props.snapshot.selection, uci)
    return
  }
  const piece = pieceMap.value.get(uci)
  if (piece && piece.side === props.snapshot.side_to_move) {
    emit('selectSquare', uci)
  } else {
    emit('selectSquare', null)
  }
}

function handleGridKeydown(ev: KeyboardEvent, file: number, rank: number) {
  const stepSign = redBelow.value ? 1 : -1
  let nf = file
  let nr = rank
  if (ev.key === 'ArrowLeft') nf = Math.max(0, Math.min(8, file - stepSign))
  else if (ev.key === 'ArrowRight') nf = Math.max(0, Math.min(8, file + stepSign))
  else if (ev.key === 'ArrowUp') nr = Math.max(0, Math.min(9, rank + stepSign))
  else if (ev.key === 'ArrowDown') nr = Math.max(0, Math.min(9, rank - stepSign))
  else if (ev.key === 'Escape') {
    emit('selectSquare', null)
    return
  } else if (ev.key === 'Enter' || ev.key === ' ') {
    ev.preventDefault()
    handleSquareActivate(uciFromFileRank(file, rank))
    return
  } else {
    return
  }

  ev.preventDefault()
  const targetUci = uciFromFileRank(nf, nr)
  focusSquare.value = targetUci
  nextTick(() => {
    const btn = containerRef.value?.querySelector<HTMLButtonElement>(`[data-square="${targetUci}"]`)
    btn?.focus()
  })
}

const allSquaresOrdered = computed(() => {
  const rows: { file: number; rank: number; uci: string }[] = []
  for (let row = 0; row < 10; row++) {
    const rank = redBelow.value ? 9 - row : row
    for (let col = 0; col < 9; col++) {
      const file = redBelow.value ? col : 8 - col
      rows.push({ file, rank, uci: uciFromFileRank(file, rank) })
    }
  }
  return rows
})

onMounted(() => {
  if (containerRef.value) {
    const measure = () => {
      if (!containerRef.value) return
      const avail = containerRef.value.clientWidth || 340
      const w = Math.max(260, Math.min(540, Math.floor(avail)))
      boardWidth.value = w
      void renderCanvas()
    }
    measure()
    if (typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(measure)
      resizeObserver.observe(containerRef.value)
    }
  }
})

onBeforeUnmount(() => {
  resizeObserver?.disconnect()
})

watch(
  () => [props.snapshot, props.material, props.traditionalGlyphs, props.darkSurface] as const,
  () => {
    void renderCanvas()
  },
  { deep: true },
)
</script>

<template>
  <div
    ref="containerRef"
    class="jade-board-frame"
    :class="{ 'is-dark': darkSurface }"
    data-testid="xiangqi-board"
  >
    <div
      class="jade-board-stage"
      :style="{ width: `${boardWidth}px`, height: `${boardHeight}px` }"
    >
      <!-- SSR & No-JS crisp vector board fallback -->
      <svg
        v-show="!canvasReady"
        class="jade-board-svg-fallback"
        :viewBox="`0 0 ${geom.w} ${geom.h}`"
        aria-hidden="true"
      >
        <rect :width="geom.w" :height="geom.h" :fill="darkSurface ? '#222B27' : '#EDE5D6'" rx="8" />
        <g :stroke="darkSurface ? '#DCE3CE' : '#252A25'" stroke-opacity="0.72" stroke-width="1.2">
          <line
            v-for="r in 10"
            :key="`hr-${r}`"
            :x1="geom.gridOriginX"
            :y1="geom.gridOriginY + (r - 1) * geom.pitch"
            :x2="geom.gridOriginX + 8 * geom.pitch"
            :y2="geom.gridOriginY + (r - 1) * geom.pitch"
          />
          <template v-for="f in 9" :key="`vf-${f}`">
            <line
              v-if="f === 1 || f === 9"
              :x1="geom.gridOriginX + (f - 1) * geom.pitch"
              :y1="geom.gridOriginY"
              :x2="geom.gridOriginX + (f - 1) * geom.pitch"
              :y2="geom.gridOriginY + 9 * geom.pitch"
            />
            <template v-else>
              <line
                :x1="geom.gridOriginX + (f - 1) * geom.pitch"
                :y1="geom.gridOriginY"
                :x2="geom.gridOriginX + (f - 1) * geom.pitch"
                :y2="geom.gridOriginY + 4 * geom.pitch"
              />
              <line
                :x1="geom.gridOriginX + (f - 1) * geom.pitch"
                :y1="geom.gridOriginY + 5 * geom.pitch"
                :x2="geom.gridOriginX + (f - 1) * geom.pitch"
                :y2="geom.gridOriginY + 9 * geom.pitch"
              />
            </template>
          </template>
        </g>
        <g v-for="piece in snapshot.pieces" :key="piece.piece_id">
          <circle
            :cx="centerForFileRank(piece.file, piece.rank).x"
            :cy="centerForFileRank(piece.file, piece.rank).y"
            :r="geom.pieceDiameter * 0.5"
            fill="#DCE3CE"
            :stroke="piece.side === 'red' ? '#A54837' : '#252A25'"
            stroke-width="1.6"
          />
          <text
            :x="centerForFileRank(piece.file, piece.rank).x"
            :y="centerForFileRank(piece.file, piece.rank).y + 6"
            text-anchor="middle"
            :fill="piece.side === 'red' ? '#A54837' : '#252A25'"
            font-size="20"
            font-weight="700"
          >
            {{ piece.glyph }}
          </text>
        </g>
      </svg>

      <!-- Mounted HiDPI Canvas 2D renderer with baked 2.5D material sprites -->
      <canvas
        ref="canvasRef"
        class="jade-board-canvas"
        :data-canvas-ready="canvasReady ? 'true' : 'false'"
        :style="{ width: `${boardWidth}px`, height: `${boardHeight}px` }"
        @pointerdown="onPointerDown"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
        @pointercancel="onPointerUp"
      />

      <!-- WCAG 2.2 Keyboard & Screen-Reader Roving Intersection Grid -->
      <div
        class="jade-board-a11y-grid"
        role="grid"
        aria-label="Xiangqi 9 by 10 board"
      >
        <button
          v-for="sq in allSquaresOrdered"
          :key="sq.uci"
          type="button"
          class="jade-sq-a11y-btn"
          :data-square="sq.uci"
          :tabindex="focusSquare === sq.uci ? 0 : -1"
          :aria-label="ariaLabelForSquare(sq.file, sq.rank)"
          :style="{
            left: `${centerForFileRank(sq.file, sq.rank).x - geom.pitch * 0.44}px`,
            top: `${centerForFileRank(sq.file, sq.rank).y - geom.pitch * 0.44}px`,
            width: `${geom.pitch * 0.88}px`,
            height: `${geom.pitch * 0.88}px`,
          }"
          @click="handleSquareActivate(sq.uci)"
          @keydown="handleGridKeydown($event, sq.file, sq.rank)"
        />
      </div>
    </div>

    <div class="sr-only" aria-live="polite" aria-atomic="true">
      {{ lastAnnouncement }}
    </div>
  </div>
</template>

<style scoped>
.jade-board-frame {
  width: 100%;
  max-width: 520px;
  margin: 0 auto;
  display: flex;
  justify-content: center;
  user-select: none;
  touch-action: none;
}

.jade-board-stage {
  position: relative;
  border-radius: 10px;
  overflow: hidden;
  box-shadow: 0 10px 28px rgba(37, 42, 37, 0.14), 0 2px 6px rgba(37, 42, 37, 0.08);
  border: 1px solid rgba(65, 84, 71, 0.24);
}

.jade-board-frame.is-dark .jade-board-stage {
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.42);
  border-color: rgba(220, 227, 206, 0.2);
}

.jade-board-svg-fallback,
.jade-board-canvas {
  display: block;
  width: 100%;
  height: 100%;
}

.jade-board-a11y-grid {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.jade-sq-a11y-btn {
  position: absolute;
  background: transparent;
  border: none;
  padding: 0;
  margin: 0;
  border-radius: 50%;
  pointer-events: none;
}

.jade-sq-a11y-btn:focus-visible {
  outline: 3px solid #A54837;
  outline-offset: 2px;
  background: rgba(165, 72, 55, 0.12);
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
</style>
