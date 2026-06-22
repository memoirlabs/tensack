use std::env;

fn main() {
    if let Err(error) = sixpack_cli::run(env::args().skip(1)) {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
