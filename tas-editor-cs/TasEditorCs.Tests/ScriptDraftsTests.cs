namespace TasEditorCs.Tests;

/// The drafts the text region keeps per script.
public class ScriptDraftsTests
{
    static readonly ScriptEntry BladeRun = new("s1", "blade-run", 42);
    static readonly ScriptEntry BarrierFlight = new("s2", "barrier-flight", 198);

    [Fact]
    public void Falls_back_to_the_mock_text_until_something_is_typed() =>
        Assert.Equal(MockScriptText.For(BladeRun), ScriptDrafts.Resolve(ScriptDrafts.Empty, BladeRun));

    [Fact]
    public void Prefers_what_was_typed_for_that_script()
    {
        var drafts = ScriptDrafts.With(ScriptDrafts.Empty, BladeRun.Id, "0 a\n");

        Assert.Equal("0 a\n", ScriptDrafts.Resolve(drafts, BladeRun));
    }

    [Fact]
    public void Leaves_the_scripts_nobody_typed_in_on_their_mock()
    {
        var drafts = ScriptDrafts.With(ScriptDrafts.Empty, BladeRun.Id, "0 a\n");

        Assert.Equal(MockScriptText.For(BarrierFlight), ScriptDrafts.Resolve(drafts, BarrierFlight));
    }

    [Fact]
    public void Replacing_a_draft_leaves_the_state_the_render_is_holding_alone()
    {
        // Reactor re-renders on a state value it does not see as a different instance, so the
        // dictionary handed in must not be the one written into.
        var before = ScriptDrafts.With(ScriptDrafts.Empty, BarrierFlight.Id, "1 b\n");
        var after = ScriptDrafts.With(before, BladeRun.Id, "0 a\n");

        Assert.Single(before);
        Assert.DoesNotContain(BladeRun.Id, before.Keys);
        Assert.Equal(2, after.Count);
        Assert.Equal("1 b\n", after[BarrierFlight.Id]);
    }
}
