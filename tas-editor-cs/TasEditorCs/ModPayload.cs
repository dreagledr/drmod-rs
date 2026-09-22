using System;
using System.Reflection;

/// The two files the editor installs into the game, read out of its own exe.
///
/// They are embedded resources rather than files next to the exe for the same reason the app ships
/// self-contained: `TasEditorCs.exe` is the distribution, and an installer whose payload can go
/// missing beside it is an installer that fails in front of the user. The payload's sources and why
/// it is not committed — `build-mod.ps1` at the solution root.
///
/// ⚠️ There is deliberately **no fallback to `null` or an empty array**. A payload that cannot be
/// read is a build that should never have shipped (`_RequireModPayload` in the csproj), and the
/// honest answer here is to say so: <see cref="Missing"/> carries the resource names, and the panel
/// paints it instead of offering a button whose click can only fail.
internal sealed record ModPayload(byte[] Asi, byte[] Loader)
{
    /// The mod itself, as the ASI loader will load it: `drmod_rs_lib.dll` renamed to `.asi`. The
    /// loader does not care what the file is called — `DllMain` on process attach is the entry point
    /// either way — so this is the same bytes the launcher embeds and extracts.
    internal const string AsiName = "drmod_rs_lib.asi";

    /// The ASI loader, as `d3d9.dll` in the game's root. ⚠️ Win32: the game is a 32-bit process and
    /// never loads a 64-bit `d3d9.dll`.
    internal const string LoaderName = "d3d9.dll";

    const string AsiResource = "Mod.drmod_rs_lib.asi";
    const string LoaderResource = "Mod.d3d9.dll";

    /// Reads the payload out of the running assembly. Returns the reason as a message when either
    /// resource is absent, rather than throwing: the only caller is a render, and a render that
    /// throws takes the window with it.
    internal static (ModPayload? Payload, string? Error) Read()
    {
        var assembly = Assembly.GetExecutingAssembly();
        var asi = Resource(assembly, AsiResource);
        var loader = Resource(assembly, LoaderResource);

        if (asi is null || loader is null)
        {
            var missing = asi is null ? AsiResource : LoaderResource;
            return (null, $"This build carries no mod payload ({missing}). Rebuild: pwsh -File build-mod.ps1");
        }

        return (new ModPayload(asi, loader), null);
    }

    /// One embedded resource, or null when it is not there. A short read is treated as absent: a
    /// truncated payload would install a DLL the game cannot load, and saying "missing" about it is
    /// both true of what the user gets and a message they can act on.
    static byte[]? Resource(Assembly assembly, string name)
    {
        using var stream = assembly.GetManifestResourceStream(name);
        if (stream is null)
        {
            return null;
        }

        var bytes = new byte[stream.Length];
        var read = 0;
        while (read < bytes.Length)
        {
            var got = stream.Read(bytes, read, bytes.Length - read);
            if (got <= 0)
            {
                return null;
            }

            read += got;
        }

        return bytes;
    }
}
