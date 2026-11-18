//! The fixture library of this engine: the definitions in the `--fixtures` folder, read
//! once on a thread of their own, and the answers to `Search`, `Fixture` and `Failures`.
//!
//! ```text
//! start(folder) --> thread "fixtures": every .qxf under the folder --> watch<Shelf> --> each connection
//! ```
//!
//! Neither the tick thread nor the API thread reads a file or parses one. A file that
//! cannot be read is a failure on the shelf; the engine runs on.

use engine_core::{
    ChannelFunction, Failure, FixtureKind, Library, MATCHES, Match, ModeChannels, ModeSize,
    NamedChannel, Refusal, Reply,
};
use fixture_library::{Fixture, Function, Kind, parse, search};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tokio::sync::watch;

/// Where reading the folder is, and what it read.
#[derive(Debug)]
pub enum Shelf {
    NoFolder,
    Reading,
    CannotOpen,
    Read {
        fixtures: Vec<Fixture>,
        failures: Vec<Failed>,
    },
}

/// A file under the folder that could not be read.
#[derive(Debug, PartialEq, Eq)]
pub struct Failed {
    file: String,
    line: Option<u32>,
    problem: String,
}

/// Starts reading `folder` on a thread of its own. The shelf says `Reading` until it is
/// done; with no folder it says `NoFolder`.
pub fn start(folder: Option<PathBuf>) -> watch::Receiver<Arc<Shelf>> {
    let Some(folder) = folder else {
        return watch::channel(Arc::new(Shelf::NoFolder)).1;
    };
    let (shelve, shelf) = watch::channel(Arc::new(Shelf::Reading));
    let reader = Arc::new(shelve);
    let failed = Arc::clone(&reader);
    let started = thread::Builder::new()
        .name("fixtures".into())
        .spawn(move || reader.send_replace(Arc::new(read(&folder))));
    if started.is_err() {
        failed.send_replace(Arc::new(Shelf::CannotOpen));
    }
    shelf
}

/// Reads every `.qxf` file under `folder`, in the order of their names.
fn read(folder: &Path) -> Shelf {
    let mut paths = Vec::new();
    let mut unlisted = Vec::new();
    if files(folder, &mut paths, &mut unlisted).is_err() {
        return Shelf::CannotOpen;
    }
    paths.sort();
    let files = paths.into_iter().map(|path| {
        let bytes = fs::read(&path);
        (path, bytes)
    });
    shelve(files.collect(), unlisted, folder)
}

/// Every `.qxf` file under `dir` into `found`; a folder below it that cannot be listed
/// into `unlisted`. Fails only when `dir` itself cannot be listed.
fn files(
    dir: &Path,
    found: &mut Vec<PathBuf>,
    unlisted: &mut Vec<(PathBuf, io::Error)>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            if let Err(error) = files(&path, found, unlisted) {
                unlisted.push((path, error));
            }
        } else if path.extension().is_some_and(|e| e == "qxf") {
            found.push(path);
        }
    }
    Ok(())
}

/// The shelf for these files: each parsed, or a failure that says why. Names are under
/// `folder`, with `/` between folders.
fn shelve(
    files: Vec<(PathBuf, io::Result<Vec<u8>>)>,
    unlisted: Vec<(PathBuf, io::Error)>,
    folder: &Path,
) -> Shelf {
    let name = |path: &Path| {
        let under = path.strip_prefix(folder).unwrap_or(path);
        under
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut fixtures = Vec::new();
    let mut failures: Vec<Failed> = unlisted
        .into_iter()
        .map(|(path, error)| Failed {
            file: name(&path),
            line: None,
            problem: error.to_string(),
        })
        .collect();
    for (path, bytes) in files {
        let file = name(&path);
        match bytes.map(|bytes| parse(&file, &bytes)) {
            Ok(Ok(fixture)) => fixtures.push(fixture),
            Ok(Err(error)) => failures.push(Failed {
                file,
                line: error.line,
                problem: error.problem.to_string(),
            }),
            Err(error) => failures.push(Failed {
                file,
                line: None,
                problem: error.to_string(),
            }),
        }
    }
    failures.sort_by(|a, b| a.file.cmp(&b.file));
    Shelf::Read { fixtures, failures }
}

impl Shelf {
    /// What the state shows.
    pub fn library(&self) -> Library {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        match self {
            Self::NoFolder => Library::NoFolder,
            Self::Reading => Library::Reading,
            Self::CannotOpen => Library::CannotOpen,
            Self::Read { fixtures, failures } => Library::Read {
                read: count(fixtures.len()),
                failed: count(failures.len()),
            },
        }
    }

    fn fixtures(&self) -> &[Fixture] {
        match self {
            Self::Read { fixtures, .. } => fixtures,
            Self::NoFolder | Self::Reading | Self::CannotOpen => &[],
        }
    }

    /// `Found`: the best matches of `text`, each mode with its number of channels.
    pub fn found(&self, text: &str) -> Option<String> {
        let found: Vec<&Fixture> = search(self.fixtures(), text)
            .into_iter()
            .take(MATCHES)
            .collect();
        let sizes: Vec<Vec<ModeSize>> = found
            .iter()
            .map(|fixture| {
                (fixture.modes.iter())
                    .map(|mode| ModeSize {
                        name: &mode.name,
                        channels: u16::try_from(mode.channels.len()).unwrap_or(u16::MAX),
                    })
                    .collect()
            })
            .collect();
        let fixtures: Vec<Match> = found
            .iter()
            .zip(&sizes)
            .map(|(fixture, modes)| Match {
                maker: &fixture.maker,
                model: &fixture.model,
                kind: kind(&fixture.kind),
                modes,
            })
            .collect();
        json(&Reply::Found {
            fixtures: &fixtures,
        })
    }

    /// `Fixture`: its modes, each channel by name and function. `NoSuchFixture` when no
    /// fixture has exactly this maker and model.
    pub fn fixture(&self, maker: &str, model: &str) -> Option<String> {
        let Some(fixture) = (self.fixtures().iter())
            .find(|fixture| fixture.maker == maker && fixture.model == model)
        else {
            return json(&Reply::Refused {
                reason: Refusal::NoSuchFixture,
            });
        };
        let channels: Vec<Vec<NamedChannel>> = (fixture.modes.iter())
            .map(|mode| {
                (mode.channels.iter())
                    .map(|channel| NamedChannel {
                        name: &channel.name,
                        function: function(&channel.function),
                    })
                    .collect()
            })
            .collect();
        let modes: Vec<ModeChannels> = (fixture.modes.iter())
            .zip(&channels)
            .map(|(mode, channels)| ModeChannels {
                name: &mode.name,
                channels,
            })
            .collect();
        json(&Reply::Fixture {
            maker: &fixture.maker,
            model: &fixture.model,
            kind: kind(&fixture.kind),
            modes: &modes,
        })
    }

    /// `Failures`: every file that could not be read, by name.
    pub fn failures(&self) -> Option<String> {
        let failed: &[Failed] = match self {
            Self::Read { failures, .. } => failures,
            Self::NoFolder | Self::Reading | Self::CannotOpen => &[],
        };
        let failures: Vec<Failure> = (failed.iter())
            .map(|failed| Failure {
                file: &failed.file,
                line: failed.line,
                problem: &failed.problem,
            })
            .collect();
        json(&Reply::Failures {
            failures: &failures,
        })
    }
}

/// A reply is plain numbers and text: this does not fail.
fn json(reply: &Reply) -> Option<String> {
    serde_json::to_string(reply).ok()
}

fn kind(kind: &Kind) -> FixtureKind<'_> {
    match kind {
        Kind::ColorChanger => FixtureKind::ColorChanger,
        Kind::Dimmer => FixtureKind::Dimmer,
        Kind::Effect => FixtureKind::Effect,
        Kind::Fan => FixtureKind::Fan,
        Kind::Flower => FixtureKind::Flower,
        Kind::Hazer => FixtureKind::Hazer,
        Kind::Laser => FixtureKind::Laser,
        Kind::LedBarBeams => FixtureKind::LedBarBeams,
        Kind::LedBarPixels => FixtureKind::LedBarPixels,
        Kind::MovingHead => FixtureKind::MovingHead,
        Kind::Other => FixtureKind::Other,
        Kind::Scanner => FixtureKind::Scanner,
        Kind::Smoke => FixtureKind::Smoke,
        Kind::Strobe => FixtureKind::Strobe,
        Kind::Unlisted(name) => FixtureKind::Unlisted { name },
    }
}

fn function(function: &Function) -> ChannelFunction<'_> {
    match function {
        Function::Dimmer => ChannelFunction::Dimmer,
        Function::Red => ChannelFunction::Red,
        Function::Green => ChannelFunction::Green,
        Function::Blue => ChannelFunction::Blue,
        Function::White => ChannelFunction::White,
        Function::Amber => ChannelFunction::Amber,
        Function::Uv => ChannelFunction::Uv,
        Function::Indigo => ChannelFunction::Indigo,
        Function::Lime => ChannelFunction::Lime,
        Function::Cyan => ChannelFunction::Cyan,
        Function::Magenta => ChannelFunction::Magenta,
        Function::Yellow => ChannelFunction::Yellow,
        Function::Hue => ChannelFunction::Hue,
        Function::Saturation => ChannelFunction::Saturation,
        Function::Lightness => ChannelFunction::Lightness,
        Function::Value => ChannelFunction::Value,
        Function::ColorTemperature => ChannelFunction::ColorTemperature,
        Function::Pan => ChannelFunction::Pan,
        Function::Tilt => ChannelFunction::Tilt,
        Function::Zoom => ChannelFunction::Zoom,
        Function::Focus => ChannelFunction::Focus,
        Function::Iris => ChannelFunction::Iris,
        Function::Frost => ChannelFunction::Frost,
        Function::Gobo => ChannelFunction::Gobo,
        Function::ColorWheel => ChannelFunction::ColorWheel,
        Function::Strobe => ChannelFunction::Strobe,
        Function::Shutter => ChannelFunction::Shutter,
        Function::Prism => ChannelFunction::Prism,
        Function::Speed => ChannelFunction::Speed,
        Function::Effect => ChannelFunction::Effect,
        Function::Control => ChannelFunction::Control,
        Function::NoFunction => ChannelFunction::NoFunction,
        Function::Other(name) => ChannelFunction::Other { name },
    }
}

#[cfg(test)]
mod tests {
    use super::{Failed, PathBuf, Shelf, shelve};
    use engine_core::Library;
    use std::io;

    #[test]
    fn every_file_is_read_or_a_failure_that_says_why() {
        let rgb = include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");
        let gone = || io::Error::from(io::ErrorKind::NotFound);
        let failed = |file: &str, line, problem: &str| Failed {
            file: file.into(),
            line,
            problem: problem.into(),
        };
        let not_xml = "not valid xml: the root node was opened but never closed";
        // (files, folders that could not be listed, read, failures)
        type Files = Vec<(&'static str, io::Result<Vec<u8>>)>;
        let cases: [(Files, Vec<&str>, u32, Vec<Failed>); 5] = [
            (vec![], vec![], 0, vec![]),
            (
                vec![("Generic/rgb.qxf", Ok(rgb.to_vec()))],
                vec![],
                1,
                vec![],
            ),
            (
                vec![
                    ("b.qxf", Ok(b"<FixtureDefinition>".to_vec())),
                    ("a.qxf", Err(gone())),
                ],
                vec![],
                0,
                vec![
                    failed("a.qxf", None, &gone().to_string()),
                    failed("b.qxf", Some(1), not_xml),
                ],
            ),
            (
                vec![],
                vec!["Locked"],
                0,
                vec![failed("Locked", None, &gone().to_string())],
            ),
            (
                vec![
                    ("rgb.qxf", Ok(rgb.to_vec())),
                    ("rgb copy.qxf", Ok(rgb.to_vec())),
                ],
                vec![],
                2,
                vec![],
            ),
        ];
        let folder = PathBuf::from("/fixtures");
        for (files, unlisted, read, failures) in cases {
            let files = files
                .into_iter()
                .map(|(name, bytes)| (folder.join(name), bytes))
                .collect();
            let unlisted = unlisted
                .into_iter()
                .map(|name| (folder.join(name), gone()))
                .collect();
            let shelf = shelve(files, unlisted, &folder);
            let failed = u32::try_from(failures.len()).unwrap();
            assert_eq!(shelf.library(), Library::Read { read, failed });
            let Shelf::Read { failures: got, .. } = shelf else {
                panic!("read")
            };
            assert_eq!(got, failures);
        }
    }
}
