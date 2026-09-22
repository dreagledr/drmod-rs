using System;
using System.Collections.Generic;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;            // TextWrapping
using static Microsoft.UI.Reactor.Factories;

sealed record WorkspacePanelProps(
    string? Folder,
    IReadOnlyList<ScriptEntry> Scripts,
    IReadOnlyDictionary<string, string> Buffers,
    string? SelectedPath,
    string? Error,
    Action<string> Select,
    Action ChooseFolder,
    Action New,
    Action Duplicate,
    Action Delete);

/// Left pane: the `.tas` files of the open folder, and the operations that manage them.
///
/// The pane is a value: it holds no state of its own, so everything it paints — the selection, the
/// unsaved markers, the folder it shows — comes from the shell that owns the workspace. That is
/// also what keeps it testable: the row and the body take their arguments rather than reaching for
/// them.
sealed class WorkspacePanel : Component<WorkspacePanelProps>
{
    public override Element Render() => View(Props);

    /// The pane body. Split out of the component because `Component<TProps>.Props` is
    /// read-only and set by the host, so a headless unit test has no way to render the
    /// component itself — it asserts on this instead.
    internal static Element View(WorkspacePanelProps props)
    {
        var scripts = props.Scripts;
        var list = ListView(scripts, script => script.Path, (script, _) => Row(script, script.Path == props.SelectedPath, props.Buffers.ContainsKey(script.Path)))
            with
            {
                SelectedIndex = IndexOf(scripts, props.SelectedPath),
                OnSelectedIndexChanged = index =>
                {
                    if (index >= 0 && index < scripts.Count) props.Select(scripts[index].Path);
                },
            };

        // A file action needs a folder to act on, and Duplicate and Delete need a script: a
        // disabled button is what says so, rather than a click that quietly does nothing.
        var hasFolder = props.Folder is not null;
        var hasSelection = IndexOf(scripts, props.SelectedPath) >= 0;
        var management = HStack(8,
            Button("New", props.New).IsEnabled(hasFolder),
            Button("Duplicate", props.Duplicate).IsEnabled(hasFolder && hasSelection),
            Button("Delete", props.Delete).IsEnabled(hasSelection)
        );

        return (FlexColumn(
            Heading("Scripts"),
            Folder(props),
            list.Flex(grow: 1, basis: 0),
            management,
            Note(props)
        ) with { RowGap = 12 })
        .FlexPadding(12)
        .Flex(grow: 1);
    }

    /// The workspace itself: the button that picks a folder, and the folder it is on. The path
    /// wraps because a pane is 340 DIP wide and a real path is longer than that — a clipped path
    /// says less than a wrapped one.
    static Element Folder(WorkspacePanelProps props) =>
        VStack(6,
            Button("Open folder…", props.ChooseFolder),
            Caption(props.Folder ?? "No folder open")
                .Foreground(Theme.SecondaryText)
                .TextWrapping(TextWrapping.Wrap));

    /// The line under the buttons: what went wrong, or what the pane is for. The error keeps the
    /// workspace's own wording — it already names the file or the folder — and is painted in the
    /// theme's error color, so a failure never reads like a note.
    static Element Note(WorkspacePanelProps props)
    {
        if (props.Error is { } error)
        {
            return Caption(error).Foreground(Theme.SystemCritical).TextWrapping(TextWrapping.Wrap);
        }

        if (props.Folder is null)
        {
            return Caption("Pick a folder to work on. Only .tas files are scripts here.");
        }

        var count = props.Scripts.Count;
        return Caption(count == 1 ? "1 script in the folder" : $"{count} scripts in the folder");
    }

    // Emphasis here comes from the typography helpers (`Heading`, `Caption`), never from a
    // hand-applied `.SemiBold()`: that modifier resolves `Microsoft.UI.Text.FontWeights`,
    // a WinRT object the headless unit layer cannot activate — it throws COMException
    // ("class not registered") and would make this whole pane untestable.
    internal static Element Row(ScriptEntry script, bool selected, bool unsaved) =>
        VStack(2,
            TextBlock(script.Name),
            Detail(script, unsaved)
        )
        .Padding(8)
        .Margin(bottom: 4)
        .CornerRadius(4)
        .Background(selected ? Theme.SubtleFill : Theme.CardBackground);

    /// The second line of a row: what the file says, or the reason it says nothing. A file that
    /// does not read as a script is worth knowing about before it is opened, so its row carries
    /// the parser's own message in the error color; `<N> frames` is the frame count the listing
    /// read out of the text.
    static Element Detail(ScriptEntry script, bool unsaved)
    {
        var unsavedMark = unsaved ? "unsaved · " : string.Empty;
        return script.Error is { } error
            ? Caption(unsavedMark + error).Foreground(Theme.SystemCritical)
            : Caption($"{unsavedMark}{script.Frames} frames");
    }

    static int IndexOf(IReadOnlyList<ScriptEntry> scripts, string? path)
    {
        if (path is null) return -1;
        for (var i = 0; i < scripts.Count; i++)
        {
            if (scripts[i].Path == path) return i;
        }
        return -1;
    }
}
