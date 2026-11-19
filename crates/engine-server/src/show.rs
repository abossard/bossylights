//! The show file on disk: opened once at start, saved after every change of the patch,
//! one save at a time on the blocking pool, never on the tick thread. What the file
//! holds: the `show-file` crate.

use engine::{Patch, Sender};
use engine_core::{ShowFile, Unopened};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::watch;

/// Opens the show file at `path` into the patch of `sender`. A file that is not there:
/// the patch stays empty and the first change makes the file. A file that cannot be
/// opened is left alone, and the patch stays empty.
pub fn open(path: &Path, sender: &mut Sender) -> ShowFile {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return ShowFile::Kept,
        Err(_) => {
            return ShowFile::LeftAlone {
                why: Unopened::CannotRead,
            };
        }
    };
    let opened = show_file::read(&text, &sender.patch()).and_then(|patch| {
        sender
            .load(patch)
            .map_err(|reason| Unopened::Refused { reason })
    });
    match opened {
        Ok(()) => ShowFile::Kept,
        Err(why) => ShowFile::LeftAlone { why },
    }
}

/// Saves the patch to `path` after every change, while `state` says the file is saved.
/// A change made while a save runs is saved after it. Once `stopped`, saves the last
/// change and ends.
pub async fn keep(
    path: PathBuf,
    mut patches: watch::Receiver<Arc<Patch>>,
    state: watch::Sender<ShowFile>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut saved = Arc::clone(&patches.borrow_and_update());
    loop {
        let stopping = tokio::select! {
            changed = patches.changed() => changed.is_err(),
            _ = stopped.wait_for(|stop| *stop) => true,
        };
        let now = Arc::clone(&patches.borrow_and_update());
        if !Arc::ptr_eq(&now, &saved) && state.borrow().saves() {
            let (path, patch) = (path.clone(), Arc::clone(&now));
            let whole = tokio::task::spawn_blocking(move || {
                show_file::write(&patch).is_some_and(|text| save(&path, &text).is_ok())
            })
            .await
            .unwrap_or(false);
            state.send_modify(|state| *state = state.saved(whole));
            if whole {
                saved = now;
            }
        }
        if stopping {
            return;
        }
    }
}

/// Writes `text` to `path` whole or not at all: into a new file beside it, then renamed
/// over it. On a failure the file at `path` is as it was.
pub fn save(path: &Path, text: &str) -> io::Result<()> {
    let new = saving(path);
    let written = File::create(&new).and_then(|mut file| {
        file.write_all(text.as_bytes())?;
        file.sync_all()
    });
    let renamed = written.and_then(|()| fs::rename(&new, path));
    if renamed.is_err() {
        let _ = fs::remove_file(&new);
    }
    renamed
}

/// The new file a save writes before it is renamed: `<name>.saving`, beside the show file.
pub fn saving(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".saving");
    path.with_file_name(name)
}
