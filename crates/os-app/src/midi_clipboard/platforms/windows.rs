// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! The Windows clipboard (spec: ui/midi-files.md §9): a DAW's registered format for its MIDI data (its name says
//! MIDI), the files a copy in Explorer puts there (`CF_HDROP`) and the text of a copied path (`CF_UNICODETEXT`).

use std::path::PathBuf;

use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW, OpenClipboard,
};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

use crate::midi_clipboard::{is_midi_path, is_midi_type, path_from_text, smf, Source, MAX_BYTES};

/// The formats holding text and a list of files.
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
/// The first format number a program registers itself (its name is asked for; the lower ones are the system's).
const FIRST_REGISTERED: u32 = 0xC000;

/// The clipboard, open for as long as this lives: the whole desktop shares it, so it is closed as soon as the read
/// is done.
struct Open;

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: the clipboard was opened by `open` below.
        unsafe { CloseClipboard() };
    }
}

fn open() -> Option<Open> {
    // SAFETY: `OpenClipboard` takes the window asking (none: the calling program only asks for data); a null `HWND`
    // is a pointer, not an owned window.
    (unsafe { OpenClipboard(std::ptr::null_mut()) } != 0).then_some(Open)
}

/// The clipboard's MIDI data: a registered MIDI format's bytes, else a `.mid` file from a copied list or path.
pub(crate) fn read() -> Option<Source> {
    let _open = open()?;
    let mut format = 0;
    loop {
        // SAFETY: each call gives the next format the clipboard holds, 0 when they run out.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if format < FIRST_REGISTERED {
            continue;
        }
        let Some(name) = format_name(format) else {
            continue;
        };
        if !is_midi_type(&name) {
            continue;
        }
        if let Some(bytes) = data(format).filter(|b| smf(b).is_some()) {
            return Some(Source::Data(bytes));
        }
    }
    if let Some(path) = data(CF_HDROP)
        .and_then(|b| drop_files(&b))
        .and_then(|files| files.into_iter().find(|p| is_midi_path(p)))
    {
        return Some(Source::File(path));
    }
    if let Some(path) = data(CF_UNICODETEXT)
        .and_then(|b| utf16(&b))
        .and_then(|text| path_from_text(&text))
    {
        return Some(Source::File(path));
    }
    None
}

/// The bytes of clipboard `format`.
fn data(format: u32) -> Option<Vec<u8>> {
    // SAFETY: the clipboard is open (`Open`) and the handle belongs to it; it is neither freed nor changed here.
    let handle = unsafe { GetClipboardData(format) };
    if handle.is_null() {
        return None;
    }
    let lock = unsafe { GlobalLock(handle) };
    if lock.is_null() {
        return None;
    }
    let size = unsafe { GlobalSize(handle) };
    // A payload bigger than a MIDI clip is not read: "public.data"-like formats can hold anything.
    let bytes = if size == 0 || size > MAX_BYTES {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(lock.cast::<u8>(), size) }.to_vec()
    };
    unsafe { GlobalUnlock(handle) };
    Some(bytes)
}

/// A registered format's name ("Standard MIDI File", "audio/midi"...); None for the system's own formats.
fn format_name(format: u32) -> Option<String> {
    let mut buf = [0u16; 256];
    // SAFETY: the buffer is as long as its count says, which `GetClipboardFormatNameW` respects.
    let n = unsafe { GetClipboardFormatNameW(format, buf.as_mut_ptr(), buf.len() as i32) };
    (n > 0).then(|| String::from_utf16_lossy(&buf[..(n as usize).min(buf.len())]))
}

/// The files a `CF_HDROP` list holds: its `DROPFILES` header (the list's offset, whether it is UTF-16) then the
/// paths, each ended by a null (and the list by another).
fn drop_files(bytes: &[u8]) -> Option<Vec<PathBuf>> {
    let offset = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize;
    let wide = u32::from_le_bytes(bytes.get(16..20)?.try_into().ok()?) != 0;
    if !wide {
        return None;
    }
    let units: Vec<u16> = bytes
        .get(offset..)?
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(
        units
            .split(|&u| u == 0)
            .filter(|part| !part.is_empty())
            .map(|part| PathBuf::from(String::from_utf16_lossy(part)))
            .collect(),
    )
}

/// A `CF_UNICODETEXT` string: UTF-16, ended by a null.
fn utf16(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}
