using System;
using System.Globalization;
using System.Threading;
using System.Threading.Tasks;

/// The frame period the run is pinned to: the game's own default tick, the cap lifted, or a limit of
/// the editor's own.
internal enum FpsCapMode
{
    /// The cap the game keeps (50 in the pacer's 3 ms units = 60 FPS in gameplay, 30 in menus).
    Default,

    /// The cap lifted: with a fixed tick every frame is exactly 1/60 s of simulation, so the game
    /// runs faster than real time and a run is over sooner (`docs/API.md` §3.13).
    Unlimited,

    /// A limit of the editor's own.
    Custom,
}

/// How a run is configured: the four levers the python tools set before a script goes out
/// (`drmod_api.fixed_dt` / `fps_cap` / `rng`, `r03_baseline.run_once`).
///
/// These are **not part of the script**. They are the mod's state for one run — the text format says
/// so itself (`docs/SCRIPT_DSL.md` §6), which is why they live in the editor's own settings and
/// never in a `.tas` file: the same script can be run pinned or unpinned, and the file must not be
/// rewritten to say which.
///
/// The defaults are the reproducible set the demo uses: the tick fixed at 1/60, the cap left as the
/// game keeps it, and the AI's decisions pinned to a frozen seed. Headless is off — it takes the
/// picture away, and that is not something to do behind the author's back.
///
/// ⚠️ A run applies these; nothing here restores them afterwards. They are session state by design
/// (the demo does the same): lifting the cap is what makes a run fast, and the mod is the one that
/// puts the render and the cap back when a headless run ends.
internal sealed record PlaybackRules(
    bool FixedTick = true,
    FpsCapMode Cap = FpsCapMode.Default,
    uint CustomFps = 60,
    bool PinSeed = true,
    uint Seed = 1,
    bool Headless = false)
{
    internal static readonly PlaybackRules Default = new();

    /// The tick the fixed period stands for — `docs/API.md` §3.8: 1/60 s, the nominal the engine's
    /// own `cSlowRateManager` carries.
    internal const float TickRate = 60f;

    /// The limits the mod enforces on `/fps` and `/rng` (`1..1000`); a value outside them would be
    /// answered `400`, so it is clamped here instead of being argued about.
    internal const uint MinFps = 1;
    internal const uint MaxFps = 1000;

    /// `POST /fps` for these rules — the cap the run keeps. The bodies spell the cap the mod's own way
    /// (`game` / `off`); what the panel calls the modes is its own business.
    internal string CapBody() => Cap switch
    {
        FpsCapMode.Unlimited => ApiJson.Cap("off"),
        FpsCapMode.Custom => ApiJson.Fps(Math.Clamp(CustomFps, MinFps, MaxFps)),
        _ => ApiJson.Cap("game"),
    };

    /// `POST /dt` for these rules.
    internal string DtBody() => ApiJson.Dt(FixedTick);

    /// `POST /rng` for these rules. `freeze` and not `seed` on purpose: a frozen LCG answers every
    /// call as a function of its arguments, so the outcome stops depending on how the AI's threads
    /// interleave (`docs/API.md` §3.10).
    internal string SeedBody() => PinSeed ? ApiJson.Seed("freeze", Seed) : ApiJson.Seed("off");

    /// What a run does to the mod, in the order it has to happen: the tick, the cap, then the seed.
    ///
    /// ⚠️ The seed goes **last**, immediately before the script: the mod freezes the LCG on the
    /// first tick of the next script, so a pin applied before anything else would land on nothing.
    /// An error comes back as a message — the caller paints it and does not start the run.
    internal async Task<string?> ApplyAsync(ModApi api, CancellationToken ct)
    {
        foreach (var (path, body) in new[]
                 {
                     ("/dt", DtBody()),
                     ("/fps", CapBody()),
                     ("/rng", SeedBody()),
                 })
        {
            var answer = await api.PostAsync(path, body, ct);
            if (!answer.Ok)
            {
                return $"{path}: {answer.Message}";
            }
        }

        return null;
    }

    /// The seed as the field spells it, read back as a number: decimal, or `0x`-prefixed hex — the
    /// spelling the reproducibility notes use (`0x55555555` in `docs/API.md` §3.10). False for
    /// anything else, which is how a half-typed seed is told apart from one the run can use.
    internal static bool TrySeed(string? text, out uint seed)
    {
        seed = 0u;
        var value = (text ?? string.Empty).Trim();
        if (value.Length == 0)
        {
            return false;
        }

        var hex = value.StartsWith("0x", StringComparison.OrdinalIgnoreCase);
        var digits = hex ? value[2..] : value;
        return hex
            ? uint.TryParse(digits, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out seed)
            : uint.TryParse(digits, NumberStyles.Integer, CultureInfo.InvariantCulture, out seed);
    }

    /// The rules as one line — what a run would set, for the panel to show before anything is sent.
    internal string Describe() =>
        $"fixed tick {(FixedTick ? "1/60" : "off")}"
        + $" · cap {(Cap switch
        {
            FpsCapMode.Unlimited => "unlimited",
            FpsCapMode.Custom => $"{Math.Clamp(CustomFps, MinFps, MaxFps)} fps",
            _ => "default",
        })}"
        + $" · seed {(PinSeed ? $"freeze {Seed}" : "off")}"
        + $" · headless {(Headless ? "on" : "off")}";
}
