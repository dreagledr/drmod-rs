using System;
using System.Collections.Generic;
using System.Linq;

namespace TasEditorCs.Tests;

/// The command reference's own list — the third place the format's tokens are spelled (the parser,
/// the command table's column headers, and this list), and the one the user reads. The tests here
/// are what keep the three from drifting: a token the parser accepts always has a line in the
/// reference, and every line offered is a token the parser accepts.
public class ScriptCommandsTests
{
    [Fact]
    public void Every_token_of_the_format_carries_help()
    {
        foreach (var (token, _) in ScriptDsl.Vocabulary)
        {
            Assert.Contains(ScriptCommands.Frame, command => command.Token == token);
        }

        foreach (var stick in ScriptCommands.Sticks)
        {
            Assert.Contains(ScriptCommands.Frame, command => command.Token == stick.Token);
        }

        // None of them is dropped on the way into the list: a token with no help line is left out
        // of the reference rather than listed blank, and this is what refuses that state.
        Assert.Equal(
            ScriptCommands.Frame.Length,
            ScriptDsl.Vocabulary.Count + ScriptCommands.Sticks.Length);
    }

    [Fact]
    public void Every_token_in_the_reference_is_one_the_parser_accepts()
    {
        foreach (var command in ScriptCommands.Frame)
        {
            // A stick token is a whole command with a value, so it is probed with one.
            var line = command.Kind == ScriptCommandKind.Stick
                ? $"0 {command.Token}:0\n"
                : $"0 {command.Token}\n";

            Assert.NotNull(ScriptDsl.Parse(line));
        }
    }

    [Fact]
    public void Every_vocabulary_key_is_a_column_of_the_command_table()
    {
        // A token, the `input` key it sets and the table's column are three names for one thing
        // (`README.md` *Script formats*), so a rename that misses one of them lands here. The two
        // compounds carry the keys of both inputs they stand for.
        foreach (var (_, key) in ScriptDsl.Vocabulary)
        {
            foreach (var part in key.Split(" + "))
            {
                Assert.Contains(CommandKeys.All, column => column.Key == part);
            }
        }
    }

    [Fact]
    public void Every_column_but_the_movement_flags_has_a_token()
    {
        // The four movement flags have a column and no token of their own: the format moves with
        // the stick, and a direction flag from a JSON script is written out as the stick it stands
        // for (`docs/SCRIPT_DSL.md` §3.1).
        var movement = new[] { "forward", "backward", "left", "right" };
        foreach (var (key, _) in CommandKeys.All)
        {
            if (movement.Contains(key))
            {
                continue;
            }

            Assert.Contains(ScriptDsl.Vocabulary, entry => entry.Key.Split(" + ").Contains(key));
        }
    }

    [Fact]
    public void The_rules_line_lists_exactly_the_attributes_the_parser_names()
    {
        // The parser's own refusal carries the list, so nothing has to be spelled twice: a new
        // attribute is either listed or the message stops matching the reference.
        var refused = Assert.Throws<ScriptFormatException>(() => ScriptDsl.Parse("! zz=1\n"));
        var named = refused.Message[(refused.Message.IndexOf('(') + 1)..refused.Message.IndexOf(')')]
            .Split(',', StringSplitOptions.TrimEntries)
            .Order();

        Assert.Equal(named, ScriptCommands.Rules.Select(rule => rule.Token).Order());
    }

    [Fact]
    public void Lists_no_token_twice()
    {
        var listed = ScriptCommands.Frame.Concat(ScriptCommands.Rules)
            .Select(command => command.Token)
            .ToList();

        Assert.Equal(listed.Count, listed.Distinct(StringComparer.OrdinalIgnoreCase).Count());
    }

    [Fact]
    public void Spells_a_token_the_way_the_text_writes_it()
    {
        // The spelling column is what the user copies into the text: a stick carries its argument,
        // a button is its bare token.
        Assert.Equal("ls:<angle>", ScriptCommands.Sticks.First(stick => stick.Token == "ls").Spelling);
        Assert.Equal("lsx:<value>", ScriptCommands.Sticks.First(stick => stick.Token == "lsx").Spelling);
        Assert.Equal("a", ScriptCommands.Frame.First(command => command.Token == "a").Spelling);
    }
}
