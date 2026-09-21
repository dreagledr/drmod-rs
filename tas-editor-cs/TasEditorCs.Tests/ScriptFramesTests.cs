namespace TasEditorCs.Tests;

/// The command table's view of a script: the frames a document expands to, and the
/// commands those frames collapse back into.
public class ScriptFramesTests
{
    [Fact]
    public void The_input_table_mirrors_the_command_tables_columns()
    {
        Assert.Equal(CommandKeys.All.Length, ScriptInput.Booleans.Length);

        for (var bit = 0; bit < CommandKeys.All.Length; bit++)
        {
            // The DSL token, the table column and the bit of a frame are one list:
            // setting the bit-th field has to light the bit-th column.
            var input = ScriptInput.Booleans[bit].Set(new ScriptInput());
            var row = Assert.Single(ScriptFrames.Expand(Document(input)));

            Assert.Equal(1u << bit, row.Buttons);
            Assert.False(ScriptInput.Booleans[bit].Get(new ScriptInput()));
        }
    }

    [Fact]
    public void Expands_one_row_per_frame_up_to_the_end_of_the_last_command()
    {
        var frames = ScriptFrames.Expand(new ScriptDocument
        {
            Commands =
            [
                Command(0, 6, new ScriptInput { Forward = true, NinjaRun = true }),
                Command(3, 4, new ScriptInput { HeavyAttack = true }),
            ],
        });

        Assert.Equal(7, frames.Count);
        Assert.Equal(Enumerable.Range(0, 7), frames.Select(row => row.Frame));
        Assert.False(frames[2].Holds(IndexOf("heavy_attack")));
        Assert.True(frames[3].Holds(IndexOf("heavy_attack")));
        Assert.True(frames[3].Holds(IndexOf("forward")));
        Assert.True(frames[6].Holds(IndexOf("heavy_attack")));
    }

    [Fact]
    public void Turns_the_movement_flags_into_the_stick_the_mod_would_assemble()
    {
        var forward = Assert.Single(ScriptFrames.Expand(Document(new ScriptInput { Forward = true })));

        // Forward is a full push at the angle 270, and walking halves it — the
        // game reads walking from the stick, not from a key.
        Assert.Equal(270.0, forward.LeftStickAngle, 3);
        Assert.Equal(1.0, forward.LeftStickAmount, 3);

        var walking = Assert.Single(ScriptFrames.Expand(Document(new ScriptInput { Forward = true, Walk = true })));

        Assert.Equal(0.5, walking.LeftStickAmount, 3);

        var diagonal = Assert.Single(ScriptFrames.Expand(Document(new ScriptInput { Forward = true, Right = true })));

        // A diagonal adds up, as the mod's own assembly does: magnitude √2, not 1.
        Assert.Equal(315.0, diagonal.LeftStickAngle, 3);
        Assert.Equal(1.414, diagonal.LeftStickAmount, 3);
    }

    [Fact]
    public void The_walk_token_halves_the_stick_the_text_wrote()
    {
        var walking = ScriptFrames.Expand(ScriptDsl.Parse("0 ls:0 wk:10\n"));

        // `wk` is the mod's own way of saying walking: the stick keeps its direction and drops to
        // half. The text writes the full press and the flag halves it — 0 on the DSL's compass is
        // 270 on the table's, where forward is the angle 270.
        Assert.Equal(0.5, walking[0].LeftStickAmount, 3);
        Assert.Equal(270.0, walking[0].LeftStickAngle, 3);
    }

    [Fact]
    public void Takes_the_stick_from_the_last_command_that_sets_one()
    {
        // The mod's own rule (`script_tick`): the stick is assigned per command, so
        // a later command's directions replace an earlier command's explicit stick,
        // and an explicit stick replaces the directions before it.
        var frames = ScriptFrames.Expand(new ScriptDocument
        {
            Commands =
            [
                Command(0, 4, new ScriptInput { LeftStick = [1000f, 0f] }),
                Command(2, 1, new ScriptInput { Forward = true }),
            ],
        });

        Assert.Equal(0.0, frames[1].LeftStickAngle, 3);   // the explicit stick alone
        Assert.Equal(270.0, frames[2].LeftStickAngle, 3); // forward, one frame later
        Assert.Equal(0.0, frames[3].LeftStickAngle, 3);   // back to the explicit one
    }

    [Fact]
    public void A_released_stick_keeps_the_direction_of_the_last_push()
    {
        var frames = ScriptFrames.Expand(new ScriptDocument
        {
            Commands =
            [
                Command(0, 2, new ScriptInput { Forward = true }),
                Command(6, 1, new ScriptInput { Jump = true }),
            ],
        });

        Assert.Equal(7, frames.Count);
        Assert.Equal(1.0, frames[0].LeftStickAmount, 3);
        Assert.All(frames.Skip(2), row =>
        {
            Assert.Equal(270.0, row.LeftStickAngle, 3);
            Assert.Equal(0.0, row.LeftStickAmount, 3);
        });
    }

    [Fact]
    public void The_right_stick_is_the_commands_camera()
    {
        var frames = ScriptFrames.Expand(Document(new ScriptInput { Camera = [300f, 0f] }));

        Assert.Equal(0.0, frames[0].RightStickAngle, 3);
        Assert.Equal(0.3, frames[0].RightStickAmount, 3);
        Assert.Equal(0.0, frames[0].LeftStickAmount, 3);
    }

    [Fact]
    public void Collapse_keeps_an_implied_stick_implicit_and_an_explicit_one_explicit()
    {
        var implied = Assert.Single(ScriptFrames.Collapse(ScriptFrames.Expand(
            new ScriptDocument { Commands = [Command(0, 3, new ScriptInput { Forward = true })] })));

        Assert.True(implied.Input.Forward);
        Assert.Equal(3u, implied.Duration);
        // Forward already says (0, −1000): writing it out again would be noise.
        Assert.Null(implied.Input.LeftStick);

        var explicitStick = Assert.Single(ScriptFrames.Collapse(ScriptFrames.Expand(
            new ScriptDocument { Commands = [Command(0, 2, new ScriptInput { Forward = true, LeftStick = [1000f, 0f] })] })));

        Assert.Equal([1000f, 0f], explicitStick.Input.LeftStick!);
    }

    [Fact]
    public void Collapse_leaves_a_frame_without_input_out()
    {
        var commands = ScriptFrames.Collapse(ScriptFrames.Expand(new ScriptDocument
        {
            Commands =
            [
                Command(0, 2, new ScriptInput { Forward = true }),
                Command(10, 2, new ScriptInput { Forward = true }),
            ],
        }));

        // The gap between the two runs is not a command: a frame that holds
        // nothing says nothing to the mod.
        Assert.Equal(2, commands.Count);
        Assert.Equal((0u, 2u), (commands[0].T, commands[0].Duration));
        Assert.Equal((10u, 2u), (commands[1].T, commands[1].Duration));
    }

    [Fact]
    public void The_frames_show_only_what_has_a_column()
    {
        var document = ScriptJson.Read(ScriptFixtures.Read("edge_inputs.json"));
        var frames = ScriptFrames.Expand(document);

        // raw_key/dik_key/when_enemy never reach a row: the only frame-level
        // command left is the ripper tap at frame 5, and the table ends with it.
        Assert.Equal(6, frames.Count);
        Assert.All(frames.Take(5), row => Assert.Equal(0u, row.Buttons));
        Assert.True(frames[5].Holds(IndexOf("ripper")));
        Assert.Equal(1.414, frames[5].LeftStickAmount, 3);
    }

    [Fact]
    public void Frames_survive_expand_collapse_expand()
    {
        foreach (var name in ScriptFixtures.Fixtures())
        {
            var document = ScriptJson.Read(ScriptFixtures.Read(name));
            var frames = ScriptFrames.Expand(document);
            var again = ScriptFrames.Expand(document with { Commands = ScriptFrames.Collapse(frames) });

            AssertSameFrames(frames, again);
        }
    }

    /// Rows are equal within the precision the projection keeps: a stick travels
    /// as an angle plus a deflection, so its axes come back within a thousandth,
    /// and nothing else moves at all.
    ///
    /// `ignoreMovement` leaves the four direction bits out of the comparison — a
    /// script that came from the text format carries its movement as the stick
    /// alone, and both reach the game (`docs/API.md` §10.2).
    internal static void AssertSameFrames(
        IReadOnlyList<CommandRow> expected,
        IReadOnlyList<CommandRow> actual,
        bool ignoreMovement = false)
    {
        Assert.Equal(expected.Count, actual.Count);
        for (var index = 0; index < expected.Count; index++)
        {
            var left = expected[index];
            var right = actual[index];

            Assert.Equal(left.Frame, right.Frame);
            Assert.Equal(
                ignoreMovement ? left.Buttons & ~MovementBits : left.Buttons,
                ignoreMovement ? right.Buttons & ~MovementBits : right.Buttons);
            Assert.Equal(left.LeftStickAngle, right.LeftStickAngle, 2);
            Assert.Equal(left.LeftStickAmount, right.LeftStickAmount, 3);
            Assert.Equal(left.RightStickAngle, right.RightStickAngle, 2);
            Assert.Equal(left.RightStickAmount, right.RightStickAmount, 3);
        }
    }

    /// The four flag columns the text format does not spell: movement is the stick there.
    static readonly uint MovementBits = CommandKeys.Mask("forward", "backward", "left", "right");

    internal static int IndexOf(string key) =>
        Array.FindIndex(CommandKeys.All, entry => entry.Key == key);

    static ScriptDocument Document(ScriptInput input) =>
        new() { Commands = [Command(0, 1, input)] };

    static ScriptCommand Command(uint t, uint duration, ScriptInput input) =>
        new() { T = t, Duration = duration, Input = input };
}
