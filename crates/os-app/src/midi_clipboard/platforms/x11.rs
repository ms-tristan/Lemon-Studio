// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The X11 clipboard (spec: ui/midi-files.md §9): the `CLIPBOARD` selection, asked for its targets and then for the
//! MIDI one it offers (`audio/midi`, `application/x-midi`, `public.midi-audio`, a DAW's own) or, as text, the URI
//! list or the path of a `.mid` file. A Wayland session has no X11 display: there is no clipboard to read.

use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::{CURRENT_TIME, NONE};

use crate::midi_clipboard::{is_midi_type, path_from_text, smf, Source, ANY_DATA, MIDI_TYPES};

/// How long the selection's owner has to answer before the paste says the clipboard holds no MIDI data.
const TIMEOUT: Duration = Duration::from_millis(500);

/// The clipboard's MIDI data.
pub(crate) fn read() -> Option<Source> {
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = conn.setup().roots.get(screen_num)?;
    // A window of our own for the transfer: the owner writes the selection's data into its property.
    let win = conn.generate_id().ok()?;
    conn.create_window(
        screen.root_depth,
        win,
        screen.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .ok()?;
    let clipboard = atom(&conn, b"CLIPBOARD")?;
    let prop = atom(&conn, b"LEMON_MIDI_CLIPBOARD")?;
    // The targets the owner offers; a program that does not list them is asked for the MIDI ones anyway.
    let targets = atom(&conn, b"TARGETS")?;
    let offered = transfer(&conn, win, clipboard, targets, prop)
        .map(|v| atoms(&v))
        .unwrap_or_default();
    // The MIDI types the owner says it offers (a DAW's own included); a program that lists no targets is asked for
    // the standard ones anyway.
    let mut midi: Vec<Atom> = offered
        .iter()
        .copied()
        .filter(|&a| atom_name(&conn, a).is_some_and(|n| is_midi_type(&n)))
        .collect();
    if midi.is_empty() && offered.is_empty() {
        midi.extend(
            MIDI_TYPES
                .iter()
                .chain([&ANY_DATA])
                .filter_map(|t| atom(&conn, t.as_bytes())),
        );
    }
    for target in midi {
        let Some(value) = transfer(&conn, win, clipboard, target, prop) else {
            continue;
        };
        if smf(&value).is_some() {
            return Some(Source::Data(value));
        }
    }
    for name in ["text/uri-list", "UTF8_STRING", "STRING"] {
        let Some(target) = atom(&conn, name.as_bytes()) else {
            continue;
        };
        if !offered.is_empty() && !offered.contains(&target) {
            continue;
        }
        let Some(value) = transfer(&conn, win, clipboard, target, prop) else {
            continue;
        };
        if let Some(path) = path_from_text(&String::from_utf8_lossy(&value)) {
            return Some(Source::File(path));
        }
    }
    None
}

/// An atom by name.
fn atom(conn: &RustConnection, name: &[u8]) -> Option<Atom> {
    conn.intern_atom(false, name)
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

/// An atom's name ("audio/midi", "text/uri-list"...).
fn atom_name(conn: &RustConnection, atom: Atom) -> Option<String> {
    conn.get_atom_name(atom)
        .ok()?
        .reply()
        .ok()
        .map(|r| String::from_utf8_lossy(&r.name).into_owned())
}

/// The atoms a property's value holds (format 32: four bytes each).
fn atoms(value: &[u8]) -> Vec<Atom> {
    value
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Ask the selection's owner for `target`: the property's bytes, once it answers. None when it answers with no
/// property or does not answer in [`TIMEOUT`].
fn transfer(
    conn: &RustConnection,
    win: u32,
    selection: Atom,
    target: Atom,
    prop: Atom,
) -> Option<Vec<u8>> {
    conn.convert_selection(win, selection, target, prop, CURRENT_TIME)
        .ok()?;
    conn.flush().ok()?;
    let deadline = Instant::now() + TIMEOUT;
    while Instant::now() < deadline {
        match conn.poll_for_event().ok()? {
            Some(Event::SelectionNotify(e)) if e.requestor == win => {
                if e.property == NONE {
                    return None;
                }
                let reply = conn
                    .get_property(true, win, e.property, AtomEnum::ANY, 0, u32::MAX)
                    .ok()?
                    .reply()
                    .ok()?;
                return Some(reply.value);
            }
            Some(_) => continue,
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    None
}
