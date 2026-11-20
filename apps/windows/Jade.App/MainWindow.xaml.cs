using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using Jade.CoreBridge;
using Microsoft.Data.Sqlite;
using Microsoft.Graphics.Canvas.Text;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Input;
using Windows.Foundation;
using Windows.System;
using Windows.UI;

namespace Jade.App;

public sealed partial class MainWindow : Window
{
    private JadeGameSession _session = JadeGameSession.CreateNew();
    private bool _redBelow = true;
    private (int File, int Rank)? _selectedSquare;
    private (int File, int Rank) _focusedSquare = (4, 0);
    private Process? _uciEngineProcess;

    private static readonly Color PaperColor = Color.FromArgb(255, 0xF3, 0xEE, 0xE4);
    private static readonly Color PaleJadeColor = Color.FromArgb(255, 0xDC, 0xE3, 0xCE);
    private static readonly Color DeepJadeColor = Color.FromArgb(255, 0x41, 0x54, 0x47);
    private static readonly Color CharcoalColor = Color.FromArgb(255, 0x25, 0x2A, 0x25);
    private static readonly Color VermilionColor = Color.FromArgb(255, 0xA5, 0x48, 0x37);

    public MainWindow()
    {
        InitializeComponent();
        InitializeOfflineSqliteStore();
        RefreshSemanticStatus();
    }

    private void OnBoardDraw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        var w = sender.ActualWidth;
        var h = sender.ActualHeight;
        const double p = 18.0;
        var s = Math.Min((w - 2.0 * p) / 9.0, (h - 2.0 * p) / 10.0);
        if (s <= 0.0)
        {
            return;
        }

        var ox = (float)((w - 8.0 * s) / 2.0);
        var oy = (float)((h - 9.0 * s) / 2.0);
        var step = (float)s;

        var ds = args.DrawingSession;
        ds.Clear(PaperColor);

        // Exact 9x10 grid markings, interrupted river between ranks 4 and 5, and palace diagonals.
        for (var r = 0; r < 10; r++)
        {
            ds.DrawLine(ox, oy + r * step, ox + 8 * step, oy + r * step, CharcoalColor, 1.25f);
        }
        for (var f = 0; f < 9; f++)
        {
            if (f == 0 || f == 8)
            {
                ds.DrawLine(ox + f * step, oy, ox + f * step, oy + 9 * step, CharcoalColor, 1.25f);
            }
            else
            {
                ds.DrawLine(ox + f * step, oy, ox + f * step, oy + 4 * step, CharcoalColor, 1.1f);
                ds.DrawLine(ox + f * step, oy + 5 * step, ox + f * step, oy + 9 * step, CharcoalColor, 1.1f);
            }
        }
        // Top and bottom palace diagonals
        ds.DrawLine(ox + 3 * step, oy, ox + 5 * step, oy + 2 * step, CharcoalColor, 1.1f);
        ds.DrawLine(ox + 5 * step, oy, ox + 3 * step, oy + 2 * step, CharcoalColor, 1.1f);
        ds.DrawLine(ox + 3 * step, oy + 7 * step, ox + 5 * step, oy + 9 * step, CharcoalColor, 1.1f);
        ds.DrawLine(ox + 5 * step, oy + 7 * step, ox + 3 * step, oy + 9 * step, CharcoalColor, 1.1f);

        using var boardView = _session.GetBoardView(w, h, p, _redBelow);
        var root = boardView.RootElement;
        var radius = step * 0.43f;

        foreach (var piece in root.GetProperty("pieces").EnumerateArray())
        {
            var file = piece.GetProperty("file").GetInt32();
            var rank = piece.GetProperty("rank").GetInt32();
            var side = piece.GetProperty("side").GetString();
            var label = piece.GetProperty("glyph").GetString() ?? "?";

            var (sx, sy) = CanonicalToScreen(ox, oy, step, file, rank, _redBelow);
            ds.FillCircle(sx + 1.5f, sy + 2.5f, radius, Color.FromArgb(48, 0, 0, 0));
            ds.FillCircle(sx, sy, radius, PaleJadeColor);
            ds.DrawCircle(sx, sy, radius, DeepJadeColor, 1.6f);

            using var fmt = new CanvasTextFormat
            {
                FontSize = radius * 1.05f,
                HorizontalAlignment = CanvasHorizontalAlignment.Center,
                VerticalAlignment = CanvasVerticalAlignment.Center
            };
            var ink = side == "red" ? VermilionColor : CharcoalColor;
            ds.DrawText(label, new Rect(sx - radius, sy - radius, radius * 2, radius * 2), ink, fmt);
        }

        // Non-color-only keyboard/selection ring
        var (fx, fy) = CanonicalToScreen(ox, oy, step, _focusedSquare.File, _focusedSquare.Rank, _redBelow);
        ds.DrawCircle(fx, fy, radius + 4.0f, DeepJadeColor, 2.5f);
    }

    private static (float X, float Y) CanonicalToScreen(
        float ox, float oy, float s, int x, int y, bool redBelow)
    {
        return redBelow
            ? (ox + s * x, oy + s * (9 - y))
            : (ox + s * (8 - x), oy + s * y);
    }

    private void OnBoardPointerPressed(object sender, PointerRoutedEventArgs e)
    {
        var pt = e.GetCurrentPoint(BoardCanvas).Position;
        var hit = JadeGameSession.HitTest(
            BoardCanvas.ActualWidth,
            BoardCanvas.ActualHeight,
            padding: 18.0,
            redBelow: _redBelow,
            px: pt.X,
            py: pt.Y);
        if (hit is null)
        {
            _selectedSquare = null;
            BoardCanvas.Invalidate();
            return;
        }

        _focusedSquare = hit.Value;
        HandleSquareActivation(hit.Value);
    }

    private void OnBoardKeyDown(object sender, KeyRoutedEventArgs e)
    {
        switch (e.Key)
        {
            case VirtualKey.Left:
                _focusedSquare.File = Math.Clamp(_focusedSquare.File + (_redBelow ? -1 : 1), 0, 8);
                break;
            case VirtualKey.Right:
                _focusedSquare.File = Math.Clamp(_focusedSquare.File + (_redBelow ? 1 : -1), 0, 8);
                break;
            case VirtualKey.Up:
                _focusedSquare.Rank = Math.Clamp(_focusedSquare.Rank + (_redBelow ? 1 : -1), 0, 9);
                break;
            case VirtualKey.Down:
                _focusedSquare.Rank = Math.Clamp(_focusedSquare.Rank + (_redBelow ? -1 : 1), 0, 9);
                break;
            case VirtualKey.Enter:
            case VirtualKey.Space:
                HandleSquareActivation(_focusedSquare);
                return;
            default:
                return;
        }
        e.Handled = true;
        RefreshSemanticStatus();
        BoardCanvas.Invalidate();
    }

    private void HandleSquareActivation((int File, int Rank) sq)
    {
        if (_selectedSquare is null)
        {
            _selectedSquare = sq;
        }
        else if (_selectedSquare.Value == sq)
        {
            _selectedSquare = null;
        }
        else
        {
            var uci = $"{(char)('a' + _selectedSquare.Value.File)}{_selectedSquare.Value.Rank}{(char)('a' + sq.File)}{sq.Rank}";
            try
            {
                _session.ApplyMoveUci(uci);
                SaveOfflineState();
            }
            catch (JadeCoreException)
            {
                // Reselect when destination is not a legal move.
                _selectedSquare = sq;
                BoardCanvas.Invalidate();
                return;
            }
            _selectedSquare = null;
        }
        RefreshSemanticStatus();
        BoardCanvas.Invalidate();
    }

    private void RefreshSemanticStatus()
    {
        using var snap = _session.GetStateSnapshot();
        var stm = snap.RootElement.GetProperty("side_to_move").GetString();
        var desc = $"{stm} to move · focus {(char)('a' + _focusedSquare.File)}{_focusedSquare.Rank}";
        StatusText.Text = desc;
        AutomationProperties.SetHelpText(BoardCanvas, desc);
    }

    private void OnNewOfflineGameClick(object sender, RoutedEventArgs e)
    {
        _session.Dispose();
        _session = JadeGameSession.CreateNew();
        _selectedSquare = null;
        SaveOfflineState();
        RefreshSemanticStatus();
        BoardCanvas.Invalidate();
    }

    private void OnFlipBoardClick(object sender, RoutedEventArgs e)
    {
        _redBelow = !_redBelow;
        BoardCanvas.Invalidate();
    }

    private void OnUndoClick(object sender, RoutedEventArgs e)
    {
        try
        {
            _session.Undo();
            SaveOfflineState();
            RefreshSemanticStatus();
            BoardCanvas.Invalidate();
        }
        catch (JadeCoreException)
        {
        }
    }

    private void OnMaterialChanged(object sender, Microsoft.UI.Xaml.Controls.SelectionChangedEventArgs e)
    {
        BoardCanvas?.Invalidate();
    }

    private async void OnToggleEngineClick(object sender, RoutedEventArgs e)
    {
        if (_uciEngineProcess is { HasExited: false })
        {
            await _uciEngineProcess.StandardInput.WriteLineAsync("stop");
            _uciEngineProcess.Kill();
            _uciEngineProcess = null;
            EngineOutputText.Text = "Analysis stopped.";
            return;
        }

        var enginePath = Environment.GetEnvironmentVariable("JADE_UCI_ENGINE_PATH") ?? "jade-uci-engine.exe";
        try
        {
            _uciEngineProcess = Process.Start(new ProcessStartInfo
            {
                FileName = enginePath,
                RedirectStandardInput = true,
                RedirectStandardOutput = true,
                UseShellExecute = false,
                CreateNoWindow = true
            });
            if (_uciEngineProcess is null) return;
            using var snap = _session.GetStateSnapshot();
            var fen = snap.RootElement.GetProperty("fen").GetString();
            await _uciEngineProcess.StandardInput.WriteLineAsync("uci");
            await _uciEngineProcess.StandardInput.WriteLineAsync($"position fen {fen}");
            await _uciEngineProcess.StandardInput.WriteLineAsync("go depth 6");
            while (await _uciEngineProcess.StandardOutput.ReadLineAsync() is { } line)
            {
                EngineOutputText.Text = line;
                if (line.StartsWith("bestmove", StringComparison.Ordinal))
                {
                    break;
                }
            }
        }
        catch (Exception ex)
        {
            EngineOutputText.Text = $"UCI engine unavailable ({ex.Message})";
        }
    }

    private static string OfflineDbPath()
    {
        var dir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "PlayJade",
            "Offline");
        Directory.CreateDirectory(dir);
        return Path.Combine(dir, "jade_offline.sqlite3");
    }

    private static void InitializeOfflineSqliteStore()
    {
        using var conn = new SqliteConnection($"Data Source={OfflineDbPath()}");
        conn.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = """
            CREATE TABLE IF NOT EXISTS local_saved_games (
                slot_id TEXT PRIMARY KEY,
                rules_profile TEXT NOT NULL,
                fen TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );
            """;
        cmd.ExecuteNonQuery();
    }

    private void SaveOfflineState()
    {
        using var snap = _session.GetStateSnapshot();
        var fen = snap.RootElement.GetProperty("fen").GetString() ?? "";
        using var conn = new SqliteConnection($"Data Source={OfflineDbPath()}");
        conn.Open();
        using var cmd = conn.CreateCommand();
        cmd.CommandText = """
            INSERT INTO local_saved_games (slot_id, rules_profile, fen, updated_at_ms)
            VALUES ('active', 'program-2023.11-v1', $fen, $ts)
            ON CONFLICT(slot_id) DO UPDATE SET fen = excluded.fen, updated_at_ms = excluded.updated_at_ms;
            """;
        cmd.Parameters.AddWithValue("$fen", fen);
        cmd.Parameters.AddWithValue("$ts", DateTimeOffset.UtcNow.ToUnixTimeMilliseconds());
        cmd.ExecuteNonQuery();
    }
}

/// <summary>
/// Windows Credential Manager (CredWriteW / CredDeleteW) adapter for revocable native session tokens.
/// Never stores credentials in the SQLite study/offline game database.
/// </summary>
public static partial class WindowsCredentialStore
{
    private const uint CRED_TYPE_GENERIC = 1;
    private const uint CRED_PERSIST_LOCAL_MACHINE = 2;

    [StructLayout(LayoutKind.Sequential)]
    private struct CREDENTIALW
    {
        public uint Flags;
        public uint Type;
        public nint TargetName;
        public nint Comment;
        public uint LastWrittenLowDateTime;
        public uint LastWrittenHighDateTime;
        public uint CredentialBlobSize;
        public nint CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public nint Attributes;
        public nint TargetAlias;
        public nint UserName;
    }

    [LibraryImport("Advapi32.dll", EntryPoint = "CredWriteW", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool CredWriteW(nint userCredential, uint flags);

    public static unsafe bool StoreRefreshToken(string installId, string refreshToken)
    {
        var bytes = Encoding.UTF8.GetBytes(refreshToken);
        var targetPtr = Marshal.StringToCoTaskMemUni($"PlayJade/RefreshToken/{installId}");
        var userPtr = Marshal.StringToCoTaskMemUni(installId);
        var blobPtr = Marshal.AllocCoTaskMem(bytes.Length);
        try
        {
            Marshal.Copy(bytes, 0, blobPtr, bytes.Length);
            var cred = new CREDENTIALW
            {
                Type = CRED_TYPE_GENERIC,
                TargetName = targetPtr,
                CredentialBlobSize = (uint)bytes.Length,
                CredentialBlob = blobPtr,
                Persist = CRED_PERSIST_LOCAL_MACHINE,
                UserName = userPtr
            };
            return CredWriteW((nint)(&cred), 0);
        }
        finally
        {
            Marshal.ZeroFreeCoTaskMemUnicode(targetPtr);
            Marshal.ZeroFreeCoTaskMemUnicode(userPtr);
            Marshal.FreeCoTaskMem(blobPtr);
        }
    }
}
