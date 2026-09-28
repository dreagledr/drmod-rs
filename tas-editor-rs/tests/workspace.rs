//! The workspace on disk: the file operations, and the two traps the format sets.
//!
//! Everything here is on a temporary folder — the operations are the ones a click runs, so they
//! have to be exercised on real files rather than argued about.
//!
//! ⚠️ `Settings::first_folder` is *not* tested here: it reads `current_exe`, which under `cargo
//! test` is the test binary in `target\debug\deps\`, and its answer would be a fact about this
//! machine's build directory. The bug it exists for — cargo's own empty `examples\` beside a
//! binary — is checked by `holds_scripts`'s rule instead, which is the part that can be wrong.

use std::path::{Path, PathBuf};

use tas_editor_rs::workspace;

/// A folder of its own under the temp directory, removed when the test ends.
struct TempFolder {
    path: PathBuf,
}

impl TempFolder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("tas-editor-rs-test-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("the temp folder can be made");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, text).expect("the fixture writes");
        path
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn the_listing_reads_every_tas_and_nothing_else() {
    let folder = TempFolder::new("listing");
    folder.write("b.tas", "! name=b\n0 a\n");
    folder.write("a.tas", "! name=a\n0 a\n");
    // Not a script, and not a file — only a top-level `.tas` is a script here.
    folder.write("notes.txt", "not a script");

    let listing = workspace::list(Some(folder.path()));

    assert_eq!(listing.error, None);
    let names: Vec<&str> = listing
        .scripts
        .iter()
        .map(|script| script.name.as_str())
        .collect();
    assert_eq!(names, vec!["a", "b"], "the listing is sorted and .tas only");
}

#[test]
fn a_row_carries_the_frame_count_its_text_ends_on() {
    let folder = TempFolder::new("frames");
    folder.write("run.tas", "! name=run trig=ticks:0\n0 ls:0:10\n10 lt:5\n");

    let listing = workspace::list(Some(folder.path()));
    let script = &listing.scripts[0];

    assert_eq!(script.error, None, "the fixture is a script");
    assert_eq!(script.frames, 15, "10 + 5");
    assert!(script.reads());
}

#[test]
fn a_file_the_parser_refuses_is_listed_with_its_reason() {
    let folder = TempFolder::new("broken");
    // A typo, not a missing feature: the row has to say so rather than the file vanishing from the
    // workspace — a script the author cannot see is a script they cannot fix.
    folder.write("broken.tas", "! name=broken\n0 nosuchtoken\n");

    let listing = workspace::list(Some(folder.path()));
    let script = &listing.scripts[0];

    assert!(!script.reads());
    assert_eq!(script.frames, 0);
    let error = script.error.as_deref().expect("the refusal is kept");
    assert!(
        error.contains("line 2"),
        "the row carries the parser's own line: {error}"
    );
}

#[test]
fn a_new_file_never_overwrites_one_that_is_there() {
    let folder = TempFolder::new("new");
    let first = folder.write("script.tas", "! name=first\n0 a\n");

    let (created, error) = workspace::create(folder.path());

    assert_eq!(error, None);
    let created = created.expect("a file was made");
    assert_eq!(created.file_name().unwrap(), "script-2.tas");
    assert_eq!(
        std::fs::read_to_string(&first).expect("the first is still there"),
        "! name=first\n0 a\n",
        "and it is untouched"
    );
}

#[test]
fn a_duplicate_lands_beside_its_original() {
    let folder = TempFolder::new("duplicate");
    let original = folder.write("run.tas", "! name=run\n0 a\n");

    let (copy, error) = workspace::duplicate(&original);

    assert_eq!(error, None);
    let copy = copy.expect("a copy was made");
    assert_eq!(copy.file_name().unwrap(), "run-copy.tas");
    assert_eq!(
        std::fs::read_to_string(&copy).expect("the copy reads"),
        "! name=run\n0 a\n"
    );
}

#[test]
fn a_rename_keeps_the_folder_and_the_extension() {
    let folder = TempFolder::new("rename");
    let original = folder.write("old.tas", "! name=old\n0 a\n");

    let (renamed, error) = workspace::rename(&original, "new");

    assert_eq!(error, None);
    let renamed = renamed.expect("the file moved");
    assert_eq!(renamed.file_name().unwrap(), "new.tas");
    assert_eq!(renamed.parent().unwrap(), folder.path());
    assert!(!original.exists(), "the old name is gone");
}

#[test]
fn a_rename_onto_a_taken_name_is_refused_and_says_so() {
    let folder = TempFolder::new("rename-taken");
    let original = folder.write("old.tas", "! name=old\n0 a\n");
    let taken = folder.write("taken.tas", "! name=taken\n0 a\n");

    let (renamed, error) = workspace::rename(&original, "taken");

    assert!(renamed.is_none());
    let error = error.expect("the refusal says so");
    assert!(error.contains("taken.tas"), "and names the file: {error}");
    assert!(original.exists(), "the original is untouched");
    assert_eq!(
        std::fs::read_to_string(&taken).expect("the taken file reads"),
        "! name=taken\n0 a\n",
        "and the file that had the name is untouched"
    );
}

#[test]
fn a_rename_may_not_spell_the_extension_out() {
    let folder = TempFolder::new("rename-ext");
    let original = folder.write("old.tas", "! name=old\n0 a\n");

    let (renamed, error) = workspace::rename(&original, "new.tas");

    assert!(renamed.is_none());
    assert!(
        error.expect("the refusal says so").contains("adds it"),
        "the refusal explains that the extension is the workspace's"
    );
}

#[test]
fn a_rename_may_not_carry_a_character_windows_refuses() {
    let folder = TempFolder::new("rename-char");
    let original = folder.write("old.tas", "! name=old\n0 a\n");

    let (renamed, error) = workspace::rename(&original, "a:b");

    assert!(renamed.is_none());
    assert!(error.expect("the refusal says so").contains(':'));
}

#[test]
fn a_rename_to_the_same_name_is_not_an_error() {
    let folder = TempFolder::new("rename-same");
    let original = folder.write("same.tas", "! name=same\n0 a\n");

    let (renamed, error) = workspace::rename(&original, "same");

    assert_eq!(error, None, "nothing to do, and nothing to complain about");
    assert_eq!(renamed.expect("the path comes back"), original);
    assert!(original.exists());
}

#[test]
fn a_write_normalises_the_line_separator() {
    let folder = TempFolder::new("write");
    let path = folder.path().join("written.tas");

    // A `.tas` that went through a WinUI `TextBox` carries a lone `\r` between its lines (measured),
    // which is not what the format reads as a line break. Writing it through unchanged would produce
    // a file that reads as one long broken line.
    let error = workspace::write(&path, "! name=x\r0 a\r1 lt\r");
    assert_eq!(error, None);

    let text = std::fs::read_to_string(&path).expect("the file reads");
    assert_eq!(text, "! name=x\n0 a\n1 lt\n");
    assert!(
        workspace::list(Some(folder.path())).scripts[0].reads(),
        "and the written file is the script it was"
    );
}

#[test]
fn the_listed_text_keeps_its_own_line_breaks() {
    // ⚠️ The buffer the editor is handed must stay `\n`-separated: `dear-imgui-cte` is not a WinUI
    // `TextBox` and does not take `\r` as a line break, so a buffer converted to `\r` shows the whole
    // script on **one line** (measured — it is what this port did first). This is the guard for that:
    // whatever the file's own separators are, the buffer the editor gets is `\n`.
    let folder = TempFolder::new("separators");
    folder.write("unix.tas", "! name=x\n0 a\n1 lt\n");
    folder.write("windows.tas", "! name=x\r\n0 a\r\n1 lt\r\n");

    let listing = workspace::list(Some(folder.path()));
    assert_eq!(listing.scripts.len(), 2);

    let mut buffers = tas_editor_rs::buffers::Buffers::new();
    for script in &listing.scripts {
        let text = tas_editor_rs::buffers::resolve(&mut buffers, script);
        assert_eq!(
            text, "! name=x\n0 a\n1 lt\n",
            "{}: the editor's text is `\\n`-separated",
            script.name
        );
        assert_eq!(text.lines().count(), 3, "and reads as three lines");
    }
}

#[test]
fn a_typed_buffer_is_shown_back_exactly_as_typed() {
    // The editor must not rewrite what it was given: a `\n` put back on the way in is the whole
    // reason the caret stays put, and a `\r` would collapse the text to one line.
    let folder = TempFolder::new("buffer");
    folder.write("run.tas", "! name=run\n0 a\n");
    let listing = workspace::list(Some(folder.path()));
    let script = &listing.scripts[0];

    let mut buffers = tas_editor_rs::buffers::Buffers::new();
    tas_editor_rs::buffers::typed(&mut buffers, script, "! name=run\n0 a\n1 lt\n".to_owned());

    assert_eq!(
        tas_editor_rs::buffers::resolve(&mut buffers, script),
        "! name=run\n0 a\n1 lt\n"
    );
    assert!(tas_editor_rs::buffers::is_dirty(&buffers, script));
}

#[test]
fn a_folder_that_is_gone_is_reported_rather_than_looked_into() {
    // The state a workspace is in when its folder was deleted under it — the pane has to say so,
    // not throw during a render.
    let listing = workspace::list(Some(Path::new("Z:\\no\\such\\folder\\anywhere")));

    assert!(listing.scripts.is_empty());
    assert!(listing.error.expect("an error is carried").contains("gone"));
}

#[test]
fn no_folder_is_not_an_error() {
    let listing = workspace::list(None);

    assert!(listing.scripts.is_empty());
    assert_eq!(listing.error, None, "there is simply no workspace yet");
}
