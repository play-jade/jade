package play.jade.app

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import kotlin.math.min
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class JadeApkLaunchAndMoveInstrumentationTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<MainActivity>()

    @Test
    fun launchesPackagedApkRendersInitialBoardAndAppliesCentralCannonMove() {
        // Ensure clean initial state in case offline DB had prior moves
        composeRule.onNodeWithText("New").performClick()
        composeRule.waitForIdle()

        // 1. Verify native `libcore_ffi.so` loaded from APK and rendered the 32-piece starting board
        composeRule.onNodeWithTag("jade-status-text")
            .assertIsDisplayed()
            .assertTextContains("Red to move · 0 plies")

        val boardNode = composeRule.onNodeWithTag("jade-compose-board").assertIsDisplayed()
        val initialDesc = boardNode.fetchSemanticsNode()
            .config[SemanticsProperties.ContentDescription]
            .joinToString(" ")
        assertTrue(
            "Expected 32 pieces on initial board, got: $initialDesc",
            initialDesc.contains("32 pieces on board")
        )

        // 2. Tap `h2` (file = 7, rank = 2) then `e2` (file = 4, rank = 2) on the Compose board
        val bounds = boardNode.fetchSemanticsNode().boundsInRoot
        val w = bounds.width
        val h = bounds.height
        val p = 18f
        val s = min((w - 2f * p) / 9f, (h - 2f * p) / 10f)
        val ox = (w - 8f * s) / 2f
        val oy = (h - 9f * s) / 2f

        fun squarePixel(file: Int, rank: Int) = Offset(ox + file * s, oy + (9 - rank) * s)

        boardNode.performTouchInput { click(squarePixel(7, 2)) }
        composeRule.waitForIdle()

        val selectedDesc = boardNode.fetchSemanticsNode()
            .config[SemanticsProperties.ContentDescription]
            .joinToString(" ")
        assertTrue(
            "Expected selected square h2, got: $selectedDesc",
            selectedDesc.contains("selected square h2")
        )

        boardNode.performTouchInput { click(squarePixel(4, 2)) }
        composeRule.waitForIdle()

        // 3. Verify move `h2e2` (`炮二平五`) executed through packaged Rust FFI and updated turn + notation
        composeRule.onNodeWithTag("jade-status-text")
            .assertTextContains("Black to move · 1 plies")
        composeRule.onNodeWithTag("jade-last-move-text")
            .assertIsDisplayed()
            .assertTextContains("炮二平五")
    }
}
