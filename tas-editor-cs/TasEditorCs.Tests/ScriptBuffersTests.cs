namespace TasEditorCs.Tests;

/// The text the editor holds per script, and what still differs from the file.
///
/// The property under test throughout is what the whole save flow rests on: "unsaved" is a
/// question about the two texts, and the answer has to be insensitive to how their lines are
/// separated — the text box reports `\r`, the file carries `\n`, and files are edited elsewhere too.
public class ScriptBuffersTests
{
    static readonly ScriptEntry BladeRun = Entry("blade-run", "! trig=ticks:0\n0 a\n");
    static readonly ScriptEntry BarrierFlight = Entry("barrier-flight", "! trig=ticks:0\n0 x\n");

    [Fact]
    public void Shows_the_files_own_text_until_something_is_typed() =>
        Assert.Equal(BladeRun.Text, ScriptDsl.Lines(ScriptBuffers.Resolve(ScriptBuffers.Empty, BladeRun)));

    [Fact]
    public void A_script_nobody_typed_in_is_shown_as_the_control_would_hold_it()
    {
        // The file carries `\n`, the text box reports `\r`, and the reconciler compares the two:
        // handed the file's own separator it writes the text back on every render, and every write
        // of `Text` drops the caret to the start. Polling the game twice a second is therefore a
        // caret that jumps to the top of the script twice a second (`View` leaves `buffers` empty
        // until the author types).
        Assert.Equal(BladeRun.Text.Replace("\n", "\r"), ScriptBuffers.Resolve(ScriptBuffers.Empty, BladeRun));
        Assert.Equal("", ScriptBuffers.Boxed(""));
        Assert.Equal("0 a\r1 b\r", ScriptBuffers.Boxed("0 a\n1 b\n"));
    }

    [Fact]
    public void Prefers_what_was_typed_for_that_script()
    {
        var buffers = ScriptBuffers.Typed(ScriptBuffers.Empty, BladeRun, "0 a\n");

        Assert.Equal("0 a\n", ScriptBuffers.Resolve(buffers, BladeRun));
        Assert.True(ScriptBuffers.IsDirty(buffers, BladeRun));
    }

    [Fact]
    public void Leaves_the_scripts_nobody_typed_in_on_their_file()
    {
        var buffers = ScriptBuffers.Typed(ScriptBuffers.Empty, BladeRun, "0 a\n");

        Assert.Equal(BarrierFlight.Text, ScriptDsl.Lines(ScriptBuffers.Resolve(buffers, BarrierFlight)));
        Assert.False(ScriptBuffers.IsDirty(buffers, BarrierFlight));
    }

    [Fact]
    public void A_buffer_is_kept_exactly_as_the_control_reported_it()
    {
        // The text box hands its lines back with a lone `\r`. Storing the file's own `\n` instead
        // would make the reconciler write the text back on the next keystroke — and take the caret
        // with it.
        var typed = "! trig=ticks:0\r0 a\r";

        Assert.Equal(typed, ScriptBuffers.Resolve(ScriptBuffers.Typed(ScriptBuffers.Empty, BladeRun, typed), BladeRun));
    }

    [Fact]
    public void A_text_that_reads_the_same_as_the_file_is_not_a_change()
    {
        // Whichever way the two spell their line breaks: the text box says `\r`, the file says `\n`,
        // and neither is a difference the author can see.
        var typed = BladeRun.Text.Replace("\n", "\r");
        var buffers = ScriptBuffers.Typed(ScriptBuffers.Empty, BladeRun, typed);

        Assert.False(ScriptBuffers.IsDirty(buffers, BladeRun));

        // And a real edit inside the same text is one.
        Assert.True(ScriptBuffers.IsDirty(ScriptBuffers.Typed(buffers, BladeRun, typed + "0 a\r"), BladeRun));
    }

    [Fact]
    public void A_save_leaves_the_buffer_alone()
    {
        // A save rewrites the file, not the editor: the text box still holds the text it reported,
        // and the pane keeps showing it. What changes is that the file now reads the same — so the
        // script is not unsaved any more.
        var typed = BladeRun.Text.Replace("\n", "\r");
        var buffers = ScriptBuffers.Typed(ScriptBuffers.Empty, BladeRun, typed);

        // The listing as it is after the save: the file holds what the buffer reads (`\n` in the
        // file, whatever the caller's text was — `Workspace.Write`).
        var saved = BladeRun with { Text = typed.Replace("\r", "\n") };

        Assert.Equal(typed, ScriptBuffers.Resolve(buffers, saved));
        Assert.False(ScriptBuffers.IsDirty(buffers, saved));
    }

    [Fact]
    public void Dropping_a_buffer_that_is_already_gone_answers_with_the_same_map()
    {
        // Reactor re-renders on a state value it does not see as a different instance, so a delete
        // that has nothing to drop must not allocate a new one.
        var buffers = ScriptBuffers.Typed(ScriptBuffers.Empty, BladeRun, "0 a\n");

        Assert.Same(buffers, ScriptBuffers.Without(buffers, BarrierFlight.Path));
        Assert.Same(ScriptBuffers.Empty, ScriptBuffers.Without(ScriptBuffers.Empty, "nobody"));
    }

    [Fact]
    public void Dropping_one_script_leaves_the_others_alone()
    {
        var before = ScriptBuffers.Typed(ScriptBuffers.Empty, BarrierFlight, "1 b\n");
        var after = ScriptBuffers.Typed(before, BladeRun, "0 a\n");

        // The map handed in comes back untouched: a render is already holding it.
        Assert.Single(before);
        Assert.False(before.ContainsKey(BladeRun.Path));
        Assert.Equal(2, after.Count);
        Assert.Equal("1 b\n", after[BarrierFlight.Path]);

        var deleted = ScriptBuffers.Without(after, BarrierFlight.Path);

        Assert.Equal("0 a\n", deleted[BladeRun.Path]);
        Assert.False(deleted.ContainsKey(BarrierFlight.Path));
    }

    [Fact]
    public void A_rename_takes_the_typed_text_with_it()
    {
        // A buffer is what the editor holds for a *path*, so a rename has to carry the text across —
        // otherwise the renamed file would come back as its own text on disk and work that was never
        // written back would be gone without a word.
        var typed = "! trig=ticks:0\r0 a\r7 x\r";
        var buffers = ScriptBuffers.Typed(ScriptBuffers.Empty, BladeRun, typed);
        var renamed = BarrierFlight with { Name = "blade-run-moved" };

        var moved = ScriptBuffers.Renamed(buffers, BladeRun.Path, renamed.Path);

        Assert.Equal(typed, ScriptBuffers.Resolve(moved, renamed));
        // Still unsaved, and the old path holds nothing: after the rename the workspace has one file.
        Assert.True(ScriptBuffers.IsDirty(moved, renamed));
        Assert.Equal(BladeRun.Text, ScriptDsl.Lines(ScriptBuffers.Resolve(moved, BladeRun)));
    }

    [Fact]
    public void Renaming_a_script_nobody_typed_in_leaves_the_buffers_alone()
    {
        var buffers = ScriptBuffers.Typed(ScriptBuffers.Empty, BarrierFlight, "1 b\n");

        Assert.Same(buffers, ScriptBuffers.Renamed(buffers, BladeRun.Path, "elsewhere"));
        Assert.Same(
            ScriptBuffers.Empty,
            ScriptBuffers.Renamed(ScriptBuffers.Empty, BladeRun.Path, "elsewhere"));
    }

    /// An entry as the workspace would list it: a path that is the identity, the file's text, and
    /// the frame count the text ends on (`ScriptTextStatus.LastFrame`).
    static ScriptEntry Entry(string name, string text)
    {
        var status = ScriptTextStatus.Of(text);
        return new ScriptEntry(
            $@"C:\workspace\{name}.tas",
            name,
            text,
            ScriptTextStatus.LastFrame(status.Document!),
            null);
    }
}
