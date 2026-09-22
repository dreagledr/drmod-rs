using Microsoft.UI.Reactor.Core;

namespace TasEditorCs.Tests;

/// The controls region: the run, the rules it is configured by, and what the game is doing. The
/// region is a value in and a tree out, so everything here is asserted on the tree — no WinUI control
/// is created (`ComboBox` and `NumberBox` would throw in the headless host).
public class ScriptControlsTests
{
    const string Text = "! name=r03-barrier-ticks trig=ticks:0\n0 a\n";

    [Fact]
    public void Run_is_live_only_with_the_game_answering_and_the_script_slot_free()
    {
        Assert.True(Live(Run(View())));
        Assert.False(Live(Cancel(View())));

        // The mod holds one script slot: a second run would be answered `409`, so the button says so
        // instead. Cancel is live exactly while that slot is held — whoever holds it.
        Assert.False(Live(Run(View(game: Online(Running)))));
        Assert.True(Live(Cancel(View(game: Online(Running)))));
        Assert.True(Live(Cancel(View(game: Online(Armed)))));

        // Nothing answering: the status line says why, and the button does not pretend otherwise.
        Assert.False(Live(Run(View(game: GameStatus.Offline))));
        Assert.False(Live(Cancel(View(game: GameStatus.Offline))));

        // A run being prepared: one click at a time.
        Assert.False(Live(Run(View(preparing: true))));
    }

    [Fact]
    public void The_status_line_reads_the_game_not_the_rules()
    {
        Assert.Equal(
            "In Game · R-03 · 58.4 fps · r03-barrier-ticks 412/1300 (running) · tick fixed 1/60"
            + " · cap 60 fps · rng freeze 1",
            Status(View(game: Online(Running))));

        // Headless is worth a word of its own: the picture is gone while it is on.
        Assert.Equal(
            "In Game · R-03 · 58.4 fps · r03-barrier-ticks 412/1300 (running) · tick fixed 1/60"
            + " · cap 60 fps · rng freeze 1 · headless",
            Status(View(game: Online(Running, headless: true))));

        // The levers are the game's words about itself: an unlimited cap, the tick as the engine
        // measures it, an unpinned RNG. No script: the mod has not run one since it was injected.
        Assert.Equal(
            "In Game · R-03 · 58.4 fps · tick as in the game · cap unlimited · rng off",
            Status(View(game: Online(fixedTick: false, cap: null, rngPin: "off"))));
    }

    [Fact]
    public void The_status_line_says_offline_when_the_mod_does_not_answer()
    {
        Assert.Equal(ScriptControls.Offline, Status(View(game: GameStatus.Offline)));
    }

    [Fact]
    public void The_note_says_what_a_run_of_this_text_would_be()
    {
        Assert.Equal(
            "“r03-barrier-ticks” starts after 0 ticks of gameplay",
            Note(View()));

        // The restart is part of what the run is: the mod plays the pause menu before it arms.
        Assert.Equal(
            "“probe” starts after 5 ticks of gameplay · restarts the mission first",
            Note(View("! name=probe trig=ticks:5 restart\n5 x\n")));

        Assert.Equal(
            "“probe” starts at 1, 2, 3",
            Note(View("! name=probe trig=pos:1,2,3\n0 a\n")));

        // No rules line at all: the mod's own default name, and no trigger to wait for.
        Assert.Equal("“script” starts at once", Note(View("0 a\n")));
    }

    [Fact]
    public void A_refused_text_says_so_rather_than_pretending_there_is_a_run()
    {
        // The note comes from the text on screen, not from the file behind it — a script whose edits
        // were never saved is the one the author is looking at.
        Assert.Equal(
            "Not a script the mod would run — the text region below says why",
            Note(View("0 zz\n")));
    }

    [Fact]
    public void A_failure_takes_the_note_over_with_its_own_wording()
    {
        // The mod names the field it refused, and that wording is what reaches the user.
        Assert.Equal(
            "name too long (max 64)",
            Note(View(error: "name too long (max 64)")));
    }

    [Fact]
    public void The_rules_row_shows_the_rules_it_was_given()
    {
        var view = View(
            rules: PlaybackRules.Default with
            {
                FixedTick = false,
                Cap = FpsCapMode.Custom,
                CustomFps = 144,
                PinSeed = false,
                Headless = true,
            },
            seedText: "0x55555555");

        Assert.Equal("Fixed tick 1/60", Tick(view).Label);
        Assert.False(Tick(view).IsChecked.Value);

        Assert.Equal(["default", "unlimited", "custom"], Cap(view).Items);
        Assert.Equal((int)FpsCapMode.Custom, Cap(view).SelectedIndex.Value);
        Assert.Equal("Frame cap", Cap(view).Header);

        Assert.Equal(144d, Fps(view).Value.Value);
        Assert.Equal(1d, Fps(view).Minimum);
        Assert.Equal(1000d, Fps(view).Maximum);
        // The limit is the editor's only while the mode says so — a live box next to "default" would
        // read as if it were in force.
        Assert.True(Live(Fps(view)));
        Assert.False(Live(Fps(View())));

        Assert.False(PinSeed(view).IsChecked.Value);
        Assert.Equal("0x55555555", SeedText(view).Value.Value);

        Assert.True(Headless(view).IsChecked.Value);
    }

    [Fact]
    public void Editing_a_rule_reports_the_whole_set_with_the_one_field_changed()
    {
        var reported = new List<PlaybackRules>();
        var view = View(rules: PlaybackRules.Default, report: reported.Add);

        Tick(view).OnIsCheckedChanged!(false);
        Cap(view).OnSelectedIndexChanged!((int)FpsCapMode.Unlimited);
        Fps(view).OnValueChanged!(144d);
        PinSeed(view).OnIsCheckedChanged!(false);
        Headless(view).OnIsCheckedChanged!(true);

        // A record per edit, each one the set it was given with one field moved — so a handler never
        // has to reconstruct the rules it did not touch.
        Assert.Equal(
            [
                PlaybackRules.Default with { FixedTick = false },
                PlaybackRules.Default with { Cap = FpsCapMode.Unlimited },
                PlaybackRules.Default with { CustomFps = 144 },
                PlaybackRules.Default with { PinSeed = false },
                PlaybackRules.Default with { Headless = true },
            ],
            reported);
    }

    [Fact]
    public void The_seed_is_text_so_hex_survives_the_field()
    {
        var typed = new List<string>();
        var view = View(seedText: "1", seedTyped: typed.Add);

        SeedText(view).OnChanged!("0x55555555");

        Assert.Equal(["0x55555555"], typed);
        Assert.Equal("Seed (0x for hex)", SeedText(view).Header);
    }

    [Fact]
    public void Run_and_cancel_hand_their_clicks_to_the_shell()
    {
        var ran = 0;
        var cancelled = 0;
        var view = View(run: () => ran++, cancel: () => cancelled++);

        Run(view).OnClick!();
        Cancel(view).OnClick!();

        Assert.Equal(1, ran);
        Assert.Equal(1, cancelled);
    }

    static readonly GameScript Running = new(7, "r03-barrier-ticks", GameScriptPhase.Running, 412, 1300);

    static readonly GameScript Armed = new(7, "r03-barrier-ticks", GameScriptPhase.Armed, 0, 1300);

    static GameStatus Online(
        GameScript? script = null,
        bool fixedTick = true,
        uint? cap = 60,
        string rngPin = "freeze",
        uint rngSeed = 1,
        bool headless = false) =>
        new(
            Online: true,
            MenuStatus: "In Game",
            MissionId: 272,
            MissionName: "R-03",
            Script: script,
            Fps: 58.4f,
            FixedTick: fixedTick,
            CapLimit: cap,
            RngPin: rngPin,
            RngSeed: rngSeed,
            Headless: headless);

    static ScriptControlsView View(
        string text = Text,
        PlaybackRules? rules = null,
        string seedText = "1",
        GameStatus? game = null,
        bool preparing = false,
        string? error = null,
        Action<PlaybackRules>? report = null,
        Action<string>? seedTyped = null,
        Action? run = null,
        Action? cancel = null)
    {
        var dirty = false;
        return new ScriptControlsView(
            ScriptTextStatus.Of(ScriptDsl.Lines(text)),
            dirty,
            StandardCommand.Save(() => { }, dirty),
            rules ?? PlaybackRules.Default,
            seedText,
            game ?? Online(),
            preparing,
            error,
            seedTyped ?? (_ => { }),
            report ?? (_ => { }),
            run ?? (() => { }),
            cancel ?? (() => { }));
    }

    /// Whether a control is live. `.IsEnabled(...)` lands in the element's modifiers rather than in its
    /// own record property — that is the entry the reconciler applies to the control — so the modifier
    /// is where the region's own choice shows up.
    static bool Live(Element element)
    {
        var applied = element.Modifiers?.IsEnabled;

        Assert.NotNull(applied);
        return applied.Value;
    }

    static FlexElement Region(ScriptControlsView view) =>
        Assert.IsType<FlexElement>(ScriptControls.View(view));

    static StackElement Buttons(ScriptControlsView view) =>
        Assert.IsType<StackElement>(Region(view).Children[0]);

    static StackElement Rules(ScriptControlsView view) =>
        Assert.IsType<StackElement>(Region(view).Children[1]);

    static CheckBoxElement Tick(ScriptControlsView view) =>
        Assert.IsType<CheckBoxElement>(Rules(view).Children[0]);

    static ComboBoxElement Cap(ScriptControlsView view) =>
        Assert.IsType<ComboBoxElement>(Rules(view).Children[1]);

    static NumberBoxElement Fps(ScriptControlsView view) =>
        Assert.IsType<NumberBoxElement>(Rules(view).Children[2]);

    static CheckBoxElement PinSeed(ScriptControlsView view) =>
        Assert.IsType<CheckBoxElement>(Rules(view).Children[3]);

    static TextBoxElement SeedText(ScriptControlsView view) =>
        Assert.IsType<TextBoxElement>(Rules(view).Children[4]);

    static CheckBoxElement Headless(ScriptControlsView view) =>
        Assert.IsType<CheckBoxElement>(Rules(view).Children[5]);

    static ButtonElement Run(ScriptControlsView view) =>
        Assert.IsType<ButtonElement>(Buttons(view).Children[1]);

    static ButtonElement Cancel(ScriptControlsView view) =>
        Assert.IsType<ButtonElement>(Buttons(view).Children[2]);

    static string? Status(ScriptControlsView view) => Content(Region(view).Children[2]);

    static string? Note(ScriptControlsView view) => Content(Region(view).Children[3]);

    static string? Content(Element element) => Assert.IsType<TextBlockElement>(element).Content;
}
