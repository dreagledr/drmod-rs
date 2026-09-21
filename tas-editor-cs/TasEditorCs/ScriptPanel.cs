using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using static Microsoft.UI.Reactor.Factories;

sealed record ScriptPanelProps(ScriptEntry? Script);

/// Right pane: information about, and controls for, the one selected script.
///
/// Stub — name and size only. The frame timeline, the per-frame properties strip and the
/// JSON editor are the next passes; this pane exists so the split has something real on
/// the right-hand side.
sealed class ScriptPanel : Component<ScriptPanelProps>
{
    public override Element Render() => View(Props.Script);

    /// The pane body. Split out of the component because `Component<TProps>.Props` is
    /// read-only and set by the host, so a headless unit test has no way to render the
    /// component itself — it asserts on this instead.
    internal static Element View(ScriptEntry? script)
    {
        if (script is null)
        {
            return FlexColumn(Caption("No script selected."))
                .FlexPadding(16)
                .Flex(grow: 1);
        }

        return (FlexColumn(
            Heading(script.Name),
            Caption($"{script.Frames} frames"),
            Border(Caption("Timeline, properties and JSON editor come next."))
                .Padding(12)
                .CornerRadius(4)
                .Background(Theme.CardBackground)
        ) with { RowGap = 12 })
        .FlexPadding(16)
        .Flex(grow: 1);
    }
}
