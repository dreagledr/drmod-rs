using System.Collections.Generic;
using Microsoft.UI.Reactor.Core;

namespace TasEditorCs.Tests;

/// The accessibility scanner walks an `Element` tree without a window, so it runs in the
/// headless unit layer. Asserting on one specific rule rather than on "no findings at all"
/// keeps this from failing over informational diagnostics.
public class AccessibilityTests
{
    static readonly ScriptEntry Script =
        new(@"C:\workspace\blade-run.tas", "blade-run", "! trig=ticks:0\n0 a\n", 1, null);

    [Fact]
    public void The_workspace_pane_has_no_unnamed_icon_only_button()
    {
        var view = WorkspacePanel.View(new WorkspacePanelProps(
            @"C:\workspace",
            [Script],
            ScriptBuffers.Empty,
            Script.Path,
            null,
            _ => { },
            () => { },
            () => { },
            () => { },
            () => { },
            () => { },
            ModPanelFixture.View()));

        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_001");
    }

    [Fact]
    public void The_script_pane_has_no_unnamed_icon_only_button()
    {
        var view = ScriptPanel.View(new ScriptPanelView(
            Script,
            Script.Text,
            ScriptTextStatus.Of(Script.Text),
            StandardCommand.Save(() => { }, canExecute: false),
            Controls(),
            _ => { }));

        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_001");
    }

    [Fact]
    public void The_script_text_region_names_its_editor()
    {
        // A bare text box has no caption of its own, and the line above it is a status message
        // rather than a label — so the automation name is what names the field.
        var view = ScriptTextEditor.View(
            ScriptTextEditorTests.RegionFor(ScriptTextStatus.Of("0 a\n")));

        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_003");
    }

    [Fact]
    public void The_run_controls_name_their_fields()
    {
        // The rules row is three checkboxes, a combo box, a number box and a seed field: the checkboxes
        // carry their own label and the others carry a header (`CheckBox`'s label is content, which is
        // why only the fields the scanner knows about can be pinned by a test).
        var view = ScriptControls.View(Controls());

        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_003");
        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_001");
    }

    [Fact]
    public void The_rename_dialog_names_its_field_and_its_buttons()
    {
        // Same rule as the delete question: a modal is the one surface a user cannot navigate around,
        // so its field and both answers have to reach a screen reader with a name of their own.
        var dialog = Editor.RenameScript(Script, "blade-run", _ => { }, _ => { });

        Assert.DoesNotContain(AccessibilityScanner.Scan(dialog), f => f.Id == "A11Y_003");
        Assert.DoesNotContain(AccessibilityScanner.Scan(dialog), f => f.Id == "A11Y_001");
    }

    [Fact]
    public void The_delete_confirmation_names_its_buttons()
    {
        // A modal is the one surface a user cannot navigate around: the destructive answer has to
        // reach a screen reader with its own label.
        var dialog = Editor.Confirm(Script, unsaved: true, _ => { });

        Assert.DoesNotContain(AccessibilityScanner.Scan(dialog), f => f.Id == "A11Y_001");
    }

    [Fact]
    public void The_command_reference_has_no_unnamed_icon_only_button()
    {
        // The region's own button carries the reference's name (A11Y_001), and the reference
        // itself is text — no row of it is clickable.
        var view = ScriptTextEditor.View(
            ScriptTextEditorTests.RegionFor(ScriptTextStatus.Of("0 a\n"), reference: true));

        Assert.DoesNotContain(AccessibilityScanner.Scan(view), f => f.Id == "A11Y_001");
    }

    static ScriptControlsView Controls() => new(
        ScriptTextStatus.Of(Script.Text),
        Dirty: false,
        StandardCommand.Save(() => { }, canExecute: false),
        PlaybackRules.Default,
        "1",
        GameStatus.Offline,
        Preparing: false,
        Error: null,
        _ => { },
        _ => { },
        () => { },
        () => { },
        () => { });
}
