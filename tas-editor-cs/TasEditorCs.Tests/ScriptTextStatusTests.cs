namespace TasEditorCs.Tests;

/// The status line's source: what a text parses to, or why it does not parse.
public class ScriptTextStatusTests
{
    [Fact]
    public void Reads_a_text_into_its_document_and_last_frame()
    {
        var status = ScriptTextStatus.Of("! name=run trig=ticks:0\n0 ls:0:10\n10 a:2\n");

        Assert.True(status.IsOk);
        Assert.Equal("run", status.Document!.Name);
        Assert.Equal(2, status.Document.Commands.Count);
        Assert.Equal(12u, ScriptTextStatus.LastFrame(status.Document));
    }

    [Fact]
    public void Names_the_line_of_a_token_it_cannot_read()
    {
        var status = ScriptTextStatus.Of("0 ls:0\n1 zz\n");

        Assert.False(status.IsOk);
        Assert.Null(status.Document);
        Assert.StartsWith("line 2: unknown token 'zz'", status.Error);
    }

    [Fact]
    public void Names_the_command_the_mod_would_refuse()
    {
        // The cross-field limits are the mod's own (`ScriptJson.Validate`), so a text breaking
        // one is refused here rather than handed on to answer a 400. The frame the text ran up to
        // rides behind the message (`ScriptFormatException.FrameAware`), which is why this matches
        // the start rather than the whole of it.
        var status = ScriptTextStatus.Of("3500 a:200\n");

        Assert.False(status.IsOk);
        Assert.StartsWith("commands[0]: t+duration exceeds max 3600", status.Error);
    }

    [Theory]
    [InlineData("")]
    [InlineData("# nothing but a comment\n")]
    [InlineData("! name=script\n")]
    public void A_text_the_mod_would_refuse_is_an_error_and_not_a_document(string text)
    {
        var status = ScriptTextStatus.Of(text);

        Assert.False(status.IsOk);
        Assert.Null(status.Document);
        Assert.Equal("commands is empty", status.Error);
    }
}
