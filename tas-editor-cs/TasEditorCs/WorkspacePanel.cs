using System;
using System.Collections.Generic;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using static Microsoft.UI.Reactor.Factories;

sealed record WorkspacePanelProps(
    IReadOnlyList<ScriptEntry> Scripts,
    string? SelectedId,
    Action<string> Select);

/// Left pane: the scripts in the workspace, plus the actions that manage them.
///
/// Stub — the list and the selection are live, the management buttons are placeholders.
sealed class WorkspacePanel : Component<WorkspacePanelProps>
{
    public override Element Render() => View(Props.Scripts, Props.SelectedId, Props.Select);

    /// The pane body. Split out of the component because `Component<TProps>.Props` is
    /// read-only and set by the host, so a headless unit test has no way to render the
    /// component itself — it asserts on this instead.
    internal static Element View(
        IReadOnlyList<ScriptEntry> scripts,
        string? selectedId,
        Action<string> select)
    {
        var list = ListView(scripts, s => s.Id, (script, _) => Row(script, script.Id == selectedId))
            with
            {
                SelectedIndex = IndexOf(scripts, selectedId),
                OnSelectedIndexChanged = index =>
                {
                    if (index >= 0 && index < scripts.Count) select(scripts[index].Id);
                },
            };

        var management = HStack(8,
            Button("New", NotWiredYet).IsEnabled(false),
            Button("Duplicate", NotWiredYet).IsEnabled(false),
            Button("Delete", NotWiredYet).IsEnabled(false)
        );

        return (FlexColumn(
            Heading("Scripts"),
            list.Flex(grow: 1, basis: 0),
            management,
            Caption("Management is not wired yet — the workspace on disk comes next.")
        ) with { RowGap = 12 })
        .FlexPadding(12)
        .Flex(grow: 1);
    }

    // Emphasis here comes from the typography helpers (`Heading`, `Caption`), never from a
    // hand-applied `.SemiBold()`: that modifier resolves `Microsoft.UI.Text.FontWeights`,
    // a WinRT object the headless unit layer cannot activate — it throws COMException
    // ("class not registered") and would make this whole pane untestable.
    internal static Element Row(ScriptEntry script, bool selected) =>
        VStack(2,
            TextBlock(script.Name),
            Caption($"{script.Frames} frames")
        )
        .Padding(8)
        .Margin(bottom: 4)
        .CornerRadius(4)
        .Background(selected ? Theme.SubtleFill : Theme.CardBackground);

    static int IndexOf(IReadOnlyList<ScriptEntry> scripts, string? id)
    {
        for (var i = 0; i < scripts.Count; i++)
        {
            if (scripts[i].Id == id) return i;
        }
        return -1;
    }

    static void NotWiredYet() { }
}
