using System.Globalization;

namespace TasEditorCs.Tests;

/// The script DSL: what the text says, what it refuses, and the fixed point the goldens rely on
/// (`Write(Parse(text))` is the text again).
public class ScriptDslTests
{
    [Fact]
    public void Writes_a_frame_per_line_without_a_rules_line_when_the_script_is_bare()
    {
        var document = Document(
            Command(0, 10, new ScriptInput { Forward = true }),
            Command(1, 1, new ScriptInput { Jump = true }));

        // Movement is the stick, so the `forward` flag reads as a full press up; `a` is a
        // one-frame tap, and a one-frame duration leaves its colon out.
        Assert.Equal("0 ls:0:10\n1 a\n", ScriptDsl.Write(document));
    }

    [Fact]
    public void Writes_the_sticks_before_the_inputs()
    {
        var document = Document(
            Command(0, 1, new ScriptInput { LeftStick = [0f, -1000f], Camera = [300f, 0f] }),
            Command(4, 2, new ScriptInput { Forward = true, NinjaRun = true, LeftStick = [-500f, -866f] }));

        var lines = ScriptDsl.Write(document).Split('\n', StringSplitOptions.RemoveEmptyEntries);

        // A full press is an angle; a partial one or a camera delta is exact axes, and a zero axis
        // is left out because a missing axis already means zero.
        Assert.Equal("0 ls:0 rsx:300", lines[0]);
        Assert.Equal("4 lsx:-500:2 lsy:-866:2 rt:2", lines[1]);
    }

    [Theory]
    // The compass: 0 is forward (up), 90 right, 180 back, 270 left.
    [InlineData(0.0, 0.0, -1000.0)]
    [InlineData(90.0, 1000.0, 0.0)]
    [InlineData(180.0, 0.0, 1000.0)]
    [InlineData(270.0, -1000.0, 0.0)]
    public void A_stick_angle_is_a_full_press_on_the_compass(double angle, double x, double y)
    {
        var command = Assert.Single(ScriptDsl.Parse($"0 ls:{Number(angle)}\n").Commands);

        Assert.Equal([(float)x, (float)y], command.Input.LeftStick!);
    }

    [Fact]
    public void An_exact_stick_token_carries_the_value_the_json_would()
    {
        var command = Assert.Single(ScriptDsl.Parse("0 lsx:300 lsy:-800 rsx:6500\n").Commands);

        // The axis tokens are how a partial stick and a fast camera turn (a mouse delta of a few
        // thousand) survive the text — a full press would be `ls:<angle>`.
        Assert.Equal([300f, -800f], command.Input.LeftStick!);
        Assert.Equal([6500f, 0f], command.Input.Camera!);

        var diagonal = Assert.Single(ScriptDsl.Parse("0 lsx:1000 lsy:-1000\n").Commands);

        Assert.Equal([1000f, -1000f], diagonal.Input.LeftStick!);
    }

    [Fact]
    public void The_movement_flags_have_no_token_of_their_own()
    {
        // `ls:0` reaches the game the same way the `forward` flag does — the stick alone drives the
        // character (`docs/API.md` §10.2) — so the text has no spelling for the flag itself.
        var command = Assert.Single(ScriptDsl.Parse("0 ls:0\n").Commands);

        Assert.False(command.Input.Forward);
        Assert.Equal([0f, -1000f], command.Input.LeftStick!);
    }

    [Fact]
    public void By_is_the_execute_prompt_and_dr_the_other_inventory_direction()
    {
        var execute = Assert.Single(ScriptDsl.Parse("0 by:2\n").Commands);

        Assert.True(execute.Input.HeavyAttack);
        Assert.True(execute.Input.Zandatsu);
        Assert.Equal("0 by:2\n", ScriptDsl.Write(Document(execute)));

        // `dr` is the same single flag as `dl` — the canonical spelling of it is `dl`.
        var inventory = ScriptDsl.Parse("0 dr\n");

        Assert.True(inventory.Commands[0].Input.WeaponSelect);
        Assert.Equal("0 dl\n", ScriptDsl.Write(inventory));
    }

    [Fact]
    public void Writes_by_only_while_the_two_presses_last_the_same()
    {
        // Y+B as one token is the shape the Execute prompt has; held for different stretches they
        // stay two tokens, because one token cannot carry two durations.
        var together = Document(Command(0, 5, new ScriptInput { HeavyAttack = true, Zandatsu = true }));
        Assert.Equal("0 by:5\n", ScriptDsl.Write(together));

        var apart = Document(
            Command(0, 5, new ScriptInput { HeavyAttack = true }),
            Command(0, 3, new ScriptInput { Zandatsu = true }));

        // The written order is the table's column order — heavy attack is a column before zandatsu.
        Assert.Equal("0 y:5 b:3\n", ScriptDsl.Write(apart));
    }

    [Fact]
    public void Writes_the_rules_line_before_the_frames()
    {
        var document = new ScriptDocument
        {
            Name = "r01-beach",
            Trigger = new ScriptTrigger { Pos = [-24.7f, 12.15f, 120.7f], Ticks = 0 },
            Restart = new RestartPolicy { Ups = 2, Downs = 1, Confirms = 3 },
            Commands = [Command(0, 40, new ScriptInput { Forward = true })],
        };

        Assert.Equal(
            "! name=r01-beach trig=pos:-24.7,12.15,120.7 trig=ticks:0 restart:ups=2,downs=1,confirms=3\n"
            + "0 ls:0:40\n",
            ScriptDsl.Write(document));
    }

    [Fact]
    public void Spells_out_a_restart_only_with_the_parameters_that_differ()
    {
        var document = Document(Command(0, 1, new ScriptInput { Jump = true }));

        Assert.Equal(
            "! restart\n0 a\n",
            ScriptDsl.Write(document with { Restart = new RestartPolicy() }));
    }

    [Fact]
    public void Reads_back_what_it_wrote()
    {
        var document = new ScriptDocument
        {
            Name = "round-trip",
            Trigger = new ScriptTrigger { Ticks = 0 },
            Restart = new RestartPolicy { Tail = 20 },
            Commands =
            [
                Command(0, 6, new ScriptInput { Forward = true, LeftStick = [0f, -1000f] }),
                Command(6, 2, new ScriptInput { Forward = true, Jump = true }),
                Command(20, 24, new ScriptInput { LeftStick = [1000f, -1000f], Camera = [0f, 1000f] }),
            ],
        };

        var text = ScriptDsl.Write(document);
        var again = ScriptDsl.Parse(text);

        Assert.Equal(text, ScriptDsl.Write(again));
        Assert.Equal(document.Name, again.Name);
        Assert.Equal(0ul, again.Trigger!.Ticks);
        Assert.Equal(20u, again.Restart!.Tail);
        Assert.Equal(document.Commands.Count, again.Commands.Count);
        Assert.Equal(6u, again.Commands[0].Duration);
        Assert.Equal([1000f, -1000f], again.Commands[2].Input.LeftStick!);
        Assert.Equal([0f, 1000f], again.Commands[2].Input.Camera!);
    }

    [Fact]
    public void Every_token_reads_as_the_input_it_names_and_writes_back_unchanged()
    {
        // `dr` is the one token whose canonical spelling differs: the inventory switch has a single
        // spelling when written.
        foreach (var (token, canonical) in Tokens)
        {
            var text = $"0 {token}:2\n";
            var document = ScriptDsl.Parse(text);

            Assert.Equal(2u, document.Commands[0].Duration);
            Assert.False(document.Commands[0].Input.IsEmpty);
            Assert.Equal($"0 {canonical}:2\n", ScriptDsl.Write(document));
        }
    }

    [Fact]
    public void Reads_tokens_without_regard_to_case()
    {
        var document = ScriptDsl.Parse("0 LS:0 RT:2\n");

        Assert.Equal(2, document.Commands.Count);
        Assert.True(WithDuration(document, 2).Input.NinjaRun);
        Assert.Equal([0f, -1000f], WithDuration(document, 1).Input.LeftStick!);
    }

    [Fact]
    public void Splits_a_line_into_one_command_per_duration()
    {
        var document = ScriptDsl.Parse("0 ls:0:10 x:2 a\n");

        // One command per duration, shortest first: the text says three different hold lengths.
        Assert.Equal(3, document.Commands.Count);
        Assert.All(document.Commands, command => Assert.Equal(0u, command.T));
        Assert.Equal(10u, document.Commands[2].Duration);
        Assert.True(document.Commands[2].Input.Forward is false);
        Assert.Equal([0f, -1000f], document.Commands[2].Input.LeftStick!);
        Assert.True(WithDuration(document, 2).Input.LightAttack);
        Assert.True(WithDuration(document, 1).Input.Jump);
    }

    [Fact]
    public void Ignores_comments_and_blank_lines()
    {
        var document = ScriptDsl.Parse("# the plan\n\n0 ls:0:1  # the run\n\n");

        Assert.Single(document.Commands);
        Assert.Equal(1u, document.Commands[0].Duration);
    }

    [Fact]
    public void Reads_lines_however_their_break_is_spelled()
    {
        // `\n` is the format's own separator, but a `.tas` file gets edited in more places than this
        // editor — Windows writes `\r\n`, and a text box hands its text back with a lone `\r`
        // (measured: not one `\n` in the whole text). The separator is not part of the format: a line
        // break is a line break.
        var canonical = ScriptDsl.Parse("! trig=ticks:0\n0 a:2\n10 x:1\n");

        foreach (var separator in new[] { "\r\n", "\r" })
        {
            var document = ScriptDsl.Parse("! trig=ticks:0\n0 a:2\n10 x:1\n".Replace("\n", separator));

            Assert.Equal(2, document.Commands.Count);
            Assert.Equal(10u, document.Commands[1].T);
            Assert.Equal(ScriptDsl.Write(canonical), ScriptDsl.Write(document));
        }
    }

    [Theory]
    [InlineData("a\rb", "a\nb")]
    [InlineData("a\r\nb", "a\nb")]
    [InlineData("a\nb", "a\nb")]
    public void Puts_a_text_into_the_formats_own_separators(string asGiven, string expected) =>
        Assert.Equal(expected, ScriptDsl.Lines(asGiven));

    [Fact]
    public void Takes_a_bare_frame_as_a_marker_without_input()
    {
        var document = ScriptDsl.Parse("0 a:1\n30\n40 x:1\n");

        Assert.Equal(2, document.Commands.Count);
        Assert.Equal(40u, document.Commands[1].T);
    }

    [Fact]
    public void Writes_a_stick_at_rest_as_the_zero_axis()
    {
        var text = ScriptDsl.Write(Document(Command(9, 1, new ScriptInput { LeftStick = [0f, 0f] })));

        Assert.Equal("9 lsx:0\n", text);
        Assert.Equal(text, ScriptDsl.Write(ScriptDsl.Parse(text)));
    }

    [Theory]
    // Every failure names its line: the text is written by hand as often as by the editor, and
    // "something is wrong somewhere" is not an answer.
    [InlineData("0 zz:1\n", "line 1: unknown token 'zz'")]
    [InlineData("0 a\nzz=1\n", "line 2: frame 'zz=1' is not a number")]
    [InlineData("0 a:0\n", "line 1: 'a' duration must be >= 1")]
    [InlineData("0 a a\n", "line 1: 'jump' is given twice")]
    [InlineData("0 y by\n", "line 1: 'heavy_attack' is given twice")]
    [InlineData("0 by b\n", "line 1: 'zandatsu' is given twice")]
    [InlineData("0 ls:0 ls:90\n", "line 1: 'ls' is given twice")]
    [InlineData("0 bs:0\n", "line 1: unknown token 'bs'")]
    [InlineData("0 ls:0 lsx:5\n", "line 1: 'lsx' and an angle on the same line say two different sticks")]
    [InlineData("0 lsx:5 ls:0\n", "line 1: 'ls' and an exact stick value on the same line say two different sticks")]
    [InlineData("0 lsx:5 lsx:6\n", "line 1: 'lsx' is given twice")]
    [InlineData("0 ls\n", "line 1: 'ls' needs a value and an optional duration")]
    [InlineData("0 ls:abc\n", "line 1: ls 'abc' is not a number")]
    [InlineData("0 ls:0:0\n", "line 1: 'ls' duration must be >= 1")]
    [InlineData("! name=a name=b\n", "line 1: name is given twice")]
    [InlineData("! trig=now:1\n", "line 1: trig must be pos or ticks")]
    [InlineData("! trig=pos:1,2\n", "line 1: trig=pos needs three numbers")]
    [InlineData("! restart:ups=1,nope=2\n", "line 1: unknown restart parameter 'nope'")]
    [InlineData("! name\n", "line 1: 'name' needs a value")]
    [InlineData("! nope=1\n", "line 1: unknown attribute 'nope'")]
    [InlineData("0 a\n! name=x\n", "line 2: the rules line must come first")]
    public void Refuses_bad_text_with_its_line(string text, string message) =>
        Assert.Contains(message, Assert.Throws<ScriptFormatException>(() => ScriptDsl.Parse(text)).Message);

    [Fact]
    public void A_refusal_names_the_frame_it_was_on()
    {
        // A `.tas` is navigated by frame, so a refusal carries one whenever the parse has read a
        // frame line: the line says where in the file the text broke and the frame says where in the
        // script — the frame of the offending line itself, which is the one to look at. Reported from
        // the editor as `line N: … · up to frame F` (`ScriptFormatException.FrameAware`).
        var refused = Assert.Throws<ScriptFormatException>(
            () => ScriptDsl.Parse("! trig=ticks:0\n0 a:2\n132 lsx:997:2 lsy:79:2 lt:2d\n"));

        Assert.Equal(132u, refused.Frame);
        Assert.Equal("line 3: lt '2d' is not a number", refused.Message);
        Assert.EndsWith("· up to frame 132", refused.FrameAware());
    }

    [Fact]
    public void A_refusal_before_any_frame_line_names_no_frame()
    {
        // Nothing has been parsed into a frame yet, so there is none to name — the message stays the
        // parser's own wording rather than carrying a made-up zero.
        var refused = Assert.Throws<ScriptFormatException>(() => ScriptDsl.Parse("! nope=1\n0 a\n"));

        Assert.Null(refused.Frame);
        Assert.DoesNotContain("frame", refused.FrameAware());
    }

    [Fact]
    public void Refuses_a_script_the_mod_would_not_take()
    {
        // The cross-field limits are the mod's own (`parse_script`): the editor checks them on the
        // way in, so a text can never describe a script the game answers 400 on.
        Assert.Contains("commands is empty", Assert.Throws<ScriptFormatException>(() => ScriptDsl.Parse("# nothing\n")).Message);
        var past = ScriptJson.MaxFrames;
        Assert.Contains("commands[0]: t+duration exceeds max", Assert.Throws<ScriptFormatException>(() => ScriptDsl.Parse($"{past - 100} a:200\n")).Message);
    }

    [Fact]
    public void Refuses_to_write_what_the_text_cannot_say()
    {
        var plain = Command(0, 1, new ScriptInput { Forward = true });

        var conditional = plain with { WhenEnemy = new EnemyCondition { Anim = [65545] } };
        Assert.Contains(
            "when_enemy has no DSL spelling",
            Assert.Throws<ScriptFormatException>(() => ScriptDsl.Write(Document(conditional))).Message);

        var raw = plain with { Input = new ScriptInput { RawKey = 139 } };
        Assert.Contains(
            "raw_key/dik_key have no DSL spelling",
            Assert.Throws<ScriptFormatException>(() => ScriptDsl.Write(Document(raw))).Message);
    }

    [Fact]
    public void Refuses_a_frame_where_two_commands_move_the_stick_differently()
    {
        var document = Document(
            Command(0, 5, new ScriptInput { LeftStick = [0f, -1000f] }),
            Command(0, 3, new ScriptInput { LeftStick = [1000f, 0f] }));

        Assert.Contains(
            "two commands move the left stick differently",
            Assert.Throws<ScriptFormatException>(() => ScriptDsl.Write(document)).Message);
    }

    [Fact]
    public void Merges_an_input_held_by_two_commands_into_its_longest_run()
    {
        // The mod ORs the inputs of every active command, so these two are one longer hold — the
        // shortest form of the same script.
        var document = Document(
            Command(0, 5, new ScriptInput { Forward = true, Jump = true }),
            Command(0, 9, new ScriptInput { Forward = true }));

        Assert.Equal("0 ls:0:9 a:5\n", ScriptDsl.Write(document));
    }

    /// Every token the text knows with the spelling it is written back as.
    static readonly (string Token, string Canonical)[] Tokens =
    [
        ("a", "a"),
        ("b", "b"),
        ("x", "x"),
        ("y", "y"),
        ("by", "by"),
        ("lt", "lt"),
        ("rt", "rt"),
        ("lb", "lb"),
        ("rb", "rb"),
        ("r", "r"),
        ("lr", "lr"),
        ("ax", "ax"),
        ("du", "du"),
        ("dd", "dd"),
        ("dl", "dl"),
        ("dr", "dl"),
        ("mu", "mu"),
        ("md", "md"),
        ("ml", "ml"),
        ("mr", "mr"),
        ("ok", "ok"),
        ("esc", "esc"),
        ("cd", "cd"),
        ("wk", "wk"),
    ];

    static string Number(double value) => value.ToString(CultureInfo.InvariantCulture);

    static ScriptDocument Document(params ScriptCommand[] commands) => new() { Commands = [.. commands] };

    static ScriptCommand Command(uint t, uint duration, ScriptInput input) =>
        new() { T = t, Duration = duration, Input = input };

    static ScriptCommand WithDuration(ScriptDocument document, uint duration) =>
        Assert.Single(document.Commands, command => command.Duration == duration);
}
