using System;
using System.Collections.Generic;

/// The text region's drafts: what has been typed for a script, or the mock text that script
/// starts from.
///
/// A draft outlives the selection on purpose — switching to another script in the list and back
/// must not throw away what is in the editor — so the pane keeps the drafts in its own state and
/// reads them back through here. Which script is selected stays where it was, in the shell.
internal static class ScriptDrafts
{
    internal static readonly IReadOnlyDictionary<string, string> Empty =
        new Dictionary<string, string>(StringComparer.Ordinal);

    internal static string Resolve(IReadOnlyDictionary<string, string> drafts, ScriptEntry script) =>
        drafts.TryGetValue(script.Id, out var typed) ? typed : MockScriptText.For(script);

    /// The drafts with one script replaced. The dictionary handed in is left alone: it is a
    /// piece of state a render is already holding, and a Reactor state value the setter does not
    /// see as a different instance is a render that does not happen.
    internal static IReadOnlyDictionary<string, string> With(
        IReadOnlyDictionary<string, string> drafts, string id, string text)
    {
        var next = new Dictionary<string, string>(drafts.Count + 1, StringComparer.Ordinal);
        foreach (var pair in drafts)
        {
            next[pair.Key] = pair.Value;
        }

        next[id] = text;
        return next;
    }
}
