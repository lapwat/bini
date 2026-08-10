use log::info;

use clap::Parser;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// The name of the package to install
    #[arg(value_parser = sanitize_name)]
    name: String
}

fn sanitize_name(s: &str) -> Result<String, String> {
    let mut result = s.to_string();

    if !s.contains('/') {
        result = format!("{s}/{s}");
    }

    Ok(result)
}

fn main() {
    let args =  Args::parse();

    info!("Checking for release {}", args.name);
}
