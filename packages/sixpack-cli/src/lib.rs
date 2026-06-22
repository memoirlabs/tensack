//! Command-line layer for sixpack.
//!
//! This crate owns command parsing and CLI behavior.

/// Runs the command-line surface.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<(), CliError> {
    let mut args = args.into_iter();

    match args.next().as_deref() {
        Some("--version") | Some("-V") => {
            println!("sixpack {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(CliError::UnknownCommand(command.to_owned())),
    }
}

/// Command-line errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    /// The command is not recognized.
    UnknownCommand(String),
}

impl CliError {
    /// Returns the intended process exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::UnknownCommand(_) => 2,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCommand(command) => {
                writeln!(formatter, "unknown command: {command}")?;
                write!(formatter, "run `sixpack help` for usage")
            }
        }
    }
}

impl std::error::Error for CliError {}

fn print_help() {
    println!("sixpack");
    println!();
    println!("Usage:");
    println!("  sixpack --version");
    println!("  sixpack help");
}
