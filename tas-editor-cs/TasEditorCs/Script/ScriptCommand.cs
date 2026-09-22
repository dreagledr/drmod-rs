using System;
using System.Linq;
using System.Text.Json.Serialization;

/// One `commands[]` entry: its inputs are held from frame `t` for `duration`
/// frames (`docs/API.md` §4.1).
///
/// The frame is a **simulation tick** — the mod feeds the inputs from its
/// `updateInputUnit` detour, so timings do not drift with the frame rate; only in
/// menus and loading (where no tick runs) does a script frame mean a drawn frame.
[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
internal sealed record ScriptCommand
{
    /// Zero is a real frame number and one a real duration, so these two are
    /// written even at their defaults — everything else unset stays out of the
    /// JSON (`ScriptJsonContext`).
    [JsonPropertyName("t")]
    [JsonIgnore(Condition = JsonIgnoreCondition.Never)]
    public required uint T { get; init; }

    [JsonPropertyName("duration")]
    [JsonIgnore(Condition = JsonIgnoreCondition.Never)]
    public required uint Duration { get; init; }

    [JsonPropertyName("input")]
    public required ScriptInput Input { get; init; }

    /// The command fires on the first tick its enemy condition holds, with `t` as
    /// the fallback — the adaptive variant used for parrying an enemy jump, whose
    /// timing drifts between runs.
    ///
    /// Only JSON carries it: the DSL has no spelling for a condition and the
    /// command table has no column for one.
    [JsonPropertyName("when_enemy")]
    public EnemyCondition? WhenEnemy { get; init; }
}

/// The `input` object: 26 booleans — the flags of the script format, plus the four movement
/// directions the DSL carries as the stick — the two sticks and the two raw key codes.
///
/// Names and meaning come from the mod's `ScriptInput` (`docs/API.md` §4.2), and
/// `uint` mirrors its unsigned fields, so a typo such as `-1` fails on read
/// instead of reaching the game.
[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
internal sealed record ScriptInput
{
    /// Whether any input is set at all. The mod rejects an empty `input`
    /// (`parse_script`), so the editor has to as well.
    internal bool IsEmpty =>
        Camera is null
        && LeftStick is null
        && RawKey is null
        && DikKey is null
        && !Booleans.Any(column => column.Get(this));

    /// The 26 boolean inputs in <see cref="CommandKeys"/> order: the column bit,
    /// the DSL token and the JSON key are the same list, so a column added to the
    /// table cannot silently miss the converter (the tests assert the order).
    internal static readonly (Func<ScriptInput, bool> Get, Func<ScriptInput, ScriptInput> Set)[] Booleans =
    [
        (input => input.Forward, input => input with { Forward = true }),
        (input => input.Backward, input => input with { Backward = true }),
        (input => input.Left, input => input with { Left = true }),
        (input => input.Right, input => input with { Right = true }),
        (input => input.Jump, input => input with { Jump = true }),
        (input => input.LightAttack, input => input with { LightAttack = true }),
        (input => input.HeavyAttack, input => input with { HeavyAttack = true }),
        (input => input.Ripper, input => input with { Ripper = true }),
        (input => input.Blade, input => input with { Blade = true }),
        (input => input.NinjaRun, input => input with { NinjaRun = true }),
        (input => input.Walk, input => input with { Walk = true }),
        (input => input.Dodge, input => input with { Dodge = true }),
        (input => input.LockOn, input => input with { LockOn = true }),
        (input => input.Subweapon, input => input with { Subweapon = true }),
        (input => input.Item, input => input with { Item = true }),
        (input => input.ArMode, input => input with { ArMode = true }),
        (input => input.WeaponSelect, input => input with { WeaponSelect = true }),
        (input => input.Codec, input => input with { Codec = true }),
        (input => input.Zandatsu, input => input with { Zandatsu = true }),
        (input => input.CameraReset, input => input with { CameraReset = true }),
        (input => input.Pause, input => input with { Pause = true }),
        (input => input.Confirm, input => input with { Confirm = true }),
        (input => input.MenuUp, input => input with { MenuUp = true }),
        (input => input.MenuDown, input => input with { MenuDown = true }),
        (input => input.MenuLeft, input => input with { MenuLeft = true }),
        (input => input.MenuRight, input => input with { MenuRight = true }),
    ];

    [JsonPropertyName("forward")]
    public bool Forward { get; init; }

    [JsonPropertyName("backward")]
    public bool Backward { get; init; }

    [JsonPropertyName("left")]
    public bool Left { get; init; }

    [JsonPropertyName("right")]
    public bool Right { get; init; }

    [JsonPropertyName("jump")]
    public bool Jump { get; init; }

    [JsonPropertyName("light_attack")]
    public bool LightAttack { get; init; }

    [JsonPropertyName("heavy_attack")]
    public bool HeavyAttack { get; init; }

    /// Camera turn of the command, fed as the right stick each frame
    /// (`[dx, dy]` mouse deltas — hundreds to thousands; the game ignores Y below
    /// ~500, so use ≥1000).
    [JsonPropertyName("camera")]
    public float[]? Camera { get; init; }

    /// Ripper mode: a keybind front on the command's first frame; `duration` is
    /// ignored — the flag lives one game tick.
    [JsonPropertyName("ripper")]
    public bool Ripper { get; init; }

    [JsonPropertyName("blade")]
    public bool Blade { get; init; }

    [JsonPropertyName("ninja_run")]
    public bool NinjaRun { get; init; }

    /// Walking: the game encodes it by stick magnitude, so this halves the
    /// implied stick instead of pressing anything.
    [JsonPropertyName("walk")]
    public bool Walk { get; init; }

    [JsonPropertyName("dodge")]
    public bool Dodge { get; init; }

    [JsonPropertyName("lock_on")]
    public bool LockOn { get; init; }

    [JsonPropertyName("subweapon")]
    public bool Subweapon { get; init; }

    [JsonPropertyName("item")]
    public bool Item { get; init; }

    [JsonPropertyName("ar_mode")]
    public bool ArMode { get; init; }

    [JsonPropertyName("weapon_select")]
    public bool WeaponSelect { get; init; }

    [JsonPropertyName("codec")]
    public bool Codec { get; init; }

    [JsonPropertyName("zandatsu")]
    public bool Zandatsu { get; init; }

    [JsonPropertyName("camera_reset")]
    public bool CameraReset { get; init; }

    [JsonPropertyName("pause")]
    public bool Pause { get; init; }

    [JsonPropertyName("confirm")]
    public bool Confirm { get; init; }

    [JsonPropertyName("menu_up")]
    public bool MenuUp { get; init; }

    [JsonPropertyName("menu_down")]
    public bool MenuDown { get; init; }

    [JsonPropertyName("menu_left")]
    public bool MenuLeft { get; init; }

    [JsonPropertyName("menu_right")]
    public bool MenuRight { get; init; }

    /// Raw game key code, delivered through the `isKeyDown`/`isKeyPressed`
    /// detours — the channel the pause menu reads (it does not tick the input
    /// unit, so an override never reaches it). Example: `139` (0x8B).
    [JsonPropertyName("raw_key")]
    public uint? RawKey { get; init; }

    /// Raw DirectInput DIK code, mixed into the device state after polling; the
    /// game maps DIK to its own codes. Example: `208` (0xD0, arrow down).
    [JsonPropertyName("dik_key")]
    public uint? DikKey { get; init; }

    /// An explicit stick position, overriding the one implied by the movement
    /// flags: `forward`/`backward`/`left`/`right` add
    /// `(0,-1000)`/`(0,1000)`/`(-1000,0)`/`(1000,0)`.
    [JsonPropertyName("left_stick")]
    public float[]? LeftStick { get; init; }
}

/// `when_enemy`: the enemy state that fires a command (`docs/API.md` §4.1).
///
/// Every bound is optional and a missing one means "no limit" — the mod keeps the
/// same meaning with `i32::MAX`/`f32::MAX` sentinels, but a nullable bound is what
/// those sentinels say, and `null` is not written to JSON.
[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
internal sealed record EnemyCondition
{
    /// Enemy animations (`Behavior + 0x618`) the command may fire in. Empty means
    /// any; known ids: 19 lunge, 65545 jump, 24 hit.
    [JsonPropertyName("anim")]
    public int[]? Anim { get; init; }

    /// Enemy animation frame (`+0x8B4`).
    [JsonPropertyName("frame_min")]
    public int? FrameMin { get; init; }

    [JsonPropertyName("frame_max")]
    public int? FrameMax { get; init; }

    /// Distance to the enemy in metres — the hit has to reach.
    [JsonPropertyName("dist_max")]
    public float? DistMax { get; init; }

    /// How far the enemy's **blade** is above the player: the body stays on the
    /// ground, the blade rises in an attack (the jump peaks around 2.97 m), and a
    /// parry only launches upward when the blade is above.
    [JsonPropertyName("blade_dy_min")]
    public float? BladeDyMin { get; init; }

    /// Player height: the hit must be a jump — only that launches — but a low one
    /// (≈0.5–0.7 m), because a high hit sends the launch horizontally.
    [JsonPropertyName("player_y_min")]
    public float? PlayerYMin { get; init; }

    [JsonPropertyName("player_y_max")]
    public float? PlayerYMax { get; init; }

    /// Vertical speed: 0 fires only while falling — a hit on the way up does not
    /// launch.
    [JsonPropertyName("player_vy_max")]
    public float? PlayerVyMax { get; init; }

    /// Fire again every `duration` frames while the condition holds. Measured as
    /// harmful for the launch (a normal hit cancels the enemy's jump), so it is
    /// off by default.
    [JsonPropertyName("repeat")]
    public bool Repeat { get; init; }
}
