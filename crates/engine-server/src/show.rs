//! The show file on disk: opened once at start, saved after every change of the patch,
//! one save at a time on the blocking pool, never on the tick thread. What the file
//! holds: the `show-file` crate.

use engine::{Looks, Patch, Sender};
use engine_core::{ShowFile, Unopened};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::watch;

/// Opens the show file at `path` into the patch of `sender`. A file that is not there:
/// the patch stays empty and the first change makes the file. Either way the patch is
/// kept in the file from now on ([`show_file::LIMIT`]): a change that would not fit in it
/// is refused. A file that cannot be opened is left alone, and the patch stays empty.
/// Reads at most one byte over [`show_file::MOST_BYTES`]: enough for `read` to say the
/// file is too big, never a file of gigabytes.
/// The looks of the file come after its patch, checked against it.
pub fn open(path: &Path, sender: &mut Sender) -> ShowFile {
    let mut text = String::new();
    let most = u64::try_from(show_file::MOST_BYTES).unwrap_or(u64::MAX);
    let empty = sender.patch();
    let read = match File::open(path).and_then(|file| file.take(most + 1).read_to_string(&mut text))
    {
        Ok(_) => show_file::read(&text, &empty),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Ok((Patch::clone(&empty), Looks::new()))
        }
        Err(_) => Err(Unopened::CannotRead),
    };
    let opened = read.and_then(|(patch, looks)| {
        sender
            .load(patch.kept_in(show_file::LIMIT))
            .and_then(|()| sender.load_looks(looks))
            .map_err(|reason| Unopened::Refused { reason })
    });
    match opened {
        Ok(()) => ShowFile::Kept,
        Err(why) => ShowFile::LeftAlone { why },
    }
}

/// What the show file keeps: the front's patch and looks after a change.
#[derive(Clone)]
pub struct Kept {
    patch: Arc<Patch>,
    looks: Arc<Looks>,
}

impl Kept {
    pub fn of(sender: &Sender) -> Self {
        Self {
            patch: sender.patch(),
            looks: sender.looks(),
        }
    }

    fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.patch, &other.patch) && Arc::ptr_eq(&self.looks, &other.looks)
    }
}

/// Saves the patch and the looks to `path` after every change, while `state` says the file is saved.
/// A change made while a save runs is saved after it. Once `stopped`, saves the last
/// change and ends.
pub async fn keep(
    path: PathBuf,
    mut patches: watch::Receiver<Kept>,
    state: watch::Sender<ShowFile>,
    mut stopped: watch::Receiver<bool>,
) {
    let mut saved = patches.borrow_and_update().clone();
    loop {
        let stopping = tokio::select! {
            changed = patches.changed() => changed.is_err(),
            _ = stopped.wait_for(|stop| *stop) => true,
        };
        let now = patches.borrow_and_update().clone();
        if !now.same(&saved) && state.borrow().saves() {
            let (path, kept) = (path.clone(), now.clone());
            let whole = tokio::task::spawn_blocking(move || {
                show_file::write(&kept.patch, &kept.looks)
                    .is_some_and(|text| save(&path, &text).is_ok())
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

/// Writes `text` to `path` whole or not at all, so that a power cut brings back the old
/// file or the new one, never half of one.
///
/// | Step | Fails | The file at `path` |
/// |---|---|---|
/// | write `<name>.saving` beside it, sync it | `Err`, the new file removed | as it was |
/// | rename it over `path` | `Err`, the new file removed | as it was |
/// | sync the folder, so the rename is on the disk | `Err` | the new text, which a power cut may undo |
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
    renamed.and_then(|()| sync_folder(path))
}

/// Syncs the folder of `path`, where the system allows it, so a rename in it is on the
/// disk. A rename is a change of the folder, not of the file: syncing the file does not
/// keep it.
///
/// | System | What happens |
/// |---|---|
/// | Linux | `fsync` of the folder: the way `fsync(2)` names to keep a rename |
/// | macOS | `F_FULLFSYNC` of the folder (Rust's `sync_all` there), which also empties the drive's cache |
/// | Windows | nothing: a folder cannot be opened as a file. NTFS writes the rename to its journal, flushed within seconds: a power cut right after a save can bring back the old file, never half of one |
#[cfg(unix)]
fn sync_folder(path: &Path) -> io::Result<()> {
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(folder)?.sync_all()
}

#[cfg(not(unix))]
fn sync_folder(_: &Path) -> io::Result<()> {
    Ok(())
}

/// The new file a save writes before it is renamed: `<name>.saving`, beside the show file.
pub fn saving(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".saving");
    path.with_file_name(name)
}
