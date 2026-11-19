package play.jade.app

import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.media.AudioAttributes
import android.media.SoundPool
import android.os.Bundle
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.activity.ComponentActivity
import androidx.activity.BackEventCompat
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlin.math.floor
import kotlin.math.min

/**
 * Native Android client using Kotlin + Jetpack Compose, Compose Canvas/DrawScope with an
 * explicit TalkBack accessibility semantics tree, SoundPool short-sample playback,
 * Android Keystore AES-GCM credential encryption, and SQLite offline play persistence.
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
        // Section 12 & 15: pause audio and stop background engine searches on lifecycle suspension.
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
    var selectedSquare by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    var moveHistory by remember { mutableStateOf(offlineDb.loadActiveMoves()) }

    val bgColor = if (darkMode) Color(0xFF1D2421) else Color(0xFFF3EEE4)
    val inkColor = if (darkMode) Color(0xFFF3EEE4) else Color(0xFF252A25)

    // Native Android Back deselects active piece before popping navigation.
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
            if (isTabletOrFoldable) {
                Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
                    JadeComposeBoard(
                        modifier = Modifier.weight(1f).fillMaxHeight(),
                        redBelow = redBelow,
                        selected = selectedSquare,
                        onSquareIntent = { sq ->
                            handleAndroidSquareIntent(sq, selectedSquare, moveHistory, offlineDb, audioPool) { nextSel, nextMoves ->
                                selectedSquare = nextSel
                                moveHistory = nextMoves
                            }
                        }
                    )
                    JadeControlsColumn(
                        modifier = Modifier.width(300.dp),
                        inkColor = inkColor,
                        moveCount = moveHistory.size,
                        onFlip = { redBelow = !redBelow },
                        onResetOffline = {
                            selectedSquare = null
                            moveHistory = emptyList()
                            offlineDb.saveActiveMoves(emptyList())
                        },
                        onToggleDark = { darkMode = !darkMode }
                    )
                }
            } else {
                // Phone layout: board stays full-width primary object; never shrunk for a desktop sidebar.
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    JadeComposeBoard(
                        modifier = Modifier.fillMaxWidth().weight(1f),
                        redBelow = redBelow,
                        selected = selectedSquare,
                        onSquareIntent = { sq ->
                            handleAndroidSquareIntent(sq, selectedSquare, moveHistory, offlineDb, audioPool) { nextSel, nextMoves ->
                                selectedSquare = nextSel
                                moveHistory = nextMoves
                            }
                        }
                    )
                    JadeControlsColumn(
                        modifier = Modifier.fillMaxWidth(),
                        inkColor = inkColor,
                        moveCount = moveHistory.size,
                        onFlip = { redBelow = !redBelow },
                        onResetOffline = {
                            selectedSquare = null
                            moveHistory = emptyList()
                            offlineDb.saveActiveMoves(emptyList())
                        },
                        onToggleDark = { darkMode = !darkMode }
                    )
                }
            }
        }
    }
}

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
    if (currentSelected == null) {
        update(sq, moves)
    } else if (currentSelected == sq) {
        update(null, moves)
    } else {
        val uci = "${'a' + currentSelected.first}${currentSelected.second}${'a' + sq.first}${sq.second}"
        val updated = moves + uci
        db.saveActiveMoves(updated)
        audio.playPlaceOnce()
        update(null, updated)
    }
}

@Composable
fun JadeComposeBoard(
    modifier: Modifier,
    redBelow: Boolean,
    selected: Pair<Int, Int>?,
    onSquareIntent: (Pair<Int, Int>?) -> Unit
) {
    val paleJade = Color(0xFFDCE3CE)
    val deepJade = Color(0xFF415447)
    val charcoal = Color(0xFF252A25)
    val vermilion = Color(0xFFA54837)

    // Explicit TalkBack accessibility semantics tree separate from Canvas drawing (Section 6 & 7).
    val semanticDescription = buildString {
        append("Jade Xiangqi Board, ")
        append(if (redBelow) "Red pieces at bottom" else "Black pieces at bottom")
        selected?.let { append(", selected square ${'a' + it.first}${it.second}") }
    }

    Canvas(
        modifier = modifier
            .semantics {
                contentDescription = semanticDescription
                role = Role.Image
                customActions = listOf(
                    CustomAccessibilityAction("Select center cannon e2") {
                        onSquareIntent(4 to 2)
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
        val w = size.width
        val h = size.height
        val p = 18f
        val s = min((w - 2f * p) / 9f, (h - 2f * p) / 10f)
        if (s <= 0f) return@Canvas
        val ox = (w - 8f * s) / 2f
        val oy = (h - 9f * s) / 2f

        for (r in 0..9) {
            drawLine(charcoal, Offset(ox, oy + r * s), Offset(ox + 8f * s, oy + r * s), strokeWidth = 2f)
        }
        for (f in 0..8) {
            if (f == 0 || f == 8) {
                drawLine(charcoal, Offset(ox + f * s, oy), Offset(ox + f * s, oy + 9f * s), strokeWidth = 2f)
            } else {
                drawLine(charcoal, Offset(ox + f * s, oy), Offset(ox + f * s, oy + 4f * s), strokeWidth = 1.6f)
                drawLine(charcoal, Offset(ox + f * s, oy + 5f * s), Offset(ox + f * s, oy + 9f * s), strokeWidth = 1.6f)
            }
        }
        // Palace diagonals
        drawLine(charcoal, Offset(ox + 3f * s, oy), Offset(ox + 5f * s, oy + 2f * s), strokeWidth = 1.6f)
        drawLine(charcoal, Offset(ox + 5f * s, oy), Offset(ox + 3f * s, oy + 2f * s), strokeWidth = 1.6f)
        drawLine(charcoal, Offset(ox + 3f * s, oy + 7f * s), Offset(ox + 5f * s, oy + 9f * s), strokeWidth = 1.6f)
        drawLine(charcoal, Offset(ox + 5f * s, oy + 7f * s), Offset(ox + 3f * s, oy + 9f * s), strokeWidth = 1.6f)

        selected?.let { (x, y) ->
            val sx = if (redBelow) ox + s * x else ox + s * (8 - x)
            val sy = if (redBelow) oy + s * (9 - y) else oy + s * y
            drawCircle(
                color = vermilion,
                radius = s * 0.46f,
                center = Offset(sx, sy),
                style = Stroke(width = 3.5f)
            )
            drawCircle(color = paleJade, radius = s * 0.41f, center = Offset(sx, sy))
            drawCircle(color = deepJade, radius = s * 0.41f, center = Offset(sx, sy), style = Stroke(width = 2f))
        }
    }
}

@Composable
private fun JadeControlsColumn(
    modifier: Modifier,
    inkColor: Color,
    moveCount: Int,
    onFlip: () -> Unit,
    onResetOffline: () -> Unit,
    onToggleDark: () -> Unit
) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Jade · 玉枰象棋", color = inkColor, style = MaterialTheme.typography.titleMedium)
        Text("Offline game · $moveCount plies · program-2023.11-v1", color = inkColor, style = MaterialTheme.typography.bodySmall)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onFlip) { Text("Flip") }
            OutlinedButton(onClick = onResetOffline) { Text("New Offline") }
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

/**
 * Android Keystore AES-GCM key protector for revocable native session refresh credentials.
 * Android Keystore holds the non-exportable AES key; encrypted ciphertext lives in dedicated private prefs,
 * never in the ordinary study/game SQLite database (Section 8 & 15).
 */
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
