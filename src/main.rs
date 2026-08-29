use std::process::ExitCode;

fn main() -> ExitCode {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("Error: failed to determine current working directory: {error}");
            return ExitCode::from(5);
        }
    };

    let cli = dirrake::cli::Cli::parse_args();
    match dirrake::run(cli, &cwd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::from(error.code())
        }
    }
}
