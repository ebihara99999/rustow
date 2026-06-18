use rustow::cli::Args;

fn main() {
    let parsed_args = Args::parse_runtime_with_operation_groups();

    if let Err(e) = rustow::run_runtime_parsed(parsed_args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
