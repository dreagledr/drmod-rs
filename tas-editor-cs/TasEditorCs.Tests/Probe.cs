using System;
using System.Threading.Tasks;
using static Microsoft.UI.Reactor.Factories;

/// Temporary probe — deleted before the commit. Prints the exact body the settler sends.
internal static class Probe
{
    internal static Task<string> RunAsync()
    {
        var document = new ScriptDocument
        {
            Name = "close-menu",
            Commands =
            [
                new ScriptCommand { T = 0, Duration = 3, Input = new ScriptInput { Pause = true } },
            ],
        };

        var json = ScriptJson.Write(document);
        var shown = json.Replace("\\", "\\").Replace("{", "{{").Replace("}", "}}");
        return Task.FromResult($"JSON>>>{shown}<<<");
    }
}
