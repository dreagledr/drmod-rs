using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace TasEditorCs.Tests;

/// The script text region. Structural like the other pane tests: the text box is asserted as the
/// element it is and is never activated as a WinUI control.
public class ScriptTextEditorTests
{
    const string Draft = "! name=blade-run\n0 ls:0:6\n6 a:2\n";

    static readonly ScriptTextStatus Read = ScriptTextStatus.Of(Draft);
    static readonly ScriptTextStatus Refused = ScriptTextStatus.Of("0 zz\n");

    [Fact]
    public void Shows_the_text_it_was_given()
    {
        Assert.Equal(Draft, Box(Read).Value.Value);
    }

    [Fact]
    public void Hands_back_what_is_typed()
    {
        var typed = new List<string>();
        var box = Box(Read, typed: typed.Add);

        Assert.NotNull(box.OnChanged);
        box.OnChanged("0 a\n");

        Assert.Equal(new[] { "0 a\n" }, typed);
    }

    [Fact]
    public void Keeps_one_line_per_frame()
    {
        // A wrapped line would read as two frames, so the editor scrolls sideways instead.
        var box = Box(Read);

        Assert.True(box.AcceptsReturn);
        Assert.Equal(TextWrapping.NoWrap, box.TextWrapping);
        Assert.False(box.IsSpellCheckEnabled);
    }

    [Fact]
    public void Sums_up_a_text_it_read()
    {
        Assert.Equal("blade-run · 2 commands · last frame 8", Status(Read));
        Assert.Equal(ScriptTextEditor.Summary(Read.Document!), Status(Read));
    }

    [Fact]
    public void Shows_a_refused_text_with_the_parsers_own_message()
    {
        Assert.StartsWith("line 1: unknown token 'zz'", Status(Refused));
    }

    [Theory]
    [InlineData("a\rb", "a\nb")]
    [InlineData("a\r\nb", "a\nb")]
    [InlineData("a\nb", "a\nb")]
    public void Puts_the_formats_line_separator_back(string asReported, string expected)
    {
        Assert.Equal(expected, ScriptTextEditor.Lines(asReported));
    }

    [Fact]
    public void Reads_a_draft_the_way_the_text_box_reports_it()
    {
        // Measured live: a WinUI `TextBox` hands its text back with a lone `\r`, so a draft stored
        // as it comes would read as one long line — the caption would paint an error the user
        // cannot clear by editing the text.
        var reported = ScriptTextStatus.Of(ScriptTextEditor.Lines(Draft.Replace('\n', '\r')));

        Assert.True(reported.IsOk);
        Assert.Equal(8u, ScriptTextStatus.LastFrame(reported.Document!));
    }

    [Fact]
    public void Gives_the_text_the_row_that_is_left_over()
    {
        // The status line takes what it needs, the editor takes the rest. A star row rather than a
        // flex slot is what makes the box fill the region — a `TextBox` stays content-sized in a
        // flex slot it was given (measured live: 92.67 DIP of 116.75).
        var grid = Body(Read);

        Assert.Equal(
            new[] { GridUnitType.Auto, GridUnitType.Star },
            grid.Definition.Rows.Select(row => row.Type));
        Assert.Equal(new[] { GridUnitType.Star }, grid.Definition.Columns.Select(column => column.Type));
        Assert.IsType<TextBlockElement>(grid.Children[0]);
        Assert.IsType<TextBoxElement>(grid.Children[1]);
    }

    static string? Status(ScriptTextStatus status) =>
        Assert.IsType<TextBlockElement>(Body(status).Children[0]).Content;

    static TextBoxElement Box(ScriptTextStatus status, string text = Draft, Action<string>? typed = null) =>
        Assert.IsType<TextBoxElement>(Body(status, text, typed).Children[1]);

    static GridElement Body(ScriptTextStatus status, string text = Draft, Action<string>? typed = null) =>
        Assert.IsType<GridElement>(ScriptTextEditor.View(text, typed ?? (_ => { }), status));
}
