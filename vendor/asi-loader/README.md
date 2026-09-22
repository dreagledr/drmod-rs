# asi-loader

The ASI loader this repository ships with drmod's ASI package, and which the TAS editor embeds to
install the mod without a self-injecting launcher.

`d3d9.dll` is the **Win32** build of
[Ultimate-ASI-Loader](https://github.com/ThirteenAG/Ultimate-ASI-Loader) — the loader the mod's own
`readme.txt` points end users at. It is checked in as a binary because there is no way to fetch it
reproducibly at build time (GitHub release assets are not immutable), and because CI has to be able
to package an ASI build without network access to a third party.

⚠️ **Both files must be the Win32 (32-bit) build.** Metal Gear Rising: Revengeance is a 32-bit
process; a 64-bit `d3d9.dll` is silently never loaded by it, which looks exactly like "the mod
installed but does nothing".

⚠️ **Do not UPX-pack these files.** `d3d9.dll` is a third party's binary, and the `.asi` payload in
the ASI package is a copy of `drmod_rs_lib.dll` — the same bytes `drmod.exe` embeds. Packing the copy
would give a payload that no longer matches what the launcher extracts.

## Provenance

Version **v9.7.4** (the `Win32-latest` line), asset
`d3d9-Win32.zip` — https://github.com/ThirteenAG/Ultimate-ASI-Loader/releases/download/Win32-latest/d3d9-Win32.zip

| File | SHA512 |
|------|--------|
| `d3d9.dll` | `5F5F1B6FCF70F4DC7DA12C5583576878B2AA46364D211EBD0225D1E5BF350C2A5489AF4954FBD2EF5B2FE167197CCDF3F1F890156E1D4C69510D8062E29F8DBD` |

The hash is the one the release publishes beside the DLL (`d3d9-Win32.SHA512`), and it was checked
against the file before it was committed — `Get-FileHash <file> -Algorithm SHA512`.

⚠️ **The asset to take is in the `Win32-latest` / `x64-latest` tags, not in the versioned release.**
The versioned release (`Ultimate-ASI-Loader.zip`, `v9.7.4`) ships a *multi-format* archive whose only
file is `dinput8.dll` — the same loader, but hooked through DirectInput, which this game would load
just as well and the mod's own readme does not describe. `d3d9.dll` comes from the rolling tag above,
whose asset list is one DLL per proxied library, and whose `Win32` and `x64` variants differ only in
that suffix.

| File | From | License |
|------|------|---------|
| `d3d9.dll` | Ultimate-ASI-Loader v9.7.4, Win32 | MIT — see `LICENSE.txt` |
| `LICENSE.txt` | the project's `license` file | — |

Update by replacing both files from the upstream release, then re-run the checks in
`docs/API.md`'s sibling notes — the editor's `ModInstaller` compares payloads by content, so a new
loader is a new install, never a silent overwrite of what a user already has.
