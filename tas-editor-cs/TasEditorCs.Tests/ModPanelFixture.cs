using System;

namespace TasEditorCs.Tests;

/// A ready-made `ModPanelView` for the tests that exercise *other* things — the workspace pane carries
/// the install above its list, so every test of that pane needs one, and its twelve parameters have
/// nothing to do with what those tests are about.
///
/// The defaults are the "nothing to say" state: no game folder, nothing installed, no message. Tests
/// that are about the install build their own view (`ModPanelTests`), because there the values are
/// the subject.
internal static class ModPanelFixture
{
    internal static ModPanelView View(
        string? gameFolder = null,
        ModState state = ModState.NoGameFolder,
        bool loaderPresent = false,
        bool loaderOurs = false,
        bool canRemove = false,
        string? payloadError = null,
        string? message = null,
        bool busy = false) =>
        new(gameFolder, state, loaderPresent, loaderOurs, canRemove, payloadError, message, busy,
            () => { }, () => { }, () => { }, () => { });
}
