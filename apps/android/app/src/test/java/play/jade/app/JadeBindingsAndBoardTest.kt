package play.jade.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.core_ffi.ffiHitTest

class JadeBindingsAndBoardTest {
    @Test
    fun renderModelProvides32PiecesLegalDestinationsAndMoveProgression() {
        val initial = computeAndroidRenderModel(
            moves = emptyList(),
            selectedSquare = 7 to 2, // Red right cannon at h2
            traditionalGlyphs = false
        )
        assertEquals(32, initial.pieces.size)
        assertEquals("red", initial.sideToMove)
        assertEquals("h2", initial.selectedUci)
        assertTrue(initial.legalDestsUci.contains("e2"))

        val afterMoves = computeAndroidRenderModel(
            moves = listOf("h2e2", "h9g7"),
            selectedSquare = null,
            traditionalGlyphs = true
        )
        assertEquals(2u, afterMoves.plyCount)
        assertEquals("h9", afterMoves.lastMoveFromUci)
        assertEquals("g7", afterMoves.lastMoveToUci)
        val blackHorse = afterMoves.pieces.firstOrNull { it.uci == "g7" }
        assertNotNull(blackHorse)
        assertEquals("馬", blackHorse!!.glyph)

        val centerHit = ffiHitTest(480.0, 533.0, 18.0, true, 240.0, 400.0)
        assertNotNull(centerHit)
    }
}
