using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.Win32.SafeHandles;

namespace Jade.CoreBridge;

/// <summary>
/// SafeHandle wrapper over the opaque Rust <c>JadeGameHandle*</c> exported by <c>core_ffi</c>.
/// Rust panics are caught with <c>catch_unwind</c> on the native side and never cross this ABI boundary.
/// </summary>
public sealed partial class SafeGameHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public SafeGameHandle() : base(ownsHandle: true)
    {
    }

    internal SafeGameHandle(nint existingHandle) : base(ownsHandle: true)
    {
        SetHandle(existingHandle);
    }

    protected override bool ReleaseHandle()
    {
        NativeMethods.jade_game_free(handle);
        return true;
    }
}

public static partial class NativeMethods
{
    public const string LibraryName = "core_ffi";
    public const uint ExpectedAbiVersion = 1;

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial uint jade_core_abi_version();

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void jade_string_free(nint utf8Ptr);

    [LibraryImport(LibraryName, StringMarshalling = StringMarshalling.Utf8)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_game_new(string rulesProfile, out nint outHandle);

    [LibraryImport(LibraryName, StringMarshalling = StringMarshalling.Utf8)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_game_from_fen(string fen, string rulesProfile, out nint outHandle);

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void jade_game_free(nint handle);

    [LibraryImport(LibraryName, StringMarshalling = StringMarshalling.Utf8)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_game_apply_uci(SafeGameHandle handle, string moveUci);

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_game_undo(SafeGameHandle handle);

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_game_state_json(SafeGameHandle handle, out nint outJsonUtf8);

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_board_view_json(
        SafeGameHandle handle,
        double width,
        double height,
        double padding,
        int redBelow,
        out nint outJsonUtf8);

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_hit_test(
        double width,
        double height,
        double padding,
        int redBelow,
        double px,
        double py,
        out int outFile,
        out int outRank);

    [LibraryImport(LibraryName)]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial int jade_last_error_message(out nint outUtf8);
}

public sealed class JadeCoreException(int code, string message)
    : Exception($"Jade core FFI error ({code}): {message}")
{
    public int ErrorCode { get; } = code;
}

public sealed class JadeGameSession : IDisposable
{
    private readonly SafeGameHandle _handle;

    private JadeGameSession(SafeGameHandle handle)
    {
        _handle = handle;
    }

    public static void VerifyAbiVersion()
    {
        var actual = NativeMethods.jade_core_abi_version();
        if (actual != NativeMethods.ExpectedAbiVersion)
        {
            throw new JadeCoreException(
                -99,
                $"ABI mismatch: expected {NativeMethods.ExpectedAbiVersion}, got {actual}");
        }
    }

    public static JadeGameSession CreateNew(string rulesProfile = "program-2023.11-v1")
    {
        VerifyAbiVersion();
        var rc = NativeMethods.jade_game_new(rulesProfile, out var raw);
        CheckStatus(rc);
        return new JadeGameSession(new SafeGameHandle(raw));
    }

    public static JadeGameSession FromFen(string fen, string rulesProfile = "program-2023.11-v1")
    {
        VerifyAbiVersion();
        var rc = NativeMethods.jade_game_from_fen(fen, rulesProfile, out var raw);
        CheckStatus(rc);
        return new JadeGameSession(new SafeGameHandle(raw));
    }

    public void ApplyMoveUci(string moveUci)
    {
        CheckStatus(NativeMethods.jade_game_apply_uci(_handle, moveUci));
    }

    public void Undo()
    {
        CheckStatus(NativeMethods.jade_game_undo(_handle));
    }

    public JsonDocument GetStateSnapshot()
    {
        CheckStatus(NativeMethods.jade_game_state_json(_handle, out var ptr));
        var json = ConsumeRustUtf8(ptr);
        return JsonDocument.Parse(json);
    }

    public JsonDocument GetBoardView(double width, double height, double padding, bool redBelow)
    {
        CheckStatus(NativeMethods.jade_board_view_json(
            _handle,
            width,
            height,
            padding,
            redBelow ? 1 : 0,
            out var ptr));
        var json = ConsumeRustUtf8(ptr);
        return JsonDocument.Parse(json);
    }

    public static (int File, int Rank)? HitTest(
        double width,
        double height,
        double padding,
        bool redBelow,
        double px,
        double py)
    {
        var rc = NativeMethods.jade_hit_test(
            width,
            height,
            padding,
            redBelow ? 1 : 0,
            px,
            py,
            out var file,
            out var rank);
        if (rc == 0)
        {
            return null;
        }
        CheckStatus(rc == 1 ? 0 : rc);
        return (file, rank);
    }

    private static void CheckStatus(int rc)
    {
        if (rc == 0)
        {
            return;
        }
        var errMsg = "Unknown FFI error";
        if (NativeMethods.jade_last_error_message(out var ptr) == 0 && ptr != 0)
        {
            errMsg = ConsumeRustUtf8(ptr);
        }
        throw new JadeCoreException(rc, errMsg);
    }

    private static string ConsumeRustUtf8(nint ptr)
    {
        if (ptr == 0)
        {
            return string.Empty;
        }
        try
        {
            return Marshal.PtrToStringUTF8(ptr) ?? string.Empty;
        }
        finally
        {
            NativeMethods.jade_string_free(ptr);
        }
    }

    public void Dispose()
    {
        _handle.Dispose();
    }
}
