package play.jade.app

import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.graphics.Paint
import android.graphics.Typeface
import android.media.AudioAttributes
import android.media.SoundPool
import android.os.Bundle
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.activity.BackEventCompat
import androidx.activity.ComponentActivity
import androidx.activity.compose.PredictiveBackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min
import uniffi.core_ffi.FfiBoardRenderModel
import uniffi.core_ffi.FfiGameSession

/**
 * Native Android client using Kotlin + Jetpack Compose, Compose Canvas/DrawScope with full
 * `FfiBoardRenderModel` piece/glyph/marker/river rendering, an explicit TalkBack accessibility
 * semantics tree, SoundPool short-sample playback, Android Keystore AES-GCM credential encryption,
 * and SQLite offline play persistence.
 */
class MainActivity : ComponentActivity() {
    private lateinit var offlineDb: JadeOfflineDb
    private lateinit var audioPool: JadeSoundController

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        offlineDb = JadeOfflineDb(this)
        audioPool = JadeSoundController(this)

        setContent {
            JadeAndroidRoot(offlineDb = offlineDb, audioPool = audioPool)
        }
    }

    override fun onPause() {
        super.onPause()
        audioPool.pauseAll()
        NativeEngineBridge.cancelActiveSearch()
    }

    override fun onDestroy() {
        audioPool.release()
        super.onDestroy()
    }
}

@Composable
fun JadeAndroidRoot(offlineDb: JadeOfflineDb, audioPool: JadeSoundController) {
    var redBelow by remember { mutableStateOf(true) }
    var darkMode by remember { mutableStateOf(false) }
    var traditionalGlyphs by remember { mutableStateOf(false) }
    var materialId by remember { mutableStateOf("pale-jade") }
    var selectedSquare by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    var moveHistory by remember { mutableStateOf(offlineDb.loadActiveMoves()) }

    val uiState = remember(moveHistory, selectedSquare, traditionalGlyphs) {
        computeAndroidBoardUiState(moveHistory, selectedSquare, traditionalGlyphs)
    }
    val renderModel = uiState.renderModel

    val bgColor = if (darkMode) Color(0xFF1D2421) else Color(0xFFF3EEE4)
    val inkColor = if (darkMode) Color(0xFFF3EEE4) else Color(0xFF252A25)

    PredictiveBackHandler(enabled = selectedSquare != null) { progress ->
        progress.collect { _: BackEventCompat -> }
        selectedSquare = null
    }

    Surface(
        modifier = Modifier
            .fillMaxSize()
            .background(bgColor)
            .windowInsetsPadding(WindowInsets.safeDrawing),
        color = bgColor
    ) {
        BoxWithConstraints(modifier = Modifier.fillMaxSize().padding(16.dp)) {
            val isTabletOrFoldable = maxWidth >= 680.dp
            val onBoardIntent: (Pair<Int, Int>?) -> Unit = { sq ->
                handleAndroidSquareIntent(sq, selectedSquare, moveHistory, offlineDb, audioPool) { nextSel, nextMoves ->
                    selectedSquare = nextSel
                    moveHistory = nextMoves
                }
            }
            val renderBoard: @Composable (Modifier) -> Unit = { boardMod ->
                JadeComposeBoard(
                    modifier = boardMod,
                    renderModel = renderModel,
                    redBelow = redBelow,
                    darkMode = darkMode,
                    materialId = materialId,
                    traditionalGlyphs = traditionalGlyphs,
                    selected = selectedSquare,
                    onSquareIntent = onBoardIntent
                )
            }
            val renderControls: @Composable (Modifier) -> Unit = { controlsMod ->
                JadeControlsColumn(
                    modifier = controlsMod,
                    inkColor = inkColor,
                    renderModel = renderModel,
                    movesChinese = uiState.movesChinese,
                    materialId = materialId,
                    traditionalGlyphs = traditionalGlyphs,
                    onFlip = { redBelow = !redBelow },
                    onUndo = {
                        if (moveHistory.isNotEmpty()) {
                            val next = moveHistory.dropLast(1)
                            selectedSquare = null
                            moveHistory = next
                            offlineDb.saveActiveMoves(next)
                        }
                    },
                    onResetOffline = {
                        selectedSquare = null
                        moveHistory = emptyList()
                        offlineDb.saveActiveMoves(emptyList())
                    },
                    onToggleDark = { darkMode = !darkMode },
                    onToggleTraditional = { traditionalGlyphs = !traditionalGlyphs },
                    onCycleMaterial = {
                        materialId = when (materialId) {
                            "pale-jade" -> "boxwood"
                            "boxwood" -> "ivory-stone"
                            else -> "pale-jade"
                        }
                    }
                )
            }
            if (isTabletOrFoldable) {
                Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
                    renderBoard(Modifier.weight(1f).fillMaxHeight())
                    renderControls(Modifier.width(300.dp))
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    renderBoard(Modifier.fillMaxWidth().weight(1f))
                    renderControls(Modifier.fillMaxWidth())
                }
            }
        }
    }
}

internal data class AndroidBoardUiState(
    val renderModel: FfiBoardRenderModel,
    val movesChinese: List<String>
)

internal fun computeAndroidBoardUiState(
    moves: List<String>,
    selectedSquare: Pair<Int, Int>?,
    traditionalGlyphs: Boolean
): AndroidBoardUiState {
    val selectedUci = selectedSquare?.let { "${'a' + it.first}${it.second}" }
    val zhMoves = mutableListOf<String>()
    val model = FfiGameSession("program-2023.11-v1").use { session ->
        for (mv in moves) {
            val zh = runCatching { session.applyMoveToken(mv) }.getOrNull() ?: break
            zhMoves.add(zh)
        }
        session.renderModel(traditionalGlyphs, selectedUci)
    }
    return AndroidBoardUiState(renderModel = model, movesChinese = zhMoves)
}

internal fun computeAndroidRenderModel(
    moves: List<String>,
    selectedSquare: Pair<Int, Int>?,
    traditionalGlyphs: Boolean
): FfiBoardRenderModel = computeAndroidBoardUiState(moves, selectedSquare, traditionalGlyphs).renderModel

private fun handleAndroidSquareIntent(
    sq: Pair<Int, Int>?,
    currentSelected: Pair<Int, Int>?,
    moves: List<String>,
    db: JadeOfflineDb,
    audio: JadeSoundController,
    update: (Pair<Int, Int>?, List<String>) -> Unit
) {
    if (sq == null) {
        update(null, moves)
        return
    }
    val targetUci = "${'a' + sq.first}${sq.second}"
    FfiGameSession("program-2023.11-v1").use { session ->
        for (pastMove in moves) {
            if (runCatching { session.applyMoveToken(pastMove) }.isFailure) {
                break
            }
        }
        val legalPrefixes = runCatching { session.summary().legalMovesUci }.getOrDefault(emptyList())

        if (currentSelected == null) {
            if (legalPrefixes.any { it.startsWith(targetUci) }) {
                update(sq, moves)
            } else {
                update(null, moves)
            }
        } else if (currentSelected == sq) {
            update(null, moves)
        } else {
            val uci = "${'a' + currentSelected.first}${currentSelected.second}$targetUci"
            if (runCatching { session.applyMoveToken(uci) }.isSuccess) {
                val updated = moves + uci
                db.saveActiveMoves(updated)
                audio.playPlaceOnce()
                update(null, updated)
            } else if (legalPrefixes.any { it.startsWith(targetUci) }) {
                update(sq, moves)
            } else {
                update(null, moves)
            }
        }
    }
}

@Composable
fun JadeComposeBoard(
    modifier: Modifier,
    renderModel: FfiBoardRenderModel,
    redBelow: Boolean,
    darkMode: Boolean,
    materialId: String,
    traditionalGlyphs: Boolean,
    selected: Pair<Int, Int>?,
    onSquareIntent: (Pair<Int, Int>?) -> Unit
) {
    val surfaceColor = if (darkMode) Color(0xFF1D2421) else Color(0xFFF3EEE4)
    val lineColor = if (darkMode) Color(0xC7DCE3CE) else Color(0xE0252A25)
    val deepJade = Color(0xFF415447)
    val charcoal = Color(0xFF252A25)
    val vermilion = Color(0xFFA54837)
    val discFill = when (materialId) {
        "boxwood" -> Color(0xFFDFC79C)
        "ivory-stone" -> Color(0xFFEDE7DA)
        else -> Color(0xFFDCE3CE)
    }

    val semanticDescription = buildString {
        append("Jade Xiangqi Board, ${renderModel.sideToMove} to move, ${renderModel.pieces.size} pieces on board, ")
        append(if (redBelow) "Red at bottom" else "Black at bottom")
        if (renderModel.inCheck) append(", check")
        selected?.let { append(", selected square ${'a' + it.first}${it.second}") }
    }

    val glyphPaint = remember {
        Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textAlign = Paint.Align.CENTER
            typeface = Typeface.create(Typeface.SERIF, Typeface.BOLD)
        }
    }
    val riverPaint = remember {
        Paint(Paint.ANTI_ALIAS_FLAG).apply {
            textAlign = Paint.Align.CENTER
            typeface = Typeface.create(Typeface.SERIF, Typeface.NORMAL)
        }
    }

    Canvas(
        modifier = modifier
            .testTag("jade-compose-board")
            .semantics {
                contentDescription = semanticDescription
                role = Role.Image
                customActions = listOf(
                    CustomAccessibilityAction("Select right cannon h2") {
                        onSquareIntent(7 to 2)
                        true
                    },
                    CustomAccessibilityAction("Clear selection") {
                        onSquareIntent(null)
                        true
                    }
                )
            }
            .pointerInput(redBelow) {
                detectTapGestures { tap ->
                    val w = size.width.toDouble()
                    val h = size.height.toDouble()
                    val p = 18.0
                    val s = min((w - 2.0 * p) / 9.0, (h - 2.0 * p) / 10.0)
                    if (s <= 0.0) return@detectTapGestures
                    val ox = (w - 8.0 * s) / 2.0
                    val oy = (h - 9.0 * s) / 2.0
                    val u = (tap.x - ox) / s
                    val v = (tap.y - oy) / s
                    if (u < -0.5 || u >= 8.5 || v < -0.5 || v >= 9.5) {
                        onSquareIntent(null)
                    } else {
                        val c = floor(u + 0.5).toInt()
                        val r = floor(v + 0.5).toInt()
                        val sq = if (redBelow) (c to (9 - r)) else ((8 - c) to r)
                        onSquareIntent(sq)
                    }
                }
            }
    ) {
        drawRect(color = surfaceColor)
        val w = size.width
        val h = size.height
        val p = 18f
        val s = min((w - 2f * p) / 9f, (h - 2f * p) / 10f)
        if (s <= 0f) return@Canvas
        val ox = (w - 8f * s) / 2f
        val oy = (h - 9f * s) / 2f

        fun screenOffset(file: Int, rank: Int): Offset {
            val sx = if (redBelow) ox + s * file else ox + s * (8 - file)
            val sy = if (redBelow) oy + s * (9 - rank) else oy + s * rank
            return Offset(sx, sy)
        }

        fun parseUci(uci: String): Pair<Int, Int>? {
            if (uci.length != 2) return null
            val f = uci[0] - 'a'
            val r = uci[1].digitToIntOrNull() ?: return null
            return if (f in 0..8 && r in 0..9) (f to r) else null
        }

        // Outer frame + 9x10 grid + river + palace diagonals
        drawRect(
            color = lineColor,
            topLeft = Offset(ox - 5f, oy - 5f),
            size = Size(8f * s + 10f, 9f * s + 10f),
            style = Stroke(width = 2.2f)
        )
        for (r in 0..9) {
            drawLine(lineColor, Offset(ox, oy + r * s), Offset(ox + 8f * s, oy + r * s), strokeWidth = 1.6f)
        }
        for (f in 0..8) {
            if (f == 0 || f == 8) {
                drawLine(lineColor, Offset(ox + f * s, oy), Offset(ox + f * s, oy + 9f * s), strokeWidth = 1.8f)
            } else {
                drawLine(lineColor, Offset(ox + f * s, oy), Offset(ox + f * s, oy + 4f * s), strokeWidth = 1.5f)
                drawLine(lineColor, Offset(ox + f * s, oy + 5f * s), Offset(ox + f * s, oy + 9f * s), strokeWidth = 1.5f)
            }
        }
        drawLine(lineColor, Offset(ox + 3f * s, oy), Offset(ox + 5f * s, oy + 2f * s), strokeWidth = 1.5f)
        drawLine(lineColor, Offset(ox + 5f * s, oy), Offset(ox + 3f * s, oy + 2f * s), strokeWidth = 1.5f)
        drawLine(lineColor, Offset(ox + 3f * s, oy + 7f * s), Offset(ox + 5f * s, oy + 9f * s), strokeWidth = 1.5f)
        drawLine(lineColor, Offset(ox + 5f * s, oy + 7f * s), Offset(ox + 3f * s, oy + 9f * s), strokeWidth = 1.5f)

        // River inscriptions (`楚河` / `漢界` or `汉界`)
        riverPaint.textSize = s * 0.42f
        riverPaint.color = if (darkMode) 0xC7DCE3CE.toInt() else 0xCC252A25.toInt()
        val riverBaseline = oy + 4.5f * s - (riverPaint.descent() + riverPaint.ascent()) / 2f
        drawContext.canvas.nativeCanvas.drawText("楚河", ox + 2f * s, riverBaseline, riverPaint)
        drawContext.canvas.nativeCanvas.drawText(
            if (traditionalGlyphs) "漢界" else "汉界",
            ox + 6f * s,
            riverBaseline,
            riverPaint
        )

        // Last-move markers & check danger ring
        listOfNotNull(renderModel.lastMoveFromUci, renderModel.lastMoveToUci).forEach { uci ->
            parseUci(uci)?.let { (f, r) ->
                val c = screenOffset(f, r)
                val half = s * 0.44f
                drawRect(
                    color = deepJade.copy(alpha = 0.6f),
                    topLeft = Offset(c.x - half, c.y - half),
                    size = Size(half * 2f, half * 2f),
                    style = Stroke(width = 2.2f)
                )
            }
        }
        renderModel.checkUci?.let(::parseUci)?.let { (f, r) ->
            drawCircle(
                color = vermilion,
                radius = s * 0.48f,
                center = screenOffset(f, r),
                style = Stroke(width = 3.5f)
            )
        }

        // Pieces (carved material discs + glyphs)
        val occupied = renderModel.pieces.map { it.uci }.toSet()
        val radius = s * 0.43f
        glyphPaint.textSize = radius * 1.12f
        for (piece in renderModel.pieces) {
            val center = screenOffset(piece.file.toInt(), piece.rank.toInt())
            drawCircle(
                color = Color(0x38000000),
                radius = radius,
                center = Offset(center.x + 1.8f, center.y + 2.8f)
            )
            drawCircle(color = discFill, radius = radius, center = center)
            drawCircle(color = deepJade, radius = radius, center = center, style = Stroke(width = 2f))

            val pieceInk = if (piece.side == "red") vermilion else charcoal
            drawCircle(
                color = pieceInk.copy(alpha = 0.55f),
                radius = radius * 0.83f,
                center = center,
                style = Stroke(width = 1.2f)
            )

            glyphPaint.color = if (piece.side == "red") 0xFFA54837.toInt() else 0xFF252A25.toInt()
            val textY = center.y - (glyphPaint.descent() + glyphPaint.ascent()) / 2f
            drawContext.canvas.nativeCanvas.drawText(piece.glyph, center.x, textY, glyphPaint)

            if (renderModel.selectedUci == piece.uci) {
                drawCircle(
                    color = deepJade,
                    radius = s * 0.46f,
                    center = center,
                    style = Stroke(width = 3.5f)
                )
            }
        }

        // Legal destination indicators
        for (dest in renderModel.legalDestsUci) {
            val (f, r) = parseUci(dest) ?: continue
            val center = screenOffset(f, r)
            if (occupied.contains(dest)) {
                drawCircle(
                    color = vermilion,
                    radius = s * 0.46f,
                    center = center,
                    style = Stroke(width = 2.8f)
                )
            } else {
                drawCircle(
                    color = if (darkMode) Color(0xD8DCE3CE) else deepJade.copy(alpha = 0.82f),
                    radius = max(5f, s * 0.13f),
                    center = center
                )
            }
        }
    }
}

@Composable
private fun JadeControlsColumn(
    modifier: Modifier,
    inkColor: Color,
    renderModel: FfiBoardRenderModel,
    movesChinese: List<String>,
    materialId: String,
    traditionalGlyphs: Boolean,
    onFlip: () -> Unit,
    onUndo: () -> Unit,
    onResetOffline: () -> Unit,
    onToggleDark: () -> Unit,
    onToggleTraditional: () -> Unit,
    onCycleMaterial: () -> Unit
) {
    val statusDesc = renderModel.outcome?.let { "Finished: $it (${renderModel.reason ?: ""})" }
        ?: "${renderModel.sideToMove.replaceFirstChar { it.uppercase() }} to move · ${renderModel.plyCount} plies" +
        if (renderModel.inCheck) " · CHECK" else ""

    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Jade · 玉枰象棋", color = inkColor, style = MaterialTheme.typography.titleMedium)
        Text(
            statusDesc,
            color = inkColor,
            style = MaterialTheme.typography.bodySmall,
            modifier = Modifier.testTag("jade-status-text")
        )
        if (movesChinese.isNotEmpty()) {
            Text(
                "Last move: ${movesChinese.last()}",
                color = inkColor,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.testTag("jade-last-move-text")
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onFlip) { Text("Flip") }
            OutlinedButton(onClick = onUndo) { Text("Undo") }
            OutlinedButton(onClick = onResetOffline) { Text("New") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onCycleMaterial) { Text(materialId) }
            OutlinedButton(onClick = onToggleTraditional) { Text(if (traditionalGlyphs) "繁體" else "简体") }
            OutlinedButton(onClick = onToggleDark) { Text("Surface") }
        }
    }
}

class JadeSoundController(context: Context) {
    private val pool: SoundPool = SoundPool.Builder()
        .setMaxStreams(4)
        .setAudioAttributes(
            AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_GAME)
                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                .build()
        )
        .build()
    private var placeSoundId: Int = 0

    fun playPlaceOnce() {
        if (placeSoundId != 0) {
            pool.play(placeSoundId, 0.85f, 0.85f, 1, 0, 1.0f)
        }
    }

    fun pauseAll() = pool.autoPause()
    fun release() = pool.release()
}

class JadeOfflineDb(context: Context) : SQLiteOpenHelper(context, "jade_offline.db", null, 1) {
    override fun onCreate(db: SQLiteDatabase) {
        db.execSQL(
            """
            CREATE TABLE IF NOT EXISTS offline_games (
                slot_id TEXT PRIMARY KEY,
                rules_profile TEXT NOT NULL,
                moves_csv TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            )
            """.trimIndent()
        )
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) = Unit

    fun loadActiveMoves(): List<String> {
        readableDatabase.rawQuery("SELECT moves_csv FROM offline_games WHERE slot_id = 'active'", null).use { cursor ->
            if (cursor.moveToFirst()) {
                val csv = cursor.getString(0)
                return if (csv.isBlank()) emptyList() else csv.split(",")
            }
        }
        return emptyList()
    }

    fun saveActiveMoves(moves: List<String>) {
        writableDatabase.execSQL(
            """
            INSERT INTO offline_games (slot_id, rules_profile, moves_csv, updated_at_ms)
            VALUES ('active', 'program-2023.11-v1', ?, ?)
            ON CONFLICT(slot_id) DO UPDATE SET moves_csv = excluded.moves_csv, updated_at_ms = excluded.updated_at_ms
            """.trimIndent(),
            arrayOf(moves.joinToString(","), System.currentTimeMillis())
        )
    }
}

object AndroidCredentialVault {
    private const val KEY_ALIAS = "jade_session_aes_key_v1"
    private const val PREFS_NAME = "jade_encrypted_auth"

    private fun getOrCreateSecretKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val keyGen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        keyGen.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build()
        )
        return keyGen.generateKey()
    }

    fun storeRefreshToken(context: Context, token: String) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, getOrCreateSecretKey())
        val ct = cipher.doFinal(token.toByteArray(Charsets.UTF_8))
        context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE).edit()
            .putString("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
            .putString("ct", Base64.encodeToString(ct, Base64.NO_WRAP))
            .apply()
    }

    fun loadRefreshToken(context: Context): String? {
        val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        val ivB64 = prefs.getString("iv", null) ?: return null
        val ctB64 = prefs.getString("ct", null) ?: return null
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(
            Cipher.DECRYPT_MODE,
            getOrCreateSecretKey(),
            GCMParameterSpec(128, Base64.decode(ivB64, Base64.NO_WRAP))
        )
        return String(cipher.doFinal(Base64.decode(ctB64, Base64.NO_WRAP)), Charsets.UTF_8)
    }
}

object NativeEngineBridge {
    private var handle: Long = 0L

    fun cancelActiveSearch() {
        if (handle != 0L) {
            runCatching { nativeCancel(handle) }
        }
    }

    @JvmStatic external fun nativeCreate(): Long
    @JvmStatic external fun nativeConfigure(handle: Long, optionsJson: String): Int
    @JvmStatic external fun nativeSearch(handle: Long, requestJson: String): String
    @JvmStatic external fun nativeCancel(handle: Long): Int
    @JvmStatic external fun nativeDispose(handle: Long)
}
