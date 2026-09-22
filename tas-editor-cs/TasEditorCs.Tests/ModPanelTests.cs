using System;
using System.Collections.Generic;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Reactor.Controls;

namespace TasEditorCs.Tests;

/// The mod pane's two invariants: the install button is live exactly when a click could work, and
/// every control in the pane can be named.
///
/// ⚠️ Nothing here asserts on the pane's *sentences*. Those are copy, and a test that repeats them
/// only fails when the wording changes — it cannot fail when the behaviour does. What a pane says is
/// checked by looking at the running app (`mur devtools screenshot`); what it *does* is here.
///
/// The install itself is `ModInstallerTests`, against a real folder — where the behaviour is.
public class ModPanelTests
{
    const string Folder = @"C:\games\Metal Gear Rising REVENGEANCE";

    /// Three answers, one rule: the button is live exactly when there is a folder to write into, a
    /// payload to write, and no install already in flight.
    [Theory]
    [InlineData(ModState.NotInstalled, Folder, false, null, true)]
    // `Installed` is live on purpose: the same button re-lays the files after a user deleted one by
    // hand, and its word changes to say so.
    [InlineData(ModState.Installed, Folder, false, null, true)]
    // A different build is the update case, and the button says `Update`.
    [InlineData(ModState.OtherVersion, Folder, false, null, true)]
    // Nowhere to write.
    [InlineData(ModState.NoGameFolder, null, false, null, false)]
    // A broken build: the button would have no payload to lay down.
    [InlineData(ModState.NotInstalled, Folder, false, "no payload", false)]
    // An install already running.
    [InlineData(ModState.NotInstalled, Folder, true, null, false)]
    internal void The_install_is_live_exactly_when_a_click_could_work(
        ModState state,
        string? folder,
        bool busy,
        string? payloadError,
        bool expected)
    {
        var view = Pane(new ModPanelView(
            folder, state, false, false, false, payloadError, null, busy,
            () => { }, () => { }, () => { }, () => { }));

        var applied = Install(view).Modifiers?.IsEnabled;

        Assert.NotNull(applied);
        Assert.Equal(expected, applied.Value);
    }

    /// The uninstall is live exactly when there is something of ours in the folder — and it is the
    /// `CanRemove` the shell read from the disk that decides, not the state word.
    [Theory]
    [InlineData(true, false, true)]
    [InlineData(false, false, false)]
    // Busy wins over everything: one install at a time.
    [InlineData(true, true, false)]
    internal void The_uninstall_is_live_exactly_when_there_is_something_to_remove(
        bool canRemove,
        bool busy,
        bool expected)
    {
        var view = Pane(new ModPanelView(
            Folder, ModState.Installed, true, true, canRemove, null, null, busy,
            () => { }, () => { }, () => { }, () => { }));

        var applied = Uninstall(view).Modifiers?.IsEnabled;

        Assert.NotNull(applied);
        Assert.Equal(expected, applied.Value);
    }

    /// Every button in the pane can be named — a non-empty `Label`, or an `AutomationName` for one
    /// drawn as an icon alone. This is the regression the pane is one commit away from at all times:
    /// a new icon-only button is invisible to a screen reader and no amount of reading the code shows
    /// it (`REACTOR_A11Y_001` flags the factory calls, and this catches what slips past it).
    [Fact]
    internal void Every_button_in_the_pane_can_be_named()
    {
        var panes = new[]
        {
            new ModPanelView(Folder, ModState.NotInstalled, false, false, false, null, null, false,
                () => { }, () => { }, () => { }, () => { }),
            new ModPanelView(Folder, ModState.Installed, true, true, true, null, "installed", false,
                () => { }, () => { }, () => { }, () => { }),
            new ModPanelView(null, ModState.NoGameFolder, false, false, false, null, null, true,
                () => { }, () => { }, () => { }, () => { }),
        };

        foreach (var pane in panes)
        {
            var buttons = Buttons(Pane(pane));

            Assert.NotEmpty(buttons);
            Assert.All(buttons, button => Assert.True(
                button.Label.Length > 0 || !string.IsNullOrEmpty(button.Modifiers?.AutomationName),
                $"A button with no label and no AutomationName: {button}"));
        }
    }

    static ButtonElement Uninstall(FlexElement view) => Find(view, "Uninstall");

    /// A button of the pane by the word it carries. Buttons are found by their label rather than by
    /// an index: the pane's layout is exactly what a test is not about.
    ///
    /// ⚠️ Named `Find`, not `Button`: the analyzer treats a method called `Button` returning a
    /// `ButtonElement` as a use of the factory and reports `REACTOR_A11Y_001` at the call site, which
    /// is a test helper and not a control anyone renders.
    static ButtonElement Find(FlexElement view, string label)
    {
        foreach (var button in Buttons(view))
        {
            if (button.Label == label)
            {
                return button;
            }
        }

        Assert.Fail($"No button labelled '{label}' in the pane");
        throw new InvalidOperationException(); // unreachable — Assert.Fail does not return
    }

    /// The pane's rendered body. `View` answers an `Element`; the pane's own shape is a flex column.
    static FlexElement Pane(ModPanelView view) =>
        Assert.IsType<FlexElement>(ModPanel.View(view));

    static ButtonElement Install(FlexElement view) =>
        Assert.IsType<ButtonElement>(
            Assert.IsType<StackElement>(view.Children[3]).Children[0]);

    /// Every button in the tree, in layout order.
    ///
    /// ⚠️ The walk has to know both container shapes: a `VStack`/`HStack` is a `StackElement` and the
    /// pane's own root is a `FlexElement`, and the two do **not** share a base with `Children` — a walk
    /// that knows only one of them answers "no buttons" for a pane that is full of them.
    static IEnumerable<ButtonElement> Buttons(Element element)
    {
        switch (element)
        {
            case ButtonElement button:
                yield return button;
                break;
            case StackElement stack:
                foreach (var child in stack.Children)
                {
                    foreach (var found in Buttons(child))
                    {
                        yield return found;
                    }
                }

                break;
            case FlexElement flex:
                foreach (var child in flex.Children)
                {
                    foreach (var found in Buttons(child))
                    {
                        yield return found;
                    }
                }

                break;
        }
    }
}
