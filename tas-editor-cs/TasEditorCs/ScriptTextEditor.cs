using System;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;    // Element, Theme
using Microsoft.UI.Reactor.Layout;  // GridSize
using Microsoft.UI.Xaml;            // TextWrapping, VerticalAlignment
using Microsoft.UI.Xaml.Controls;   // ScrollBarVisibility, ScrollViewer
using Microsoft.UI.Xaml.Media;      // FontFamily
using static Microsoft.UI.Reactor.Factories;

sealed record ScriptTextEditorProps(string Text, Action<string> TextChanged);

/// The script text region: the selected script as `.tas` text, with what that text currently
/// says right above it.
///
/// The text is edited in place and read back on every keystroke — the status line is the
/// converter's own answer, either the document's name, command count and last frame or the line
/// the parser refused. Nothing here reaches the command table: the two regions read the same
/// script, and neither writes for the other yet.
///
/// The text box is monospaced and does not wrap on purpose: one line is one frame
/// (`docs/SCRIPT_DSL.md` in the sibling repo), so a wrapped line would read as two.
sealed class ScriptTextEditor : Component<ScriptTextEditorProps>
{
    public override Element Render()
    {
        // Both a memo and a per-keystroke cost: the whole document is read again whenever the
        // text changes, and the answer is what the caption paints. A re-render for another
        // reason — a theme toggle, a selection — reuses it.
        var status = UseMemo(() => ScriptTextStatus.Of(Lines(Props.Text)), Props.Text);

        return View(Props.Text, Props.TextChanged, status);
    }

    /// The text as the format reads it. A WinUI `TextBox` separates its lines with a lone `\r`
    /// and reports that back through `TextChanged`, while the `.tas` format is `\n` — so the
    /// text that reaches the parser is the control's own text with its line separator put back.
    ///
    /// The separator is normalised here, on the way *into* the reader, and not in the change
    /// handler: the draft then keeps exactly the bytes the control reported, which is what keeps
    /// the reconciler from writing the text back on every keystroke (and moving the caret with
    /// it). The stored text is the editor's buffer, not the file — anything written out goes
    /// through the converter, which spells the format's own `\n`.
    internal static string Lines(string text) => text.Replace("\r\n", "\n").Replace('\r', '\n');

    /// The region body. Split out of the component because `Component<TProps>.Props` is
    /// read-only and set by the host, so a headless unit test has no way to render the component
    /// itself — it asserts on this instead.
    ///
    /// A grid with the text in its star row, not a flex column: a `TextBox` hands a flex panel its
    /// *content* height, so `Flex(grow: 1)` wins it a full-height slot that the control then sits
    /// content-sized inside — measured live, 92.67 DIP of a 116.75 slot. A star row is the grid's
    /// own and the box stretches into it, which is how the editor takes the whole region.
    internal static Element View(string text, Action<string> textChanged, ScriptTextStatus status) =>
        Grid(
            columns: [GridSize.Star()],
            rows: [GridSize.Auto, GridSize.Star()],
            children:
            [
                Status(status).Grid(row: 0),
                Text(text, textChanged).Grid(row: 1),
            ])
        .Padding(12)
        .Flex(grow: 1);

    /// The status line: what the text says, or why it says nothing. An error keeps the parser's
    /// own wording — it already names the line or the command — and is painted in the theme's
    /// error color, so a text the mod would answer `400` on never looks like one it accepts.
    internal static Element Status(ScriptTextStatus status) =>
        status.Document is { } document
            ? Caption(Summary(document))
            : Caption(status.Error!).Foreground(Theme.SystemCritical);

    internal static string Summary(ScriptDocument document) =>
        $"{document.Name} · {document.Commands.Count} commands · last frame {ScriptTextStatus.LastFrame(document)}";

    /// The editor itself, pinned to the top of whatever height its row gives it — the default
    /// content alignment centres the lines when the text is shorter than the box.
    ///
    /// The automation name is what keeps a bare text box from being an unnamed form field
    /// (REACTOR_A11Y_003): the caption above it is a status message, not a label.
    internal static Element Text(string text, Action<string> textChanged) =>
        TextBox(text, textChanged)
            .AcceptsReturn()
            .TextWrapping(TextWrapping.NoWrap)
            .IsSpellCheckEnabled(false)
            .FontSize(12)
            .VerticalContentAlignment(VerticalAlignment.Top)
            .AutomationName("Script text")
            .Set(box =>
            {
                box.FontFamily = new FontFamily(Monospace);
                // A `TextBox` keeps both of its scrollbars hidden until told otherwise, which is
                // why a text that scrolls (the wheel works) still looks like it has nowhere to
                // go. The attached properties are the box's own way in — the value lands on the
                // `ScrollViewer` inside its template (measured). The type is spelled out because
                // the bare name is the `ScrollViewer` *factory* in this file.
                Microsoft.UI.Xaml.Controls.ScrollViewer.SetVerticalScrollBarVisibility(
                    box, ScrollBarVisibility.Auto);
                Microsoft.UI.Xaml.Controls.ScrollViewer.SetHorizontalScrollBarVisibility(
                    box, ScrollBarVisibility.Auto);
            });

    /// Set through `.Set` rather than through the `.FontFamily(…)` modifier on purpose: that
    /// modifier resolves the name into a `FontFamily` WinRT object while the element is *built*,
    /// which throws `COMException` in the headless unit layer (measured — the same trap as
    /// `.SemiBold()`). A setter runs against the mounted control instead, which only the app has.
    ///
    /// The first font of the list a machine is likely to have: Cascadia ships with Windows
    /// Terminal and the newer SDKs, Consolas with Windows itself, and the last entry is the
    /// generic fallback WinUI understands.
    const string Monospace = "Cascadia Mono, Consolas, monospace";
}
