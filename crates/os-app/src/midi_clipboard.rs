// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The system MIDI clipboard: the piano roll's File ▸ Paste from MIDI clipboard... (spec: ui/midi-files.md §9).
//!
//! A DAW copies a clip or a MIDI pattern as a Standard MIDI File's bytes under a type of its own (`audio/midi`,
//! `application/x-midi`, `public.midi-audio`, a registered Windows format, an X11 target), as a `.mid` file's URL or
//! as the file's path in text. The platform modules below read whatever the clipboard holds ([`platforms::read`]) and
//! this module turns it into the file's bytes and the name the status message shows ([`Source::read`]); the app then
//! imports them into the piano roll's channel through the MIDI file import (`crate::midi_file::import_into_piano`),
//! without the form.

use std::path::{Path, PathBuf};

use os_smf::Smf;

/// The types a Standard MIDI File is copied under, beside a DAW's own.
pub(crate) const MIDI_TYPES: [&str; 3] = ["audio/midi", "application/x-midi", "public.midi-audio"];
/// The type that holds any data: read last, and only what parses as a Standard MIDI File is kept.
pub(crate) const ANY_DATA: &str = "public.data";
/// The name the status message shows for bytes copied as data rather than as a file.
pub(crate) const LABEL: &str = "MIDI clipboard";
/// The largest clipboard payload parsed: a bigger one is not a MIDI clip a piano roll pastes (and "public.data" can
/// hold a video), so it is refused rather than parsed.
pub(crate) const MAX_BYTES: usize = 64 << 20;
/// What the piano roll says when the clipboard holds nothing it can read.
pub(crate) const NOTHING: &str = "Paste from MIDI clipboard: the clipboard holds no MIDI data";

/// What the system clipboard offers, as far as Lemon Studio reads it.
pub(crate) enum Source {
    /// The file's bytes, under one of [`MIDI_TYPES`], [`ANY_DATA`] or a DAW's own type.
    Data(Vec<u8>),
    /// A `.mid` file named by the clipboard: a file URL, a list of them, or a path in text.
    File(PathBuf),
}

impl Source {
    /// The file's bytes and the name the status message shows: the file's own name for a file, [`LABEL`] for data.
    pub(crate) fn read(self) -> Option<(PathBuf, Vec<u8>)> {
        match self {
            Source::Data(b) => Some((PathBuf::from(LABEL), b)),
            Source::File(p) => std::fs::read(&p).ok().map(|b| (p, b)),
        }
    }
}

/// The MIDI data the clipboard holds, as (name, bytes). Never panics: no clipboard, no window system or nothing
/// usable gives None.
pub(crate) fn read() -> Option<(PathBuf, Vec<u8>)> {
    #[cfg(test)]
    if let Some(source) = TEST_SOURCE.with_borrow_mut(Option::take) {
        return source.read();
    }
    platforms::read().and_then(Source::read)
}

#[cfg(test)]
thread_local! {
    /// The payload a test hands [`read`] instead of the system clipboard's (it is taken, so one test's payload is not
    /// read by the next press).
    static TEST_SOURCE: std::cell::RefCell<Option<Source>> = const { std::cell::RefCell::new(None) };
}

/// Stand in for the system clipboard in a test (spec: ui/midi-files.md §9).
#[cfg(test)]
fn set_test_source(source: Source) {
    TEST_SOURCE.with_borrow_mut(|slot| *slot = Some(source));
}

/// `bytes` read as a Standard MIDI File; a payload too big to be one is refused.
pub(crate) fn smf(bytes: &[u8]) -> Option<Smf> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    os_smf::read(bytes).ok()
}

/// A path to a Standard MIDI File from text the clipboard holds: a `file://` URL (percent escapes decoded), a path,
/// or a list of them, one per line (the URI list a file manager copies).
pub(crate) fn path_from_text(text: &str) -> Option<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .find_map(|line| {
            let p = file_url(line).unwrap_or_else(|| PathBuf::from(line));
            is_midi_path(&p).then_some(p)
        })
}

/// Whether `p` names a Standard MIDI File (`.mid` / `.midi`).
pub(crate) fn is_midi_path(p: &Path) -> bool {
    p.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"))
}

/// Whether `name` is a type MIDI data comes under, or a DAW's own MIDI type ("Midi Clip", "Standard MIDI File"...).
/// The platform readers ask a type or format this way, so a DAW's own name is read too.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_midi_type(name: &str) -> bool {
    MIDI_TYPES
        .iter()
        .chain([&ANY_DATA])
        .any(|t| name.eq_ignore_ascii_case(t))
        || name.to_ascii_lowercase().contains("midi")
}

/// The path of a `file://` URL, percent escapes decoded; None for anything else.
fn file_url(s: &str) -> Option<PathBuf> {
    let rest = s.strip_prefix("file://")?;
    // The authority (`file://localhost/...`, `file:///...`) is empty for local files.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::with_capacity(rest.len());
    let mut it = rest.bytes();
    while let Some(b) = it.next() {
        if b == b'%' {
            let hi = it.next()?;
            let lo = it.next()?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(&[hi, lo]).ok()?, 16).ok()?);
        } else {
            bytes.push(b);
        }
    }
    Some(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use os_model::Note;
    use os_smf::{Event, Kind as Ev, Smf, Track};

    /// A synthetic type 1 file: a conductor track (tempo) then two notes of velocity 100 on MIDI channel 1.
    fn smf_bytes() -> Vec<u8> {
        let conductor = vec![Event {
            tick: 0,
            kind: Ev::Tempo(500_000),
        }];
        let notes = vec![
            Event {
                tick: 0,
                kind: Ev::NoteOn {
                    channel: 0,
                    key: 60,
                    velocity: 100,
                },
            },
            Event {
                tick: 96,
                kind: Ev::NoteOff {
                    channel: 0,
                    key: 60,
                    velocity: 0,
                },
            },
            Event {
                tick: 96,
                kind: Ev::NoteOn {
                    channel: 0,
                    key: 64,
                    velocity: 100,
                },
            },
            Event {
                tick: 192,
                kind: Ev::NoteOff {
                    channel: 0,
                    key: 64,
                    velocity: 0,
                },
            },
        ];
        os_smf::write(&Smf {
            format: 1,
            ppq: 96,
            tracks: vec![Track { events: conductor }, Track { events: notes }],
        })
    }

    /// The clipboard's bytes parse back into the same file, and the import's converter turns them into the notes the
    /// piano roll would get (spec §9): the piano roll's import uses `convert` with `per_track`.
    #[test]
    fn bytes_parse_and_convert() {
        let bytes = smf_bytes();
        let f = smf(&bytes).expect("a Standard MIDI File");
        assert_eq!((f.format, f.ppq), (1, 96));
        let imp = crate::midi_file::convert(
            &f,
            &crate::midi_file::Options {
                per_track: true,
                ..crate::midi_file::Options::default()
            },
            96,
        );
        let notes: Vec<Note> = imp
            .parts
            .iter()
            .flat_map(|p| p.notes.iter())
            .copied()
            .collect();
        let spans: Vec<(u32, u32, u16)> = notes
            .iter()
            .map(|n| (n.position, n.length, n.key))
            .collect();
        assert_eq!(spans, vec![(0, 96, 60), (96, 96, 64)], "{notes:?}");
        assert!(notes.iter().all(|n| n.velocity == 100));
        assert!(!imp.parts.is_empty(), "one part per track");
    }

    /// Text the clipboard may hold: a `file://` URL (percent escapes decoded), a path, a URI list; only a `.mid` /
    /// `.midi` name counts.
    #[test]
    fn paths_from_text() {
        assert_eq!(
            path_from_text("file:///Users/me/My%20Song.mid"),
            Some(PathBuf::from("/Users/me/My Song.mid"))
        );
        assert_eq!(
            path_from_text("/Users/me/take.midi"),
            Some(PathBuf::from("/Users/me/take.midi"))
        );
        assert_eq!(
            path_from_text("# comment\r\nfile:///tmp/a.mid\r\nfile:///tmp/b.wav"),
            Some(PathBuf::from("/tmp/a.mid"))
        );
        assert_eq!(
            path_from_text("C:\\Users\\me\\clip.MID"),
            Some(PathBuf::from("C:\\Users\\me\\clip.MID"))
        );
        assert_eq!(
            path_from_text("/Users/me/notes.txt"),
            None,
            "not a MIDI file"
        );
        assert_eq!(path_from_text("hello"), None);
    }

    /// The clipboard's file goes into the piano roll's channel through the import, without the form: the notes
    /// arrive, the piano roll selects them, says what it imported, and one undo step takes them back (spec §9).
    #[test]
    fn paste_into_the_piano_roll() {
        let mut app = crate::app::App::new(None);
        app.state.project = crate::app::test_project();
        app.state.selected_channel = 0;
        let f = smf(&smf_bytes()).expect("the file");
        let o = crate::midi_file::options_for(&f);
        app.import_into_piano(&PathBuf::from(LABEL), &f, &o);
        let ch = crate::piano::channel(&app.state).expect("a channel");
        let spans: Vec<(u32, u32, u16)> = crate::piano::notes(&app.state)
            .iter()
            .filter(|n| n.channel == ch)
            .map(|n| (n.position, n.length, n.key))
            .collect();
        assert_eq!(spans, vec![(0, 96, 60), (96, 96, 64)]);
        assert_eq!(
            app.state.piano.selection(app.state.pattern, Some(ch)).len(),
            2,
            "the imported notes are selected"
        );
        assert!(
            app.state
                .message
                .as_deref()
                .is_some_and(|m| m.contains(LABEL)),
            "{:?}",
            app.state.message
        );
        assert!(
            crate::history::undo(&mut app.state).is_some(),
            "one undo step"
        );
        assert_eq!(
            crate::piano::notes(&app.state)
                .iter()
                .filter(|n| n.channel == ch)
                .count(),
            0
        );
    }

    /// The piano roll's File menu lists the paste, with its shortcut (spec §9).
    #[test]
    fn the_menu_lists_the_paste() {
        let app = crate::app::App::new(None);
        let tools = crate::piano_tools::tools_menu(&app.state, 0.0, 0.0);
        let item = tools
            .items
            .iter()
            .find(|i| i.label == "Paste from MIDI clipboard...")
            .expect("the entry");
        assert_eq!(item.key, "Ctrl+Shift+V");
        assert_eq!(item.cmd, crate::playlist::Cmd::PasteMidiClipboard);
        assert!(item.enabled);
    }

    /// The types a DAW's MIDI data comes under are recognised by name, the other types are not.
    #[test]
    fn midi_types_by_name() {
        for t in [
            "audio/midi",
            "application/x-midi",
            "public.midi-audio",
            "Standard MIDI File",
            "Midi Clip",
            "public.data",
        ] {
            assert!(is_midi_type(t), "{t}");
        }
        for t in ["public.utf8-plain-text", "public.file-url", "image/png"] {
            assert!(!is_midi_type(t), "{t}");
        }
        assert!(smf(b"not a midi file").is_none(), "garbage is not a file");
    }

    /// Ctrl+Shift+V while the piano roll is shown pastes the clipboard's Standard MIDI File into the current channel
    /// without the form, undoably; a payload that is not one changes nothing and says so (spec §9).
    #[test]
    fn the_key_pastes_into_the_piano_roll() {
        use winit::keyboard::Key;
        let mut app = crate::app::App::new(None);
        app.state.project = crate::app::test_project();
        app.state.selected_channel = 0;
        app.state.z = vec![crate::app::Win::PianoRoll];
        let count = |app: &crate::app::App| {
            crate::piano::notes(&app.state)
                .iter()
                .filter(|n| n.channel == 0)
                .count()
        };
        let press = |app: &mut crate::app::App| {
            app.shift = true;
            app.key(&Key::Character("V".into()), true, false);
            app.shift = false;
        };
        // Not a Standard MIDI File: nothing is imported, and the status message says why.
        set_test_source(Source::Data(b"not a midi file".to_vec()));
        press(&mut app);
        assert_eq!(count(&app), 0, "nothing imported");
        assert_eq!(
            app.state.message.as_deref(),
            Some(NOTHING),
            "{:?}",
            app.state.message
        );
        // The file's bytes: its notes arrive in the piano roll's channel, selected, and one undo step takes them back.
        set_test_source(Source::Data(smf_bytes()));
        app.state.message = None;
        press(&mut app);
        assert_eq!(count(&app), 2, "{:?}", crate::piano::notes(&app.state));
        assert!(
            app.state
                .message
                .as_deref()
                .is_some_and(|m| m.contains(LABEL)),
            "{:?}",
            app.state.message
        );
        assert!(
            crate::history::undo(&mut app.state).is_some(),
            "one undo step"
        );
        assert_eq!(count(&app), 0);
    }

    /// With no channel to import into, Ctrl+Shift+V says so and imports nothing (spec §9).
    #[test]
    fn the_key_says_so_without_a_channel() {
        use winit::keyboard::Key;
        let mut app = crate::app::App::new(None);
        app.state.project = crate::app::test_project();
        app.state.project.channels.clear();
        app.state.z = vec![crate::app::Win::PianoRoll];
        set_test_source(Source::Data(smf_bytes()));
        app.shift = true;
        app.key(&Key::Character("V".into()), true, false);
        app.shift = false;
        assert!(
            app.state
                .message
                .as_deref()
                .is_some_and(|m| m.contains("select a channel first")),
            "{:?}",
            app.state.message
        );
        assert!(crate::piano::notes(&app.state).is_empty(), "nothing imported");
    }

    /// A held Ctrl+Shift+V pastes once: the import is not a key-repeat key, so holding it does not stamp the channel
    /// with one copy per repeat (spec §9).
    #[test]
    fn the_key_does_not_repeat_the_paste() {
        use winit::keyboard::Key;
        let mut app = crate::app::App::new(None);
        app.state.project = crate::app::test_project();
        app.state.selected_channel = 0;
        app.state.z = vec![crate::app::Win::PianoRoll];
        set_test_source(Source::Data(b"not a midi file".to_vec()));
        app.repeat = true;
        app.shift = true;
        app.key(&Key::Character("V".into()), true, false);
        app.shift = false;
        app.repeat = false;
        assert_eq!(app.state.message, None, "a repeat does not paste");
        assert_eq!(
            crate::piano::notes(&app.state)
                .iter()
                .filter(|n| n.channel == 0)
                .count(),
            0
        );
        // Take the stand-in payload back, so a later press on this thread does not read it.
        let _ = read();
    }

    /// The clipboard's file is read by name, and bytes copied as data come back under [`LABEL`] (spec §9).
    #[test]
    fn a_named_file_reads_its_bytes() {
        let path =
            std::env::temp_dir().join(format!("lemon-midi-clipboard-{}.mid", std::process::id()));
        std::fs::write(&path, smf_bytes()).expect("the temporary file");
        let (name, bytes) = Source::File(path.clone()).read().expect("its bytes");
        assert_eq!(name, path);
        assert_eq!(bytes, smf_bytes());
        let (name, bytes) = Source::Data(vec![1, 2, 3]).read().expect("the data");
        assert_eq!((name, bytes), (PathBuf::from(LABEL), vec![1, 2, 3]));
        let _ = std::fs::remove_file(path);
    }
}

/// The system clipboard readers, one per platform behind `cfg` (spec §9): macOS `NSPasteboard`, the Windows
/// clipboard, the X11 `CLIPBOARD` selection.
mod platforms {
    #[cfg(target_os = "macos")]
    mod macos;
    #[cfg(target_os = "macos")]
    pub(super) use macos::read;

    #[cfg(windows)]
    mod windows;
    #[cfg(windows)]
    pub(super) use windows::read;

    #[cfg(target_os = "linux")]
    mod x11;
    #[cfg(target_os = "linux")]
    pub(super) use x11::read;

    /// A system with no clipboard reader here: the piano roll only says the clipboard holds no MIDI data.
    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    pub(crate) fn read() -> Option<super::Source> {
        None
    }
}
