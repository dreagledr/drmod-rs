using System.Text.Json;

namespace TasEditorCs.Tests;

/// The API JSON of a script. The fixtures come from the Rust tool
/// (`tools/script_gen`), which builds them from the mod's own DTOs — so what these
/// tests really check is that the editor agrees with the mod about the format.
public class ScriptJsonTests
{
    [Fact]
    public void Reads_every_fixture()
    {
        var names = ScriptFixtures.Fixtures();

        Assert.NotEmpty(names);
        foreach (var name in names)
        {
            var document = ScriptJson.Read(ScriptFixtures.Read(name));
            Assert.False(string.IsNullOrEmpty(document.Name));
            Assert.NotEmpty(document.Commands);
        }
    }

    [Fact]
    public void Writing_what_was_read_is_a_fixed_point()
    {
        foreach (var name in ScriptFixtures.Fixtures())
        {
            var once = ScriptJson.Write(ScriptJson.Read(ScriptFixtures.Read(name)));
            var twice = ScriptJson.Write(ScriptJson.Read(once));

            Assert.Equal(once, twice);
        }
    }

    [Fact]
    public void Keeps_the_defaults_the_json_leaves_out()
    {
        var document = ScriptJson.Read(ScriptFixtures.Read("minimal.json"));

        // The fixture is `commands` and nothing else: the mod names it "script"
        // and neither arms it nor restarts the mission.
        Assert.Equal(ScriptDocument.DefaultName, document.Name);
        Assert.Null(document.Trigger);
        Assert.Null(document.Restart);
    }

    [Fact]
    public void Carries_the_edge_inputs_the_text_format_cannot_say()
    {
        var commands = ScriptJson.Read(ScriptFixtures.Read("edge_inputs.json")).Commands;

        Assert.Equal(139u, commands[0].Input.RawKey);
        Assert.Equal(208u, commands[1].Input.DikKey);
        Assert.Equal([65545], commands[3].WhenEnemy!.Anim!);
        Assert.Equal(0.3f, commands[3].WhenEnemy!.PlayerYMin);
        Assert.Equal(0.0f, commands[3].WhenEnemy!.PlayerVyMax);
        Assert.True(commands[4].WhenEnemy!.Repeat);
        Assert.Equal(1.5f, commands[4].WhenEnemy!.PlayerVyMax);
        Assert.Equal(60, commands[4].WhenEnemy!.FrameMax);
        Assert.Equal(5, commands[4].WhenEnemy!.FrameMin);
    }

    [Fact]
    public void Reads_the_trigger_and_restart_of_the_rules_fixtures()
    {
        var restart = ScriptJson.Read(ScriptFixtures.Read("rules_restart.json"));

        Assert.Equal([-24.7f, 12.15f, 120.7f], restart.Trigger!.Pos!);
        Assert.Null(restart.Trigger.Ticks);
        Assert.Equal(2u, restart.Restart!.Ups);
        Assert.Equal(3u, restart.Restart.Confirms);
        Assert.Equal(30u, restart.Restart.ConfirmGap);

        var ticks = ScriptJson.Read(ScriptFixtures.Read("rules_ticks.json"));

        Assert.Equal(0ul, ticks.Trigger!.Ticks);
        Assert.Null(ticks.Trigger.Pos);
        Assert.Null(ticks.Restart);
    }

    [Fact]
    public void Keeps_the_mod_defaults_for_the_restart_parameters_the_json_leaves_out()
    {
        // Measured on .NET 10: the source-generated deserializer writes `default`
        // over every property a JSON omits, so an absent `hold` inside `restart`
        // arrives as null rather than as the initializer — and a plain `uint` would
        // read it as a literal 0, "hold the arrow for no frames at all".
        var document = ScriptJson.Read("""
            { "restart": { "ups": 2 },
              "commands": [ { "t": 0, "duration": 1, "input": { "jump": true } } ] }
            """);

        Assert.Equal(2u, document.Restart!.Ups!.Value);
        Assert.Null(document.Restart.Hold);

        // A parameter left unset stays out of the JSON, so the mod applies its own
        // default instead of the editor inventing a zero.
        Assert.DoesNotContain("hold", ScriptJson.Write(document));
    }

    [Fact]
    public void Writes_only_the_inputs_that_are_set()
    {
        var document = ScriptJson.Read("""
            { "commands": [ { "t": 0, "duration": 1, "input": { "forward": true } } ] }
            """);

        var json = ScriptJson.Write(document);

        // The frame number and the duration are the two fields a value type would
        // drop as "default"; everything unset or false stays out, so the JSON reads
        // like the format's own examples instead of 26 lines of `false`.
        Assert.Contains("\"t\": 0", json);
        Assert.Contains("\"duration\": 1", json);
        Assert.Contains("\"forward\": true", json);
        Assert.DoesNotContain("\"jump\"", json);
        Assert.DoesNotContain("camera", json);
        Assert.DoesNotContain("trigger", json);
    }

    [Fact]
    public void Refuses_an_unknown_input_key()
    {
        const string json = """
            { "commands": [ { "t": 0, "duration": 1, "input": { "light_attackk": true } } ] }
            """;

        // The same typo protection the mod gets from `deny_unknown_fields`: a
        // misspelled key must not become a silently absent input.
        Assert.Throws<JsonException>(() => ScriptJson.Read(json));
    }

    [Fact]
    public void Refuses_an_unknown_command_field()
    {
        const string json = """
            { "commands": [ { "t": 0, "duration": 1, "inputs": {} } ] }
            """;

        Assert.Throws<JsonException>(() => ScriptJson.Read(json));
    }

    [Fact]
    public void Refuses_a_negative_duration()
    {
        const string json = """
            { "commands": [ { "t": 0, "duration": -1, "input": { "jump": true } } ] }
            """;

        // `duration` is unsigned, exactly as in the mod: the failure has to happen
        // on the way in, not in the game.
        Assert.Throws<JsonException>(() => ScriptJson.Read(json));
    }

    [Theory]
    // The mod's own cross-field checks (`parse_script` in src/api.rs): the editor
    // refuses what the game would answer 400 on.
    [InlineData("""{ "commands": [] }""")]
    [InlineData("""{ "commands": [ { "t": 0, "duration": 1, "input": {} } ] }""")]
    [InlineData("""{ "commands": [ { "t": 0, "duration": 0, "input": { "jump": true } } ] }""")]
    [InlineData("""{ "trigger": {}, "commands": [ { "t": 0, "duration": 1, "input": { "jump": true } } ] }""")]
    [InlineData("""{ "commands": [ { "t": 0, "duration": 1, "input": { "camera": [1, 2, 3] } } ] }""")]
    public void Refuses_what_the_mod_would_answer_400_on(string json) =>
        Assert.Throws<ScriptFormatException>(() => ScriptJson.Read(json));

    [Fact]
    public void Refuses_a_script_past_the_frame_ceiling()
    {
        // Named separately from the theory above because the ceiling is a constant, and an
        // `InlineData` is frozen at compile time — this way the test follows `ScriptJson.MaxFrames`
        // instead of pinning the number it happened to be.
        var json = $$"""
            { "commands": [ { "t": 10, "duration": {{ScriptJson.MaxFrames}}, "input": { "jump": true } } ] }
            """;

        Assert.Throws<ScriptFormatException>(() => ScriptJson.Read(json));
    }

    [Fact]
    public void Refuses_a_name_longer_than_the_mod_allows()
    {
        var json = $$"""
            { "name": "{{new string('x', ScriptJson.MaxNameLength + 1)}}",
              "commands": [ { "t": 0, "duration": 1, "input": { "jump": true } } ] }
            """;

        Assert.Throws<ScriptFormatException>(() => ScriptJson.Read(json));
    }
}
