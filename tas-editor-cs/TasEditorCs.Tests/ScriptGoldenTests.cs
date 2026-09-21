namespace TasEditorCs.Tests;

/// The fixtures end to end: the JSON the Rust tool generates from the mod's DTOs,
/// the text and the frames the editor makes of it, and the goldens that pin both
/// down.
///
/// The JSON is the source of truth; the text format and the command table are
/// views of it, and these tests say exactly how much each view can lose.
public class ScriptGoldenTests
{
    [Fact]
    public void The_editor_writes_the_json_of_every_fixture_back_into_its_golden()
    {
        Assert.NotEmpty(ScriptFixtures.Fixtures());

        foreach (var name in ScriptFixtures.Fixtures())
        {
            var written = ScriptJson.Write(ScriptJson.Read(ScriptFixtures.Read(name)));
            ScriptFixtures.ExpectGolden(ScriptFixtures.GoldenName(name, ScriptFixtures.JsonExtension), written);
        }
    }

    [Fact]
    public void The_editor_writes_the_text_of_every_expressible_fixture_into_its_golden()
    {
        foreach (var name in ScriptFixtures.Fixtures())
        {
            var document = ScriptJson.Read(ScriptFixtures.Read(name));
            var golden = ScriptFixtures.GoldenName(name, ScriptFixtures.DslExtension);
            if (!Expressible(document))
            {
                // raw_key, dik_key and when_enemy have no DSL spelling, so a fixture
                // holding them has no text — and must not pretend to have a golden.
                Assert.False(ScriptFixtures.Exists(golden), $"{golden} exists, but the script has no text form");
                continue;
            }

            var text = ScriptDsl.Write(document);
            ScriptFixtures.ExpectGolden(golden, text);

            // A golden is what a user edits: reading it back has to give the same
            // text, or every round trip through the editor would rewrite the file.
            Assert.Equal(text, ScriptDsl.Write(ScriptDsl.Parse(text)));
        }
    }

    [Fact]
    public void The_text_of_a_fixture_is_the_same_script_as_its_json()
    {
        foreach (var name in ScriptFixtures.Fixtures())
        {
            var document = ScriptJson.Read(ScriptFixtures.Read(name));
            if (!Expressible(document))
            {
                continue;
            }

            var fromText = ScriptDsl.Parse(ScriptDsl.Write(document));

            // The rules line is a lossless spelling of name/trigger/restart, so comparing the
            // written rules compares the fields themselves.
            Assert.Equal(RulesLine(document), RulesLine(fromText));

            // Movement goes through the stick in the text and through the direction flags in the
            // JSON — both drive the character (`docs/API.md` §10.2), so the four movement bits are
            // what the two forms are allowed to differ in. The stick itself is compared.
            ScriptFramesTests.AssertSameFrames(
                ScriptFrames.Expand(document), ScriptFrames.Expand(fromText), ignoreMovement: true);
        }
    }

    [Fact]
    public void The_table_of_the_input_fixture_covers_every_command()
    {
        var document = ScriptJson.Read(ScriptFixtures.Read("all_inputs.json"));

        // The last command starts at frame 45 and lasts five frames.
        Assert.Equal(50, ScriptFrames.Expand(document).Count);
    }

    /// The rules line of a document's text — everything the text says that is not
    /// a frame.
    static string RulesLine(ScriptDocument document) => ScriptDsl.Write(document).Split('\n')[0];

    /// Whether the text format can say this script at all.
    static bool Expressible(ScriptDocument document)
    {
        try
        {
            ScriptDsl.Write(document);
            return true;
        }
        catch (ScriptFormatException)
        {
            return false;
        }
    }
}
