//! Runs a command while it holds the machine's engine slot, and frees the slot when the
//! command ends: the suites that start the real app run through it (`npm run test:e2e`).
//! Takes the slot before the command starts, so no clock of the command runs while it waits.
//!
//! `engine-slot <program> [arguments]`: the exit code is the program's.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(program) = args.next() else {
        eprintln!(
            "engine-slot <program> [arguments]: runs the program while it holds the machine's engine slot"
        );
        return ExitCode::from(2);
    };
    let _slot = match engine_slot::take() {
        Ok(slot) => slot,
        Err(error) => {
            eprintln!("engine-slot: {error}");
            return ExitCode::FAILURE;
        }
    };
    match Command::new(&program).args(args).status() {
        Ok(status) => ExitCode::from(exit_code(status.code())),
        Err(error) => {
            eprintln!(
                "engine-slot: cannot start {}: {error}",
                program.to_string_lossy()
            );
            ExitCode::FAILURE
        }
    }
}

/// The program's exit code as this process's: a code that does not fit, or none (ended by
/// a signal), is 1.
fn exit_code(code: Option<i32>) -> u8 {
    code.and_then(|code| u8::try_from(code).ok()).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_exit_code_is_the_programs() {
        // (the program's code, this process's)
        let cases = [
            (Some(0), 0),
            (Some(3), 3),
            (Some(255), 255),
            (Some(256), 1),
            (Some(-1), 1),
            (None, 1),
        ];
        for (code, expected) in cases {
            assert_eq!(super::exit_code(code), expected, "{code:?}");
        }
    }
}
