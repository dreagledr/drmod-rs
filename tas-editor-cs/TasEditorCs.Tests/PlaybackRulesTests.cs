namespace TasEditorCs.Tests;

/// The run rules: what a run sets, spelled the way the mod's endpoints read it.
///
/// The bodies are asserted byte for byte on purpose — they are a contract with a program in another
/// repository (`src/api.rs`), and a field spelled `frames` instead of `fps` is a `400` nobody would
/// see until the game refused a run.
public class PlaybackRulesTests
{
    static PlaybackRules Rules => PlaybackRules.Default;

    [Fact]
    public void The_tick_is_pinned_to_one_sixtieth_or_left_to_the_game()
    {
        Assert.Equal("""{"fixed":true,"ticks":true}""", Rules.DtBody());
        // Off: the mod restores its own measured delta, and the synthetic clocks are not asked for.
        Assert.Equal("""{"fixed":false}""", (Rules with { FixedTick = false }).DtBody());
    }

    [Fact]
    public void The_frame_cap_is_the_games_default_unlimited_or_the_editors_own()
    {
        Assert.Equal("""{"cap":"game"}""", Rules.CapBody());
        Assert.Equal("""{"cap":"off"}""", (Rules with { Cap = FpsCapMode.Unlimited }).CapBody());
        Assert.Equal(
            """{"fps":144}""",
            (Rules with { Cap = FpsCapMode.Custom, CustomFps = 144 }).CapBody());
    }

    [Fact]
    public void A_custom_cap_is_clamped_to_what_the_mod_accepts()
    {
        // The mod answers `400` outside `1..1000`; a number box allows any number, and a refusal over
        // a typed zero would be a run that never starts for a reason nobody can see.
        Assert.Equal("""{"fps":1}""", (Rules with { Cap = FpsCapMode.Custom, CustomFps = 0 }).CapBody());
        Assert.Equal("""{"fps":1000}""", (Rules with { Cap = FpsCapMode.Custom, CustomFps = 5000 }).CapBody());
    }

    [Fact]
    public void The_seed_is_frozen_with_its_number_or_handed_back_to_the_game()
    {
        Assert.Equal("""{"pin":"freeze","seed":1}""", Rules.SeedBody());
        Assert.Equal(
            """{"pin":"freeze","seed":1431655765}""",
            (Rules with { Seed = 0x55555555 }).SeedBody());
        // Unpinned: no seed at all — `freeze` is what carries one.
        Assert.Equal("""{"pin":"off"}""", (Rules with { PinSeed = false }).SeedBody());
    }

    [Theory]
    [InlineData("1", true, 1u)]
    [InlineData(" 42 ", true, 42u)]
    [InlineData("0x55555555", true, 1431655765u)]
    [InlineData("0X10", true, 16u)]
    [InlineData("4294967295", true, 4294967295u)]
    [InlineData("", false, 0u)]
    [InlineData("zz", false, 0u)]
    [InlineData("0x", false, 0u)]
    [InlineData("0xZZ", false, 0u)]
    [InlineData("-1", false, 0u)]
    public void A_seed_is_read_as_decimal_or_hex(string typed, bool reads, uint expected)
    {
        // The reproducibility notes spell their seeds in hex (`0x55555555`, `docs/API.md` §3.10), so
        // the field has to speak both — and to say "no" for anything in between, which is how a
        // half-typed seed is told apart from one a run can use.
        Assert.Equal(reads, PlaybackRules.TrySeed(typed, out var seed));
        Assert.Equal(expected, seed);
    }

    [Fact]
    public void The_rules_read_as_one_line()
    {
        Assert.Equal(
            "fixed tick 1/60 · cap default · seed freeze 1 · headless off",
            Rules.Describe());

        Assert.Equal(
            "fixed tick off · cap unlimited · seed off · headless on",
            (Rules with { FixedTick = false, Cap = FpsCapMode.Unlimited, PinSeed = false, Headless = true })
                .Describe());

        Assert.Equal(
            "fixed tick 1/60 · cap 144 fps · seed freeze 1 · headless off",
            (Rules with { Cap = FpsCapMode.Custom, CustomFps = 144 }).Describe());
    }
}
