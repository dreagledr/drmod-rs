using System;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;    // Command, StandardCommand
using Microsoft.UI.Reactor.Docking; // DockManager, DockSplit, Document, DockNode
using Microsoft.UI.Xaml.Controls;   // Orientation
using static Microsoft.UI.Reactor.Factories;

sealed record ScriptPanelProps(
    ScriptEntry? Script,
    string Text,
    ScriptTextStatus Status,
    Command SaveCommand,
    ScriptControlsView Controls,
    Action<string> TextChanged);

/// Everything the pane paints, in one record.
///
/// Gathered instead of passed one by one because this is the headless layer's entry point: a test
/// renders <see cref="ScriptPanel.View"/> rather than the component — so the pane takes its status
/// and its run controls as values rather than computing them.
internal sealed record ScriptPanelView(
    ScriptEntry? Script,
    string Text,
    ScriptTextStatus Status,
    Command SaveCommand,
    ScriptControlsView Controls,
    Action<string> TextChanged);

/// Right pane: the one selected script.
///
/// The body is three regions stacked top to bottom — script controls, command table, script text
/// — separated by the docking host's drag-resize splitters. The set of panes is fixed, so the host
/// keeps its own split ratios across renders and the selection only decides what the regions read.
///
/// The text is the file's, and the buffers that edit it belong to the shell — they are what the
/// list's unsaved markers and the Delete question are about, and two panes cannot read the same
/// state out of one of them. The parse of that text is the shell's too: the controls region runs the
/// script the text region shows, so one reading of one text is handed to the whole pane instead of
/// each region making up its own.
sealed class ScriptPanel : Component<ScriptPanelProps>
{
    public override Element Render() => View(new ScriptPanelView(
        Props.Script,
        Props.Text,
        Props.Status,
        Props.SaveCommand,
        Props.Controls,
        Props.TextChanged));

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

        // The Save command carries the Ctrl+S accelerator, and the host below is what registers it
        // for this subtree, so the shortcut reaches the editor while the caret is in it. The same
        // command is the controls region's Save button — one command, so one answer to whether there
        // is anything to write back.
        var save = view.SaveCommand;

        // Orientation.Vertical means the splitters between the children are horizontal
        // bars. The heights are **weights, not DIPs**: the host bootstraps a split's ratios from
        // its children's hints only when every child carries one (`BootstrapRatios`), and normalizes
        // them — so these are the base proportions of the pane, one part of controls to two of table
        // to three of text. A drag of a splitter is what the host keeps from then on.
        var regions = new DockSplit(Orientation.Vertical, new DockNode[]
        {
            Region(ScriptControlsKey, "Script controls", ControlsWeight,
                ScriptControls.View(view.Controls)),
            Region(CommandTableKey, "Command table", TableWeight,
                Component<CommandTable, CommandTableProps>(
                    new CommandTableProps(view.Script, view.Text, view.Status))),
            Region(ScriptTextKey, "Script text", TextWeight,
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

    /// The pane's base proportions: one part of controls, two of command table, three of text (see
    /// the split — these are weights, not DIPs).
    const double ControlsWeight = 1;
    const double TableWeight = 2;
    const double TextWeight = 3;

    /// One region of the body: a bare pane rather than a tab group, because a group always
    /// carries a tab strip and the only thing we want around a region is its splitter.
    ///
    /// Nothing here closes, floats or drags either — the regions are fixed and only the
    /// splitters between them move, which keeps the pane out of the docking states the
    /// shell does not survive (see `Editor.cs` and `README.md`).
    static DockNode Region(string key, string title, double weight, Element body) =>
        new Document
        {
            Title = title,
            Key = key,
            Content = body,
            Height = weight,
            CanClose = false,
            CanFloat = false,
            CanMove = false,
            CanDockAsToolWindow = false,
        };
}
