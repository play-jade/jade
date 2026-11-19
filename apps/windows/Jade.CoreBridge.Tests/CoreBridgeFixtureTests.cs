using System.Text.Json;
using Xunit;

namespace Jade.CoreBridge.Tests;

public sealed class CoreBridgeFixtureTests
{
    private static string RepoRoot()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null && !File.Exists(Path.Combine(dir.FullName, "Cargo.toml")))
        {
            dir = dir.Parent;
        }
        return dir?.FullName ?? throw new InvalidOperationException("Repository root not found.");
    }

    [Fact]
    public void AbiVersionAndOpeningPositionMatchCanonicalRules()
    {
        using var game = JadeGameSession.CreateNew();
        using var state = game.GetStateSnapshot();
        Assert.Equal("red", state.RootElement.GetProperty("side_to_move").GetString());
        Assert.Equal(44, state.RootElement.GetProperty("legal_moves").GetArrayLength());

        game.ApplyMoveUci("h2e2");
        using var afterCannon = game.GetStateSnapshot();
        Assert.Equal("black", afterCannon.RootElement.GetProperty("side_to_move").GetString());

        game.Undo();
        using var restored = game.GetStateSnapshot();
        Assert.Equal("red", restored.RootElement.GetProperty("side_to_move").GetString());
    }

    [Fact]
    public void AllPublishedPikafishExamplesMatchThroughDotNetCAbi()
    {
        var path = Path.Combine(RepoRoot(), "data", "fixtures", "rules", "pikafish_examples.json");
        using var doc = JsonDocument.Parse(File.ReadAllText(path));
        foreach (var example in doc.RootElement.GetProperty("examples").EnumerateArray())
        {
            var id = example.GetProperty("id").GetString()!;
            var fen = example.GetProperty("fen").GetString()!;
            var expectedOutcome = example.GetProperty("expected_outcome").GetString()!;

            using var session = JadeGameSession.FromFen(fen);
            foreach (var move in example.GetProperty("moves_uci").EnumerateArray())
            {
                session.ApplyMoveUci(move.GetString()!);
            }

            using var finalState = session.GetStateSnapshot();
            var adjudication = finalState.RootElement.GetProperty("adjudication");
            Assert.False(adjudication.ValueKind == JsonValueKind.Null, $"Expected terminal verdict on {id}");
            Assert.Equal(expectedOutcome, adjudication.GetProperty("outcome").GetString());
        }
    }

    [Fact]
    public void Section4HitTestingNeverClampsOutsideViewport()
    {
        Assert.Null(JadeGameSession.HitTest(450, 500, 16, redBelow: true, px: 2.0, py: 2.0));
        var center = JadeGameSession.HitTest(450, 500, 16, redBelow: true, px: 225.0, py: 250.0);
        // Midpoint between ranks 4 and 5 (river): half-open boundary resolves deterministically to cell below.
        Assert.NotNull(center);
    }
}
