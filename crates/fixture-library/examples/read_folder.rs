//! Reads a folder of QLC+ definitions (every `.qxf` under it) and prints how many were
//! read, how many failed, and why.
//!
//!     cargo run -p fixture-library --example read_folder -- path/to/resources/fixtures

use fixture_library::parse;
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
    let (mut read, mut failures) = (0, Vec::new());
    for path in &paths {
        let name = path
            .strip_prefix(&folder)
            .unwrap_or(path)
            .display()
            .to_string();
        match fs::read(path) {
            Ok(bytes) => match parse(&name, &bytes) {
                Ok(_) => read += 1,
                Err(error) => failures.push(error),
            },
            Err(error) => eprintln!("{name}: {error}"),
        }
    }
    println!(
        "read {read}, failed {}, of {} files",
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
    process::exit(i32::from(!failures.is_empty()));
}
