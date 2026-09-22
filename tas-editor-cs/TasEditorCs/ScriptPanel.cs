using System;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;    // UseMemo
using Microsoft.UI.Reactor.Docking; // DockManager, DockSplit, Document, DockNode
using Microsoft.UI.Xaml;            // TextWrapping
using Microsoft.UI.Xaml.Controls;   // Orientation
using static Microsoft.UI.Reactor.Factories;

sealed record ScriptPanelProps(
    ScriptEntry? Script,
    string Text,
    bool Dirty,
    Action<string> TextChanged,
    Action Save);

/// Everything the pane paints, in one record.
///
/// Gathered instead of passed one by one because this is the headless layer's entry point: the
/// component reads a single parse of the text and hands the answer to the regions, and a test
/// renders <see cref="ScriptPanel.View"/> rather than the component — so the pane takes its status
/// as a value rather than computing one.
internal sealed record ScriptPanelView(
    ScriptEntry? Script,
    string Text,
    bool Dirty,
    ScriptTextStatus Status,
    Action<string> TextChanged,
    Action Save);

/// Right pane: the one selected script.
///
/// The body is three regions stacked top to bottom — script controls, command table, script text
/// — separated by the docking host's drag-resize splitters. The set of panes is fixed, so the host
/// keeps its own split ratios across renders and the selection only decides what the regions read.
///
/// The text is the file's, and the buffers that edit it belong to the shell — they are what the
/// list's unsaved markers and the Delete question are about, and two panes cannot read the same
/// state out of one of them.
sealed class ScriptPanel : Component<ScriptPanelProps>
{
    public override Element Render()
    {
        // One parse per render for the whole pane: the status line, the controls region's own line
        // and the Save button's state all come from the same answer, and a 3 600-frame text is not
        // walked three times per keystroke. The text box reports its lines with a lone `\r`, so the
        // text is put into the format's own separators on the way into the parser
        // (`ScriptDsl.Lines`).
        var status = UseMemo(
            () => ScriptTextStatus.Of(ScriptDsl.Lines(Props.Text)),
            Props.Text);

        return View(new ScriptPanelView(
            Props.Script, Props.Text, Props.Dirty, status, Props.TextChanged, Props.Save));
    }

    /// The pane body. Split out of the component because `Component<TProps>.Props` is
    /// read-only and set by the host, so a headless unit test has no way to render the
    /// component itself — it asserts on this instead.
    internal static Element View(ScriptPanelView view)
    {
        if (view.Script is null)
        {
            return FlexColumn(Caption("No script selected."))
                .FlexPadding(16)
                .Flex(grow: 1);
        }

        // Ctrl+S lives with the pane that holds the text: the command carries the accelerator, and
        // the host below is what registers it for this subtree, so the shortcut reaches the editor
        // while the caret is in it.
        var save = StandardCommand.Save(view.Save, view.Dirty);

        // Orientation.Vertical means the splitters between the children are horizontal
        // bars. Initial heights belong on the panes — the split's direct children — and
        // only the last one is left open so it takes what the others do not.
        var regions = new DockSplit(Orientation.Vertical, new DockNode[]
        {
            Region(ScriptControlsKey, "Script controls", 180, Controls(view, save)),
            Region(CommandTableKey, "Command table", 320,
                Component<CommandTable, CommandTableProps>(
                    new CommandTableProps(view.Script, view.Text, view.Status))),
            Region(ScriptTextKey, "Script text", null,
                Component<ScriptTextEditor, ScriptTextEditorProps>(
                    new ScriptTextEditorProps(view.Text, view.Status, view.TextChanged))),
        });

        // A docked pane body is content-sized unless it is told to grow — the wrapper is
        // what makes the three regions fill the pane instead of collapsing to their
        // desired height at the top of it.
        return FlexColumn(
            CommandHost([save],
                    new DockManager { Layout = regions }.Flex(grow: 1, basis: 0))
                .Flex(grow: 1, basis: 0)
        ).Flex(grow: 1);
    }

    const string ScriptControlsKey = "script:controls";
    const string CommandTableKey = "script:table";
    const string ScriptTextKey = "script:text";

    /// The controls region: Save, whether the text has been written back, and what the script is.
    /// The name, the trigger and the restart policy come next — which is what the note says.
    ///
    /// What the script *is* comes from the text on screen, not from the file behind it: the pane is
    /// about what is being edited, and a line saying "not a script the mod would run" above a text
    /// region that reads it perfectly is a contradiction the user has no way to resolve.
    static Element Controls(ScriptPanelView view, Command save)
    {
        var summary = view.Status.Document is { } document
            ? $"{ScriptTextStatus.LastFrame(document)} frames — name, trigger and restart policy come next"
            : "Not a script the mod would run — the text region below says why";

        return FlexColumn(
            HStack(8,
                Button(save),
                Caption(view.Dirty ? "unsaved changes" : "saved")),
            Caption(summary).TextWrapping(TextWrapping.Wrap)
        ).FlexPadding(12).Flex(grow: 1);
    }

    /// One region of the body: a bare pane rather than a tab group, because a group always
    /// carries a tab strip and the only thing we want around a region is its splitter.
    ///
    /// Nothing here closes, floats or drags either — the regions are fixed and only the
    /// splitters between them move, which keeps the pane out of the docking states the
    /// shell does not survive (see `Editor.cs` and `README.md`).
    static DockNode Region(string key, string title, double? height, Element body) =>
        new Document
        {
            Title = title,
            Key = key,
            Content = body,
            Height = height,
            CanClose = false,
            CanFloat = false,
            CanMove = false,
            CanDockAsToolWindow = false,
        };
}
