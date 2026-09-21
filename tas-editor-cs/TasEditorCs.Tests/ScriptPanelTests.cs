using Microsoft.UI.Reactor.Core;

namespace TasEditorCs.Tests;

/// The right-hand pane. Assertions are structural: `Element` is a record, so the rendered
/// tree can be inspected directly. Nothing here creates a WinUI control — a headless test
/// cannot, and gets a COMException if it tries.
public class ScriptPanelTests
{
    [Fact]
    public void Shows_the_selected_script()
    {
        var children = Children(new ScriptEntry("s1", "blade-run", 42));

        Assert.Equal("blade-run", Text(children[0]));
        Assert.Equal("42 frames", Text(children[1]));
    }

    [Fact]
    public void Says_so_when_nothing_is_selected()
    {
        var children = Children(null);

        Assert.Single(children);
        Assert.Equal("No script selected.", Text(children[0]));
    }

    static Element[] Children(ScriptEntry? script) =>
        Assert.IsType<FlexElement>(ScriptPanel.View(script)).Children;

    static string? Text(Element element) => Assert.IsType<TextBlockElement>(element).Content;
}
