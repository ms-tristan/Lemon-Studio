// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The macOS clipboard: `NSPasteboard`, the general pasteboard (spec: ui/midi-files.md §9). A DAW putting a MIDI
//! clip on it offers the Standard MIDI File's bytes under a type of its own (`public.midi-audio`, `audio/midi`, a
//! registered UTI) or, like the Finder, the `.mid` file's URL as text.

use std::ptr::NonNull;

use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSData, NSString};

use crate::midi_clipboard::{
    is_midi_type, path_from_text, smf, Source, ANY_DATA, MAX_BYTES, MIDI_TYPES,
};

/// The clipboard's MIDI data: the file's bytes under a MIDI type (then, last, any data that parses as a Standard MIDI
/// File), else the `.mid` file its URL or text names.
pub(crate) fn read() -> Option<Source> {
    let pasteboard = NSPasteboard::generalPasteboard();
    for name in MIDI_TYPES.iter().copied().chain([ANY_DATA]) {
        let Some(data) = pasteboard.dataForType(&NSString::from_str(name)) else {
            continue;
        };
        let bytes = bytes(&data);
        // "public.data" holds anything a program copied: only a Standard MIDI File is kept.
        if smf(&bytes).is_some() {
            return Some(Source::Data(bytes));
        }
    }
    // The types the copy declares: a DAW's own type naming MIDI is read as the file's bytes too.
    if let Some(types) = pasteboard.types() {
        for t in types.iter() {
            if !is_midi_type(&t.to_string()) {
                continue;
            }
            let Some(data) = pasteboard.dataForType(&t) else {
                continue;
            };
            let bytes = bytes(&data);
            if smf(&bytes).is_some() {
                return Some(Source::Data(bytes));
            }
        }
    }
    // A file URL ("file:///Users/me/Song.mid") or a path in text.
    for name in [
        "public.file-url",
        "public.utf8-plain-text",
        "NSStringPboardType",
    ] {
        let Some(text) = pasteboard.stringForType(&NSString::from_str(name)) else {
            continue;
        };
        if let Some(path) = path_from_text(&text.to_string()) {
            return Some(Source::File(path));
        }
    }
    None
}

/// The bytes of an `NSData` (its length and its buffer, as `getBytes:length:` fills them); nothing for a payload too
/// big to be a MIDI clip.
fn bytes(data: &NSData) -> Vec<u8> {
    let len = data.length();
    if len > MAX_BYTES {
        return Vec::new();
    }
    let mut out = vec![0u8; len];
    if let Some(buffer) = NonNull::new(out.as_mut_ptr().cast()) {
        // SAFETY: the buffer is `len` bytes long, the length the data holds.
        unsafe { data.getBytes_length(buffer, len) };
    }
    out
}
