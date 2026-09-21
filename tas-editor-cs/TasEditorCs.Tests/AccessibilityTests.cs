using Microsoft.UI.Reactor.Core;

namespace TasEditorCs.Tests;

/// The accessibility scanner walks an `Element` tree without a window, so it runs in the
/// headless unit layer. Asserting on one specific rule rather than on "no findings at all"
/// keeps this from failing over informational diagnostics.
public class AccessibilityTests
{
    [Fact]
    public void The_workspace_pane_has_no_unnamed_icon_only_button()
    {
        var view = WorkspacePanel.View([new ScriptEntry("s1", "blade-run", 42)], "s1", _ => { });

        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_001");
    }

    [Fact]
    public void The_script_pane_has_no_unnamed_icon_only_button()
    {
        var view = ScriptPanel.View(new ScriptEntry("s1", "blade-run", 42));

        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_001");
    }
}
