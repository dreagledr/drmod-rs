using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.CompilerServices;

namespace TasEditorCs.Tests;

/// The fixture files shared with the Rust tool (`tools/script_gen` in the sibling
/// repo): JSON generated there from the mod's own DTOs, plus the goldens the
/// editor writes next to them (`.tas` for the DSL, `.expected.json` for the JSON).
///
/// Paths come from the test source file rather than the output directory: the
/// fixtures are read and regenerated in place, so nothing has to be copied on
/// build and a new fixture is picked up without touching the project.
static class ScriptFixtures
{
    /// Set to `1` to rewrite the goldens instead of comparing them — the way to
    /// bless a deliberate format change:
    /// `set TAS_REGEN_GOLDENS=1 && dotnet test TasEditorCs.slnx`.
    internal const string RegenerateVariable = "TAS_REGEN_GOLDENS";

    internal const string JsonExtension = ".json";
    internal const string DslExtension = ".tas";

    internal static bool Regenerating => Environment.GetEnvironmentVariable(RegenerateVariable) == "1";

    /// The `Fixtures` directory of this project.
    internal static string Folder([CallerFilePath] string source = "") =>
        Path.Combine(Path.GetDirectoryName(source)!, "Fixtures");

    /// The suffix an editor golden carries.
    internal static string GoldenSuffix(string extension) => ".expected" + extension;

    /// The golden's name for a fixture: `all_inputs.json` → `all_inputs.expected.json`,
    /// and `all_inputs.expected.tas` for the DSL one.
    internal static string GoldenName(string fixture, string extension) =>
        Path.GetFileNameWithoutExtension(fixture) + GoldenSuffix(extension);

    /// The fixture JSONs the Rust tool writes, by name, in a stable order.
    internal static IReadOnlyList<string> Fixtures() =>
        Directory.GetFiles(Folder(), "*" + JsonExtension)
            .Select(Path.GetFileName)
            .OfType<string>()
            .Where(name => !name.EndsWith(GoldenSuffix(JsonExtension), StringComparison.Ordinal))
            .OrderBy(name => name, StringComparer.Ordinal)
            .ToList();

    /// A file's text with line endings normalised: the files are written with
    /// `\n`, but a checkout may hand them back with `\r\n`.
    internal static string Read(string name) =>
        File.ReadAllText(Path.Combine(Folder(), name)).Replace("\r\n", "\n");

    internal static bool Exists(string name) => File.Exists(Path.Combine(Folder(), name));

    /// Compares against a golden, or writes it when regenerating.
    internal static void ExpectGolden(string name, string actual)
    {
        var path = Path.Combine(Folder(), name);
        if (Regenerating)
        {
            File.WriteAllText(path, actual);
            return;
        }

        Assert.True(File.Exists(path), $"{name} is missing — regenerate with {RegenerateVariable}=1");
        Assert.Equal(File.ReadAllText(path).Replace("\r\n", "\n"), actual);
    }
}
