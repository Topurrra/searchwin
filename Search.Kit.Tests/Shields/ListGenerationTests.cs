using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

/// The race ShieldLists had: Forget (the lists switched off) could run on
/// the UI thread between a worker's last check and its Save, and the saved
/// list came back after Forget had deleted it, marked loaded.
public class ListGenerationTests
{
    [Fact]
    public void A_save_that_lands_after_forget_is_taken_back_out()
    {
        var folder = Directory.CreateTempSubdirectory("lists");
        try
        {
            var blob = Path.Combine(folder.FullName, "lists.bin");
            void Cleanup() => File.Delete(blob);
            var turns = new ListGeneration();

            var turn = turns.Current;          // the worker starts
            Assert.True(turns.Holds(turn));    // its last check before saving
            turns.Advance();                   // Forget, on the UI thread
            Cleanup();
            File.WriteAllText(blob, "list");   // the save lands anyway

            Assert.True(turns.Overtaken(turn, Cleanup));
            Assert.False(File.Exists(blob));
        }
        finally
        {
            folder.Delete(recursive: true);
        }
    }

    [Fact]
    public void A_save_in_the_current_turn_stays()
    {
        var turns = new ListGeneration();
        var undone = false;
        Assert.False(turns.Overtaken(turns.Current, () => undone = true));
        Assert.False(undone);
    }

    [Fact]
    public void Loaded_in_a_turn_forget_ended_is_not_loaded()
    {
        var turns = new ListGeneration();
        Assert.False(turns.Loaded);
        var turn = turns.Current;
        turns.Advance();
        turns.MarkLoaded(turn);
        Assert.False(turns.Loaded);
        turns.MarkLoaded(turns.Current);
        Assert.True(turns.Loaded);
        turns.Advance();
        Assert.False(turns.Loaded);
    }

    // Publish hands the list over on the UI thread, where Forget runs too:
    // checked there, the answer can't go stale before Shield.Use.
    [Fact]
    public void A_list_from_an_ended_turn_is_not_handed_over()
    {
        var turns = new ListGeneration();
        var turn = turns.Current;
        turns.Advance();
        Assert.False(turns.Holds(turn));
    }
}
