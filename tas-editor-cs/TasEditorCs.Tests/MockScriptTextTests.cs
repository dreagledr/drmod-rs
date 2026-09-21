namespace TasEditorCs.Tests;

/// The text a stub script opens with — the mock the text region reads until the on-disk
/// workspace is wired up.
public class MockScriptTextTests
{
    static readonly ScriptEntry BladeRun = new("s1", "blade-run", 42);
    static readonly ScriptEntry BarrierFlight = new("s2", "barrier-flight", 198);
    static readonly ScriptEntry LightningStrike = new("s3", "lightning-strike", 20_000);

    [Fact]
    public void Every_stub_script_opens_with_a_text_the_converter_accepts()
    {
        foreach (var script in new[] { BladeRun, BarrierFlight, LightningStrike })
        {
            Assert.Null(ScriptTextStatus.Of(MockScriptText.For(script)).Error);
        }
    }

    [Fact]
    public void The_hand_written_texts_carry_their_own_rules_line()
    {
        Assert.StartsWith("! name=blade-run trig=ticks:0\n", MockScriptText.For(BladeRun));
        Assert.StartsWith(
            "! name=barrier-flight trig=pos:0.5,-1,57 restart\n",
            MockScriptText.For(BarrierFlight));
    }

    [Fact]
    public void The_hand_written_texts_stay_inside_the_frame_count_the_list_shows()
    {
        // The list's `Frames` and the text are two mock sources, so they can disagree — the two
        // short ones are written to agree: the discrepancy the long one carries is documented.
        Assert.Equal(42u, LastFrame(BladeRun));
        Assert.Equal(198u, LastFrame(BarrierFlight));
    }

    [Fact]
    public void The_long_script_is_generated_and_says_where_it_was_clipped()
    {
        var text = MockScriptText.For(LightningStrike);
        var document = ScriptTextStatus.Of(text).Document!;

        Assert.Equal("lightning-strike", document.Name);
        Assert.True(document.Commands.Count > 50, $"only {document.Commands.Count} commands");
        // Measured on this stub (s3): 15 191 characters, 819 lines, 816 commands, last frame
        // 3600 — the long runs the table holds collapse into one command each, so the text is
        // hundreds of lines rather than the twenty thousand the grid has.
        Assert.InRange(ScriptTextStatus.LastFrame(document), 1u, ScriptJson.MaxFrames);
        // The mod refuses a command past 3600, so the frames the table still has are dropped —
        // and the text says so rather than ending for no visible reason.
        Assert.Contains("\n# mock:", text);
    }

    [Fact]
    public void A_short_script_that_is_not_written_out_by_hand_carries_no_clip_note()
    {
        var script = new ScriptEntry("s9", "made-up", 120);
        var text = MockScriptText.For(script);

        Assert.Equal("made-up", ScriptTextStatus.Of(text).Document!.Name);
        Assert.DoesNotContain("# mock:", text);
    }

    [Fact]
    public void Asks_for_the_same_text_every_time()
    {
        // Two entries with the same id: the id is the script's identity (the frames are seeded
        // from it), so the second ask is the cached text rather than a second generation.
        Assert.Equal(
            MockScriptText.For(LightningStrike),
            MockScriptText.For(new ScriptEntry("s3", "lightning-strike", 20_000)));
    }

    static uint LastFrame(ScriptEntry script) =>
        ScriptTextStatus.LastFrame(ScriptTextStatus.Of(MockScriptText.For(script)).Document!);
}
