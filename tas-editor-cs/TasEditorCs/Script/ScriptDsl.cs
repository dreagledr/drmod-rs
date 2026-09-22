using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Linq;
using System.Text;

/// The script DSL: one line per frame, the whole file is the script. The format itself is
/// specified in `docs/SCRIPT_DSL.md` (sibling repo).
///
/// Tokens are the console pad's names (`a` jump, `x` light attack, `lt` blade, `du` augment …)
/// plus `ls`/`rs` for the sticks, and **movement lives in the stick**: `ls:<angle>` is a full
/// press on the compass (0 forward, 90 right, 180 back, 270 left), `lsx`/`lsy`/`rsx`/`rsy` give
/// exact axis values, and `wk` halves the left stick. A stick or a direction flag both reach the
/// same place — the stick alone drives the character (`docs/API.md` §10.2) — so a direction flag
/// that a JSON script carries is written out as the stick that flag stands for.
///
/// `Write` is canonical — one line per frame a command touches, tokens in a fixed order, invariant
/// numbers, a one-frame duration left out — which makes `Write(Parse(text))` the same text again;
/// the golden tests rely on that. `Parse` is strict: an unknown token or attribute is an error
/// naming its 1-based line, never a silently skipped word — the same typo protection the mod gets
/// from `deny_unknown_fields`.
internal static class ScriptDsl
{
    /// The extension of a script text in the workspace.
    internal const string Extension = ".tas";

    /// The rules line starts here and is the file's first non-comment line.
    const char RulesMark = '!';

    /// A comment runs to the end of the line, alone or after a rule or a frame.
    const char CommentMark = '#';

    /// The stick tokens: full press by angle, exact value by axis.
    const string LeftStick = "ls";
    const string LeftStickX = "lsx";
    const string LeftStickY = "lsy";
    const string RightStick = "rs";
    const string RightStickX = "rsx";
    const string RightStickY = "rsy";

    /// One axis unit: a full press reaches ±1000, the pad's own range — so `ls:<angle>` and
    /// `rs:<angle>` carry the same magnitude, and a faster camera (a mouse delta of a few
    /// thousand) is spelled with `rsx`/`rsy`.
    const double AxisUnit = 1000.0;

    /// Decimals the text keeps: a thousandth of an axis unit for the values, and a thousandth of
    /// a degree for the angles.
    const int AxisDecimals = 3;
    const int AngleDecimals = 3;
    static readonly double AxisStep = Math.Pow(10, -AxisDecimals);

    /// Axis values closer than half of the last decimal count as zero — `sin 0°` is not exactly
    /// 0, and a `-0` would read as a direction.
    static readonly double AxisEpsilon = AxisStep / 2;

    /// How close to ±1000 a stick has to be to be written as an angle instead of two axes.
    static readonly double FullPressTolerance = AxisStep;

    /// One mod input: how to read it, how to set it, and the token that spells it in the text —
    /// `null` for the four movement flags, which have no token because the DSL moves with the
    /// stick.
    ///
    /// The list follows the command table's column order, which is also the order the tokens of a
    /// line are written in. The tokens are not a rename of those columns: a column can share its
    /// token with another (`dr` is the inventory switch the column calls `dl`) and one token can
    /// stand for two columns (`by` is Y+B, the game's Execute prompt).
    static readonly Input[] Inputs =
    [
        new("forward", input => input.Forward, (input, held) => input with { Forward = held }, null),
        new("backward", input => input.Backward, (input, held) => input with { Backward = held }, null),
        new("left", input => input.Left, (input, held) => input with { Left = held }, null),
        new("right", input => input.Right, (input, held) => input with { Right = held }, null),
        new("jump", input => input.Jump, (input, held) => input with { Jump = held }, "a"),
        new("light_attack", input => input.LightAttack, (input, held) => input with { LightAttack = held }, "x"),
        new("heavy_attack", input => input.HeavyAttack, (input, held) => input with { HeavyAttack = held }, "y"),
        new("ripper", input => input.Ripper, (input, held) => input with { Ripper = held }, "lr"),
        new("blade", input => input.Blade, (input, held) => input with { Blade = held }, "lt"),
        new("ninja_run", input => input.NinjaRun, (input, held) => input with { NinjaRun = held }, "rt"),
        new("walk", input => input.Walk, (input, held) => input with { Walk = held }, "wk"),
        new("dodge", input => input.Dodge, (input, held) => input with { Dodge = held }, "ax"),
        new("lock_on", input => input.LockOn, (input, held) => input with { LockOn = held }, "rb"),
        new("subweapon", input => input.Subweapon, (input, held) => input with { Subweapon = held }, "lb"),
        new("item", input => input.Item, (input, held) => input with { Item = held }, "dd"),
        new("ar_mode", input => input.ArMode, (input, held) => input with { ArMode = held }, "du"),
        new("weapon_select", input => input.WeaponSelect, (input, held) => input with { WeaponSelect = held }, "dl"),
        new("codec", input => input.Codec, (input, held) => input with { Codec = held }, "cd"),
        new("zandatsu", input => input.Zandatsu, (input, held) => input with { Zandatsu = held }, "b"),
        new("camera_reset", input => input.CameraReset, (input, held) => input with { CameraReset = held }, "r"),
        new("pause", input => input.Pause, (input, held) => input with { Pause = held }, "esc"),
        new("confirm", input => input.Confirm, (input, held) => input with { Confirm = held }, "ok"),
        new("menu_up", input => input.MenuUp, (input, held) => input with { MenuUp = held }, "mu"),
        new("menu_down", input => input.MenuDown, (input, held) => input with { MenuDown = held }, "md"),
        new("menu_left", input => input.MenuLeft, (input, held) => input with { MenuLeft = held }, "ml"),
        new("menu_right", input => input.MenuRight, (input, held) => input with { MenuRight = held }, "mr"),
    ];

    /// One input of the format with the token that spells it — the reading side of `Inputs`.
    readonly record struct Input(
        string Key,
        Func<ScriptInput, bool> Get,
        Func<ScriptInput, bool, ScriptInput> Set,
        string? Token);

    static readonly int Heavy = IndexOf("heavy_attack");
    static readonly int Zandatsu = IndexOf("zandatsu");

    /// What each token sets, and which inputs it touches — the second half is what makes
    /// "this input is already on this line" checkable when one token stands for two inputs.
    static readonly Dictionary<string, (Func<ScriptInput, ScriptInput> Set, int[] Touched)> Flags =
        BuildFlags();

    static Dictionary<string, (Func<ScriptInput, ScriptInput> Set, int[] Touched)> BuildFlags()
    {
        var flags = new Dictionary<string, (Func<ScriptInput, ScriptInput>, int[])>(
            StringComparer.OrdinalIgnoreCase);
        for (var index = 0; index < Inputs.Length; index++)
        {
            if (Inputs[index].Token is { } token)
            {
                var captured = index;
                flags[token] = (input => Inputs[captured].Set(input, true), [captured]);
            }
        }

        // Y+B is the Execute prompt: one token, two inputs, same frame.
        flags["by"] = (input => input with { HeavyAttack = true, Zandatsu = true }, [Heavy, Zandatsu]);

        // The inventory switch is a single flag in the mod, so the D-pad's other direction spells
        // the same input — `dl` is the canonical one, `dr` is accepted.
        flags["dr"] = flags["dl"];
        return flags;
    }

    static int IndexOf(string key) =>
        Array.FindIndex(Inputs, input => input.Key == key);

    /// Every token a frame line accepts, with the `input` key it sets — the reading side of the
    /// token table, in the order the writer spells them: the inputs in column order, then the two
    /// compounds (`by` is Y+B on one line, `dr` the D-pad right the inventory switch shares with
    /// `dl`).
    ///
    /// The editor's command list (<see cref="ScriptCommands"/>) attaches its help to these rather
    /// than spelling the tokens a second time, so a token cannot reach the text without a help
    /// line, and the help cannot name a token the parser would refuse. The sticks are not here —
    /// they are not a flag of <see cref="ScriptInput"/> but the shape of a whole command — so
    /// <see cref="ScriptCommands"/> carries them itself.
    internal static readonly IReadOnlyList<(string Token, string Key)> Vocabulary = BuildVocabulary();

    static IReadOnlyList<(string Token, string Key)> BuildVocabulary()
    {
        var vocabulary = new List<(string Token, string Key)>();
        foreach (var input in Inputs)
        {
            if (input.Token is { } token)
            {
                vocabulary.Add((token, input.Key));
            }
        }

        vocabulary.Add(("by", $"{Inputs[Heavy].Key} + {Inputs[Zandatsu].Key}"));
        vocabulary.Add(("dr", Inputs[IndexOf("weapon_select")].Key));
        return vocabulary;
    }

    // ── writing ──────────────────────────────────────────────────────────────

    /// The canonical text of a document. Ends with a newline.
    ///
    /// Commands the DSL cannot say — `raw_key`, `dik_key`, `when_enemy` — are an error rather than
    /// a silent loss.
    internal static string Write(ScriptDocument document)
    {
        ScriptJson.Validate(document);

        var text = new StringBuilder();
        var rules = Rules(document);
        if (rules.Count > 0)
        {
            text.Append(RulesMark).Append(' ').Append(string.Join(' ', rules)).Append('\n');
        }

        foreach (var (frame, tokens) in Frames(document))
        {
            text.Append(frame.ToString(CultureInfo.InvariantCulture));
            foreach (var token in tokens)
            {
                text.Append(' ').Append(token);
            }

            text.Append('\n');
        }

        return text.ToString();
    }

    /// The rules line's attributes: name, trigger, restart — every field of the
    /// `POST /script/run` body that is not a command.
    static List<string> Rules(ScriptDocument document)
    {
        var rules = new List<string>();
        if (document.Name != ScriptDocument.DefaultName)
        {
            // Whitespace separates the attributes and `#` starts a comment, so such a name has no
            // text spelling at all — refusing beats writing a line the parser reads back as
            // something else.
            if (document.Name.Any(char.IsWhiteSpace) || document.Name.Contains('#'))
            {
                throw new ScriptFormatException(
                    $"name '{document.Name}': the text format cannot carry whitespace or '#' in a name");
            }

            rules.Add($"name={document.Name}");
        }

        if (document.Trigger?.Pos is { } position)
        {
            rules.Add($"trig=pos:{Number(position[0])},{Number(position[1])},{Number(position[2])}");
        }

        if (document.Trigger?.Ticks is { } ticks)
        {
            rules.Add($"trig=ticks:{ticks.ToString(CultureInfo.InvariantCulture)}");
        }

        if (document.Restart is { } restart)
        {
            rules.Add(RestartToken(restart));
        }

        return rules;
    }

    /// `restart`, or `restart:ups=2,downs=1` with the parameters that differ from the mod's
    /// defaults, spelled out in the JSON's own field names. A parameter the JSON leaves out
    /// (`null`) means the mod's default, so it is not written either.
    static string RestartToken(RestartPolicy restart)
    {
        var defaults = new RestartPolicy();
        var parameters = new List<string>();
        Add("ups", restart.Ups, defaults.Ups);
        Add("downs", restart.Downs, defaults.Downs);
        Add("hold", restart.Hold, defaults.Hold);
        Add("open_gap", restart.OpenGap, defaults.OpenGap);
        Add("gap", restart.Gap, defaults.Gap);
        Add("confirms", restart.Confirms, defaults.Confirms);
        Add("confirm_gap", restart.ConfirmGap, defaults.ConfirmGap);
        Add("tail", restart.Tail, defaults.Tail);

        return parameters.Count == 0 ? "restart" : $"restart:{string.Join(',', parameters)}";

        void Add(string name, uint? value, uint? fallback)
        {
            if (value is { } frames && frames != fallback)
            {
                parameters.Add($"{name}={frames.ToString(CultureInfo.InvariantCulture)}");
            }
        }
    }

    /// One line per frame a command touches, ascending.
    static IEnumerable<(uint Frame, List<string> Tokens)> Frames(ScriptDocument document)
    {
        var byFrame = new SortedDictionary<uint, List<ScriptCommand>>();
        for (var index = 0; index < document.Commands.Count; index++)
        {
            var command = document.Commands[index];
            Unsupported(command, index);
            if (!byFrame.TryGetValue(command.T, out var commands))
            {
                commands = [];
                byFrame[command.T] = commands;
            }

            commands.Add(command);
        }

        foreach (var (frame, commands) in byFrame)
        {
            yield return (frame, Tokens(frame, commands));
        }
    }

    /// The tokens of one frame, in canonical order: the sticks first (`ls`, then `rs`), then the
    /// inputs in column order.
    ///
    /// A boolean held by several commands of the frame becomes the longest of their durations: the
    /// mod ORs the bits of every active command, so the run of frames is the union — the same
    /// input, said shorter. A stick is the last command to set it, as the mod assigns it; two
    /// different values on one frame cannot be said by one line, and that is an error.
    static List<string> Tokens(uint frame, List<ScriptCommand> commands)
    {
        var tokens = new List<string>();
        double[]? left = null;
        double[]? right = null;
        var leftFrames = 1u;
        var rightFrames = 1u;
        var durations = new uint[Inputs.Length];

        foreach (var command in commands)
        {
            if (ScriptFrames.CommandedStick(command.Input) is { } commanded)
            {
                if (left is not null && !Same(left, commanded))
                {
                    throw new ScriptFormatException(
                        $"frame {frame}: two commands move the left stick differently — a line says one stick");
                }

                left = commanded;
                leftFrames = Math.Max(leftFrames, command.Duration);
            }

            if (command.Input.Camera is { } camera)
            {
                double[] axes = [camera[0], camera[1]];
                if (right is not null && !Same(right, axes))
                {
                    throw new ScriptFormatException(
                        $"frame {frame}: two commands move the camera differently — a line says one stick");
                }

                right = axes;
                rightFrames = Math.Max(rightFrames, command.Duration);
            }

            for (var index = 0; index < durations.Length; index++)
            {
                if (Inputs[index].Get(command.Input) && command.Duration > durations[index])
                {
                    durations[index] = command.Duration;
                }
            }
        }

        if (left is not null)
        {
            tokens.AddRange(StickTokens(LeftStick, LeftStickX, LeftStickY, left, leftFrames));
        }

        if (right is not null)
        {
            tokens.AddRange(StickTokens(RightStick, RightStickX, RightStickY, right, rightFrames));
        }

        // Y+B is one token when the two are held for the same stretch — the shape the game's
        // Execute prompt has; held for different stretches they stay two tokens.
        var byFrames = durations[Heavy] > 0 && durations[Heavy] == durations[Zandatsu]
            ? durations[Heavy]
            : 0;

        for (var index = 0; index < Inputs.Length; index++)
        {
            var input = Inputs[index];
            if (input.Token is null || durations[index] == 0)
            {
                continue;
            }

            if (byFrames > 0)
            {
                if (index == Zandatsu)
                {
                    continue;
                }

                if (index == Heavy)
                {
                    tokens.Add("by" + Duration(byFrames));
                    continue;
                }
            }

            tokens.Add(input.Token + Duration(durations[index]));
        }

        return tokens;
    }

    /// One stick as tokens. A full press reads as an angle; anything else reads as exact axis
    /// values, and an axis at zero is left out because a missing axis already means zero.
    static IEnumerable<string> StickTokens(string angle, string x, string y, double[] axes, uint frames)
    {
        var magnitude = Math.Sqrt(axes[0] * axes[0] + axes[1] * axes[1]);
        if (Math.Abs(magnitude - AxisUnit) < FullPressTolerance)
        {
            yield return $"{angle}:{Number(AngleOf(axes))}{Duration(frames)}";
            yield break;
        }

        if (axes[0] == 0 && axes[1] == 0)
        {
            yield return $"{x}:0{Duration(frames)}";
            yield break;
        }

        if (axes[0] != 0)
        {
            yield return $"{x}:{Number(Rounded(axes[0]))}{Duration(frames)}";
        }

        if (axes[1] != 0)
        {
            yield return $"{y}:{Number(Rounded(axes[1]))}{Duration(frames)}";
        }
    }

    /// Refuses what the DSL has no spelling for, naming the command.
    static void Unsupported(ScriptCommand command, int index)
    {
        if (command.WhenEnemy is not null)
        {
            throw new ScriptFormatException(
                $"commands[{index}]: when_enemy has no DSL spelling — the editor shows it as JSON only");
        }

        if (command.Input.RawKey is not null || command.Input.DikKey is not null)
        {
            throw new ScriptFormatException(
                $"commands[{index}]: raw_key/dik_key have no DSL spelling — the editor shows them as JSON only");
        }
    }

    /// `:frames`, left out for a single frame — the DSL's own default.
    static string Duration(uint frames) =>
        frames == 1 ? string.Empty : ":" + frames.ToString(CultureInfo.InvariantCulture);

    /// Axis values as a compass angle: 0 is forward (up), 90 right, 180 back, 270 left — the way
    /// the stick reads on the pad, not the way the table measures the axis.
    static double AngleOf(double[] axes)
    {
        var angle = Math.Atan2(axes[0], -axes[1]) * 180.0 / Math.PI;
        var degrees = Math.Round(angle < 0 ? angle + 360.0 : angle, AngleDecimals);
        return degrees >= 360.0 ? 0.0 : degrees;
    }

    /// A full press at that compass angle, in axis units.
    static float[] FullPress(int number, double angle)
    {
        var radians = angle * Math.PI / 180.0;
        return
        [
            Axis(number, AxisUnit * Math.Sin(radians), null),
            Axis(number, -AxisUnit * Math.Cos(radians), null),
        ];
    }

    /// One axis value, rounded to what the text keeps and snapped away from `-0`.
    ///
    /// The refusal is passed in because the two callers live in different worlds: the whole-angle
    /// form is the writer's — it computes the axes itself, so a non-finite value there is a bug and
    /// `null` means the plain `ScriptFormatException` — while the exact-value form comes from a text
    /// and refuses through the parse's own `Refuse`, which carries the frame.
    static float Axis(int number, double value, Action<string>? refuse)
    {
        if (!double.IsFinite(value))
        {
            if (refuse is null)
            {
                throw new ScriptFormatException($"line {number}: stick angle or value is not finite");
            }

            Refuse<float>(refuse, $"line {number}: stick angle or value is not finite");
        }

        var rounded = Math.Round(value, AxisDecimals);
        return rounded > -AxisEpsilon && rounded < AxisEpsilon ? 0f : (float)rounded;
    }

    static double Rounded(double value) => Math.Round(value, AxisDecimals);

    /// Numbers are written shortest-round-trip in the invariant culture, so a value survives
    /// `Write` → `Parse` unchanged.
    static string Number(double value) => value.ToString(CultureInfo.InvariantCulture);

    /// A `float` is written as a `float`: casting it to `double` first would spell `-24.7f` as
    /// `-24.700000762939453` — the value the machine stores, not the value the user typed.
    static string Number(float value) => value.ToString(CultureInfo.InvariantCulture);

    /// Two stick values are the same when their axes agree to the precision the text keeps.
    static bool Same(double[] a, double[] b) =>
        Math.Abs(a[0] - b[0]) < AxisStep && Math.Abs(a[1] - b[1]) < AxisStep;

    // ── parsing ──────────────────────────────────────────────────────────────

    /// The text as the format reads it: lines separated by `\n`.
    ///
    /// Every reader of a script text goes through here, so one text never reads two ways. Nothing
    /// in the format is sensitive to the separator — a frame line is trimmed, tokens are separated
    /// by spaces — but a control hands its text back with a lone `\r` (measured: a WinUI `TextBox`
    /// writes `\r` between its lines and no `\n` at all, so a save that wrote the text through
    /// would leave a file the parser reads as one long line) and a file edited elsewhere may carry
    /// `\r\n`.
    internal static string Lines(string text) =>
        text.Replace("\r\n", "\n").Replace('\r', '\n');

    /// Reads a script text. Every failure is a <see cref="ScriptFormatException"/> naming its
    /// line; the cross-field limits come from <see cref="ScriptJson.Validate"/> and name the
    /// command instead.
    internal static ScriptDocument Parse(string text)
    {
        var commands = new List<ScriptCommand>();
        string? name = null;
        float[]? triggerPosition = null;
        ulong? triggerTicks = null;
        RestartPolicy? restart = null;
        var rulesSeen = false;

        // The frame of the last line that started with a number: what every refusal reports, so a
        // message carries the `.tas` coordinate and not just the line index. `null` until one is read.
        uint? lastFrame = null;
        void Refuse(string message) => throw new ScriptFormatException(message, lastFrame);

        // The same refusal as a delegate: a local function does not convert to `Action<string>` on
        // its own, and the value readers below take one so that their refusals carry the frame too.
        var refusal = new Action<string>(Refuse);

        var lines = Lines(text).Split('\n');
        for (var index = 0; index < lines.Length; index++)
        {
            var number = index + 1;
            var line = StripComment(lines[index]).Trim();
            if (line.Length == 0)
            {
                continue;
            }

            if (line[0] == RulesMark)
            {
                if (rulesSeen || commands.Count > 0)
                {
                    Refuse($"line {number}: the rules line must come first");
                }

                rulesSeen = true;
                ParseRules(number, line[1..]);
                continue;
            }

            ParseFrame(number, line, commands);
        }

        var document = new ScriptDocument
        {
            Name = name ?? ScriptDocument.DefaultName,
            Trigger = triggerPosition is null && triggerTicks is null
                ? null
                : new ScriptTrigger { Pos = triggerPosition, Ticks = triggerTicks },
            Restart = restart,
            Commands = commands,
        };
        try
        {
            ScriptJson.Validate(document);
        }
        catch (ScriptFormatException refused)
        {
            // The cross-field limits refuse a document rather than a line: there is no line to name,
            // so the frame the whole text had is what they are re-thrown with.
            Refuse(refused.Message);
        }

        return document;

        void ParseRules(int number, string rest)
        {
            foreach (var attribute in rest.Split(' ', StringSplitOptions.RemoveEmptyEntries))
            {
                // An attribute is `key=value` or `key:value` — the colon is what `restart` carries
                // its own `name=value` list behind, so the split stops at whichever comes first.
                var separator = attribute.IndexOfAny(['=', ':']);
                var key = separator < 0 ? attribute : attribute[..separator];
                var raw = separator < 0 ? null : attribute[(separator + 1)..];

                switch (key.ToLowerInvariant())
                {
                    case "name":
                        if (name is not null)
                        {
                            Refuse($"line {number}: name is given twice");
                        }

                        name = Value(refusal, number, key, raw);
                        break;

                    case "trig":
                        ParseTrigger(number, Value(refusal, number, key, raw));
                        break;

                    case "restart":
                        if (restart is not null)
                        {
                            Refuse($"line {number}: restart is given twice");
                        }

                        restart = ParseRestart(number, raw ?? string.Empty);
                        break;

                    default:
                        Refuse(
                            $"line {number}: unknown attribute '{key}' (name, trig, restart)");
                        break;
                }
            }
        }

        void ParseTrigger(int number, string value)
        {
            var separator = value.IndexOf(':');
            var kind = separator < 0 ? value : value[..separator];
            var argument = separator < 0 ? string.Empty : value[(separator + 1)..];
            switch (kind.ToLowerInvariant())
            {
                case "pos":
                    if (triggerPosition is not null)
                    {
                        Refuse($"line {number}: trig=pos is given twice");
                    }

                    var parts = argument.Split(',');
                    if (parts.Length != 3)
                    {
                        Refuse($"line {number}: trig=pos needs three numbers");
                    }

                    triggerPosition =
                    [
                        Float(refusal, number, "trig=pos", parts[0]),
                        Float(refusal, number, "trig=pos", parts[1]),
                        Float(refusal, number, "trig=pos", parts[2]),
                    ];
                    break;

                case "ticks":
                    if (triggerTicks is not null)
                    {
                        Refuse($"line {number}: trig=ticks is given twice");
                    }

                    triggerTicks = ULong(refusal, number, "trig=ticks", argument);
                    break;

                default:
                    Refuse($"line {number}: trig must be pos or ticks, got '{kind}'");
                    break;
            }
        }

        RestartPolicy ParseRestart(int number, string value)
        {
            var policy = new RestartPolicy();
            if (value.Length == 0)
            {
                return policy;
            }

            foreach (var parameter in value.Split(','))
            {
                if (parameter.Length == 0)
                {
                    continue;
                }

                var separator = parameter.IndexOf('=');
                var key = separator < 0 ? parameter : parameter[..separator];
                var raw = separator < 0 ? null : parameter[(separator + 1)..];
                var parsed = UInt(refusal, number, $"restart:{key}", Value(refusal, number, key, raw));
                policy = key.ToLowerInvariant() switch
                {
                    "ups" => policy with { Ups = parsed },
                    "downs" => policy with { Downs = parsed },
                    "hold" => policy with { Hold = parsed },
                    "open_gap" => policy with { OpenGap = parsed },
                    "gap" => policy with { Gap = parsed },
                    "confirms" => policy with { Confirms = parsed },
                    "confirm_gap" => policy with { ConfirmGap = parsed },
                    "tail" => policy with { Tail = parsed },
                    _ => Refuse<RestartPolicy>(refusal,
                        $"line {number}: unknown restart parameter '{key}' " +
                        "(ups, downs, hold, open_gap, gap, confirms, confirm_gap, tail)"),
                };
            }

            return policy;
        }

        void ParseFrame(int number, string line, List<ScriptCommand> into)
        {
            var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            // The frame of the line is read — and recorded — before its tokens are checked, so a bad
            // token on frame 132 reports 132: that is the coordinate an author navigates by.
            var frame = UInt(refusal, number, "frame", parts[0]);
            lastFrame = frame;

            // Tokens of one line become one command per distinct duration: the DSL gives every
            // token its own, and a command holds a single one.
            var groups = new SortedDictionary<uint, ScriptInput>();
            var held = new bool[Inputs.Length];
            var sticks = new float[]?[2];
            var stickFrames = new uint[2];
            var angleSeen = new bool[2];
            var axisSeen = new bool[2, 2];

            for (var index = 1; index < parts.Length; index++)
            {
                var token = parts[index];
                var separator = token.IndexOf(':');
                var key = separator < 0 ? token : token[..separator];
                var arguments = separator < 0 ? [] : token[(separator + 1)..].Split(':');
                var lower = key.ToLowerInvariant();

                if (ParseStick(number, lower, arguments))
                {
                    continue;
                }

                if (!Flags.TryGetValue(lower, out var flag))
                {
                    Refuse(
                        $"line {number}: unknown token '{key}' — a pad button, a stick token or `by`");
                }

                if (arguments.Length > 1)
                {
                    Refuse($"line {number}: '{key}' takes at most one duration");
                }

                foreach (var touched in flag.Touched)
                {
                    if (held[touched])
                    {
                        Refuse(
                            $"line {number}: '{Inputs[touched].Key}' is given twice on this frame");
                    }

                    held[touched] = true;
                }

                Hold(arguments.Length == 0 ? 1 : Duration(refusal, number, key, arguments[0]), flag.Set);
            }

            // The sticks join the line's durations like any other token — a stick held for its own
            // stretch becomes its own command.
            for (var side = 0; side < 2; side++)
            {
                if (sticks[side] is not { } axes)
                {
                    continue;
                }

                var captured = axes;
                Hold(stickFrames[side], side == 0
                    ? input => input with { LeftStick = captured }
                    : input => input with { Camera = captured });
            }

            foreach (var (frames, input) in groups)
            {
                into.Add(new ScriptCommand { T = frame, Duration = frames, Input = input });
            }

            void Hold(uint frames, Func<ScriptInput, ScriptInput> apply)
            {
                groups[frames] = apply(groups.TryGetValue(frames, out var current) ? current : new ScriptInput());
            }

            /// The stick tokens: `<name>:<angle>[:<frames>]` is a full press, `<name>x`/`<name>y`
            /// with a value set one axis exactly. The two forms do not mix on one line, and a stick
            /// is set once per line.
            bool ParseStick(int number, string lower, string[] arguments)
            {
                var isLeft = lower is LeftStick or LeftStickX or LeftStickY;
                var isRight = lower is RightStick or RightStickX or RightStickY;
                if (!isLeft && !isRight)
                {
                    return false;
                }

                if (arguments.Length is < 1 or > 2)
                {
                    Refuse(
                        $"line {number}: '{lower}' needs a value and an optional duration");
                }

                var side = isLeft ? 0 : 1;
                var frames = arguments.Length == 2 ? Duration(refusal, number, lower, arguments[1]) : 1;
                stickFrames[side] = frames;

                if (lower is LeftStick or RightStick)
                {
                    if (angleSeen[side])
                    {
                        Refuse($"line {number}: '{lower}' is given twice");
                    }

                    if (axisSeen[side, 0] || axisSeen[side, 1])
                    {
                        Refuse(
                            $"line {number}: '{lower}' and an exact stick value on the same line say two different sticks");
                    }

                    angleSeen[side] = true;
                    sticks[side] = FullPress(number, Float(refusal, number, lower, arguments[0]));
                    return true;
                }

                if (angleSeen[side])
                {
                    Refuse(
                        $"line {number}: '{lower}' and an angle on the same line say two different sticks");
                }

                var axis = lower is LeftStickX or RightStickX ? 0 : 1;
                if (axisSeen[side, axis])
                {
                    Refuse($"line {number}: '{lower}' is given twice");
                }

                axisSeen[side, axis] = true;
                var axes = sticks[side] ?? [0f, 0f];
                axes[axis] = Axis(number, Float(refusal, number, lower, arguments[0]), refusal);
                sticks[side] = axes;
                return true;
            }
        }
    }

    /// The value readers below refuse a bad number through the parse's own `Refuse`, so their
    /// message carries the frame as well as the line. They are static and `Refuse` is local to
    /// `Parse`, so the refusal is passed in as a delegate — a local function does not convert to
    /// `Action<string>` on its own, hence the explicit `new`.
    static string Value(Action<string> refuse, int number, string key, string? value) =>
        value ?? Refuse<string>(refuse, $"line {number}: '{key}' needs a value");

    static uint Duration(Action<string> refuse, int number, string key, string value)
    {
        var frames = UInt(refuse, number, key, value);
        return frames == 0
            ? Refuse<uint>(refuse, $"line {number}: '{key}' duration must be >= 1")
            : frames;
    }

    static uint UInt(Action<string> refuse, int number, string field, string value) =>
        uint.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var parsed)
            ? parsed
            : Refuse<uint>(refuse, $"line {number}: {field} '{value}' is not a number");

    static ulong ULong(Action<string> refuse, int number, string field, string value) =>
        ulong.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var parsed)
            ? parsed
            : Refuse<ulong>(refuse, $"line {number}: {field} '{value}' is not a number");

    static float Float(Action<string> refuse, int number, string field, string value) =>
        float.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed)
        && float.IsFinite(parsed)
            ? parsed
            : Refuse<float>(refuse, $"line {number}: {field} '{value}' is not a number");

    /// `Refuse` never returns — it throws — but the compiler needs a value of the reader's own type
    /// where one is used inside an expression, and this beats every call site casting.
    static T Refuse<T>(Action<string> refuse, string message)
    {
        refuse(message);
        throw new UnreachableException("Refuse always throws");
    }

    static string StripComment(string line)
    {
        var comment = line.IndexOf(CommentMark);
        return comment < 0 ? line : line[..comment];
    }
}
