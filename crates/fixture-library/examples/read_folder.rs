//! Reads a folder of QLC+ definitions (every `.qxf` under it) and prints how many were
//! read, how many failed and why, every warning (what was left out so the rest could be
//! read), then the tally of what the model makes of them: what
//! the channels of every mode do, what is left as "other", fine bytes, defaults, what
//! ranges do, and how shutters and strobes let light out.
//!
//!     cargo run -p fixture-library --example read_folder -- path/to/resources/fixtures

use fixture_library::{Byte, Does, Fixture, Function, Mode, Pace, parse};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{env, fs, io, process};

fn files(dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files(&path, found)?;
        } else if path.extension().is_some_and(|e| e == "qxf") {
            found.push(path);
        }
    }
    Ok(())
}

fn main() {
    let Some(folder) = env::args().nth(1) else {
        eprintln!("usage: read_folder <folder of .qxf files>");
        process::exit(2);
    };
    let mut paths = Vec::new();
    if let Err(error) = files(Path::new(&folder), &mut paths) {
        eprintln!("{folder}: {error}");
        process::exit(2);
    }
    paths.sort();
    let (mut read, mut failures, mut warnings) = (Vec::new(), Vec::new(), Vec::new());
    for path in &paths {
        let name = path
            .strip_prefix(&folder)
            .unwrap_or(path)
            .display()
            .to_string();
        match fs::read(path) {
            Ok(bytes) => match parse(&name, &bytes) {
                Ok(fixture) => {
                    warnings.extend(fixture.warnings.iter().map(|w| format!("{name}: {w}")));
                    read.push(fixture);
                }
                Err(error) => failures.push(error),
            },
            Err(error) => eprintln!("{name}: {error}"),
        }
    }
    println!(
        "read {}, failed {}, of {} files",
        read.len(),
        failures.len(),
        paths.len()
    );
    let mut why: BTreeMap<String, usize> = BTreeMap::new();
    for failure in &failures {
        println!("{failure}");
        *why.entry(failure.problem.to_string()).or_default() += 1;
    }
    if !why.is_empty() {
        println!("\nwhy:");
        for (problem, count) in why {
            println!("{count:>5}  {problem}");
        }
    }
    println!("\n{} warnings: left out, the rest read", warnings.len());
    for warning in &warnings {
        println!("{warning}");
    }
    tally(&read);
    process::exit(i32::from(!failures.is_empty()));
}

/// The name of a function, with every `Other` counted as one.
fn function_name(function: &Function) -> String {
    let name = format!("{function:?}");
    name.split('(').next().unwrap_or_default().to_string()
}

fn row(what: &str, count: usize, of: usize) {
    #[allow(clippy::cast_precision_loss, reason = "a share for people to read")]
    let share = 100.0 * count as f64 / of.max(1) as f64;
    println!("| {what} | {count} | {share:.1}% |");
}

fn tally(fixtures: &[Fixture]) {
    let modes: Vec<&Mode> = fixtures.iter().flat_map(|f| &f.modes).collect();
    let channels: Vec<_> = modes.iter().flat_map(|m| &m.channels).collect();
    let total = channels.len();
    println!("\n{} modes, {total} channels in modes\n", modes.len());

    println!("| Channels that are | Count | Share |\n|---|---|---|");
    let mut by_function: BTreeMap<String, usize> = BTreeMap::new();
    let mut others: BTreeMap<&str, usize> = BTreeMap::new();
    for channel in &channels {
        *by_function
            .entry(function_name(&channel.function))
            .or_default() += 1;
        if let Function::Other(name) = &channel.function {
            *others.entry(name.as_str()).or_default() += 1;
        }
    }
    let mut sorted: Vec<_> = by_function.into_iter().collect();
    sorted.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    for (name, count) in sorted {
        row(&name, count, total);
    }
    let fine = channels.iter().filter(|c| c.byte == Byte::Fine).count();
    let fine_named_coarse = channels
        .iter()
        .filter(|c| c.byte == Byte::Coarse && c.name.to_ascii_lowercase().contains("fine"))
        .count();
    let with_default = channels.iter().filter(|c| c.default.is_some()).count();
    println!("\n| Also | Count | Share |\n|---|---|---|");
    row("fine bytes", fine, total);
    row(
        "\"fine\" in the name, read as coarse",
        fine_named_coarse,
        total,
    );
    row("with a default", with_default, total);
    let with_direction = channels.iter().filter(|c| c.direction.is_some()).count();
    row("with a direction", with_direction, total);

    let mut others: Vec<_> = others.into_iter().collect();
    others.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    println!("\n| Most common \"other\" | Count |\n|---|---|");
    for (name, count) in others.iter().take(15) {
        println!("| {name} | {count} |");
    }

    let ranges: Vec<_> = channels.iter().flat_map(|c| &c.ranges).collect();
    let mut does: BTreeMap<&str, usize> = BTreeMap::new();
    for range in &ranges {
        let name = match &range.does {
            None => "nothing said",
            Some(Does::Open) => "open",
            Some(Does::Closed) => "closed",
            Some(Does::Strobe(Pace::Hertz(_))) => "strobe, in hertz",
            Some(Does::Strobe(Pace::Goes(_))) => "strobe, slow to fast or back",
            Some(Does::Strobe(Pace::Random)) => "strobe, at random",
            Some(Does::Strobe(Pace::Unsaid)) => "strobe, pace not said",
            Some(Does::Color(_)) => "a colour",
            Some(Does::Colors(..)) => "two colours",
            Some(Does::Rotation(_)) => "rotation",
            Some(Does::Prism { .. }) => "prism",
            Some(Does::PrismOff) => "prism off",
            Some(Does::Gobo {
                picture: Some(_), ..
            }) => "gobo, with a picture",
            Some(Does::Gobo { picture: None, .. }) => "gobo, no picture",
            Some(Does::LampOn) => "lamp on",
            Some(Does::LampOff) => "lamp off",
            Some(Does::Reset) => "reset",
            Some(Does::Runs(_)) => "runs up or down",
        };
        *does.entry(name).or_default() += 1;
    }
    println!("\n| Ranges that say | Count | Share |\n|---|---|---|");
    for (name, count) in does {
        row(name, count, ranges.len());
    }

    let shutters: Vec<_> = channels.iter().filter(|c| c.open().is_some()).collect();
    let marked = shutters
        .iter()
        .filter(|c| c.ranges.iter().any(|r| r.does == Some(Does::Open)))
        .count();
    let with_shutter = modes
        .iter()
        .filter(|m| m.channels.iter().any(|c| c.open().is_some()))
        .count();
    println!("\n| Shutters and strobes | Count | Share |\n|---|---|---|");
    row(
        "modes with a shutter or strobe channel",
        with_shutter,
        modes.len(),
    );
    row("channels: a range says open", marked, shutters.len());
    row(
        "channels: open by the rule",
        shutters.len() - marked,
        shutters.len(),
    );

    let with_heads: Vec<_> = modes.iter().filter(|m| !m.pixels.is_empty()).collect();
    let without_layout = with_heads.iter().filter(|m| m.layout.is_none()).count();
    println!("\n| Pixels and movement | Count | Share |\n|---|---|---|");
    row("modes with heads", with_heads.len(), modes.len());
    row(
        "of those: no layout, or one that does not fit the heads",
        without_layout,
        with_heads.len(),
    );
    let with_degrees = modes.iter().filter(|m| m.degrees.is_some()).count();
    row(
        "modes with a pan and tilt range in degrees",
        with_degrees,
        modes.len(),
    );
}
