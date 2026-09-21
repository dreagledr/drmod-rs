using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Text;

/// The text a script opens with while the on-disk workspace is not wired up — the text region's
/// mock, the way <see cref="CommandRows"/> is the command table's.
///
/// Two scripts carry a hand-written text: short enough to read as the format's own example. The
/// 20 000-frame one is generated instead — a literal of that size cannot be written by hand, and
/// expanding the table's own mock frames through the converters gives a text that agrees with the
/// grid without either of the two reading the other.
///
/// ⚠️ The generated text stops at <see cref="ScriptJson.MaxFrames"/>: the mod refuses a command
/// whose `t + duration` passes 3600, so a text built from all 20 000 frames would not parse at
/// all, and the region would open on an error it can never clear. The clip is spelled out in the
/// text itself, as a comment, so the editor's status line is not the only place the difference
/// between the list's frame count and the text shows.
internal static class MockScriptText
{
    /// Generated once per script id — the 20 000-frame mock is a collapse of the table's whole
    /// frame list, and a re-render asks for it again. The id keys the cache because it is the
    /// script's identity (the frames are seeded from it too); the *name* picks which text is
    /// written, because the name is what the text itself carries — the rules line, and the
    /// `.tas` file it will become.
    static readonly ConcurrentDictionary<string, Lazy<string>> Texts = new(StringComparer.Ordinal);

    internal static string For(ScriptEntry script) =>
        Texts.GetOrAdd(script.Id, _ => new Lazy<string>(() => Build(script))).Value;

    static string Build(ScriptEntry script) => script.Name switch
    {
        "blade-run" => BladeRun,
        "barrier-flight" => BarrierFlight,
        _ => Generated(script),
    };

    /// A blade-mode run: hold the blade through the approach, one light cut, then turn away.
    /// The frames stay inside the entry's own 42 so the list and the text agree.
    const string BladeRun =
        """
        ! name=blade-run trig=ticks:0
        0 ls:0:20 lt:20
        20 ls:0:6 x:3
        26 ls:0:8 lt:10
        34 ls:270:8
        """;

    /// The R-03 barrier flight: ripper mode, a run-up, the launch, blade mode, the heavy hit and
    /// the zandatsu — then the pause menu handles the landing.
    const string BarrierFlight =
        """
        ! name=barrier-flight trig=pos:0.5,-1,57 restart
        0 lr
        2 ls:0:40 rt:40
        42 ls:0:2 a:2
        44 ls:0:60 lt:60
        104 y:6
        110 ls:0:30 b:8
        140 ls:0:20
        160 ok:2
        162 ls:0:16
        178 ls:180:20
        """;

    /// The mock text of a script too long to write out: its own table frames, collapsed to
    /// commands and written by the DSL. The frames past the mod's limit are dropped, and the note
    /// that says so is the last line.
    static string Generated(ScriptEntry script)
    {
        var frames = CommandRows.For(script);
        var limit = Math.Min(frames.Count, (int)ScriptJson.MaxFrames);
        var document = new ScriptDocument
        {
            Name = script.Name,
            Commands = ScriptFrames.Collapse(frames.Take(limit).ToArray()),
        };

        var text = new StringBuilder(ScriptDsl.Write(document));
        if (limit < frames.Count)
        {
            text.Append(ClipNote).Append('\n');
        }

        return text.ToString();
    }

    /// Says out loud what the text leaves out — the frames the mod would refuse.
    const string ClipNote = "# mock: the table's 20 000 frames, clipped to the mod's 3600-frame limit";
}
