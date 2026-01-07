use clap::Parser;
use clio::ClioPath;

#[derive(Parser, Debug)]
#[clap(author, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    /// Dump all streams found in CFB file.
    all: bool,

    /// Overwrite the output directory if the directory name already exists.
    #[clap(long, default_value_t = false)]
    overwrite: bool,

    /// Path to CFB file to dump
    input: ClioPath,

    /// Path to dump destination
    #[arg(value_parser = clap::value_parser!(ClioPath), default_value_t = ClioPath::new(std::env::current_dir().unwrap()).unwrap())]
    output: ClioPath,
}
fn main() {
    let cli = Args::parse();
    let mut file = std::fs::File::open(cli.input.to_path_buf()).unwrap();
    let header =
        cfb::Header::read_from(&mut file, cfb::Validation::Strict).unwrap();
    print!("{header:#?}")
}
