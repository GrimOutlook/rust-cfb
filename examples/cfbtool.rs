use std::path::{Path, PathBuf};
use std::{env, fs, io};

use cfb::CompoundFile;
use clap::{Parser, Subcommand};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[clap(author, about, long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Concatenates and prints streams
    Cat { path: Vec<String> },

    /// Changes storage CLSIDs
    Chcls { clsid: Uuid, path: Vec<String> },

    /// Lists storage contents
    Ls {
        #[clap(short, long)]
        /// Lists in long format
        long: bool,

        #[clap(short, long)]
        /// Includes . in output
        all: bool,

        path: Vec<String>,
    },

    /// Dump a given stream by navigating to it from Root Storage.
    Dump {
        #[clap(short, long)]
        /// Dump all streams found in CFB file.
        all: bool,
        /// Path to dump destination
        path: String,
    },
}

const TABLE_PREFIX: char = '\u{4840}';
fn from_b64(value: u32) -> char {
    debug_assert!(value < 64);
    if value < 10 {
        char::from_u32(value + '0' as u32).unwrap()
    } else if value < 36 {
        char::from_u32(value - 10 + 'A' as u32).unwrap()
    } else if value < 62 {
        char::from_u32(value - 36 + 'a' as u32).unwrap()
    } else if value == 62 {
        '.'
    } else {
        '_'
    }
}

/// Decodes a stream name, and returns the decoded name and whether the stream
/// was a table.
fn decode(name: &str) -> (String, bool) {
    let mut output = String::new();
    let mut is_table = false;
    let mut chars = name.chars().peekable();
    if chars.peek() == Some(&TABLE_PREFIX) {
        is_table = true;
        chars.next();
    }
    for chr in chars {
        let value = chr as u32;
        if (0x3800..0x4800).contains(&value) {
            let value = value - 0x3800;
            output.push(from_b64(value & 0x3f));
            output.push(from_b64(value >> 6));
        } else if (0x4800..0x4840).contains(&value) {
            output.push(from_b64(value - 0x4800));
        } else {
            output.push(chr);
        }
    }
    (output, is_table)
}

fn split(path: &str) -> (PathBuf, PathBuf) {
    let mut pieces = path.splitn(2, ':');
    if let Some(piece1) = pieces.next() {
        if let Some(piece2) = pieces.next() {
            (PathBuf::from(piece1), PathBuf::from(piece2))
        } else {
            (PathBuf::from(piece1), PathBuf::new())
        }
    } else {
        (PathBuf::new(), PathBuf::new())
    }
}

fn list_entry(name: &str, entry: &cfb::Entry, long: bool) {
    if !long {
        println!("{}", entry.name());
        return;
    }
    let length = if entry.len() >= 10_000_000_000 {
        format!("{} GB", entry.len() / (1 << 30))
    } else if entry.len() >= 100_000_000 {
        format!("{} MB", entry.len() / (1 << 20))
    } else if entry.len() >= 1_000_000 {
        format!("{} kB", entry.len() / (1 << 10))
    } else {
        format!("{} B ", entry.len())
    };
    let last_modified = {
        let timestamp = entry.created().max(entry.modified());
        let datetime = OffsetDateTime::from(timestamp);
        let (year, month, day) = datetime.to_calendar_date();
        format!("{:04}-{:02}-{:02}", year, month as u8, day)
    };
    println!(
        "{}{:08x}   {:>10}   {}   {}",
        if entry.is_storage() { '+' } else { '-' },
        entry.state_bits(),
        length,
        last_modified,
        name
    );
    if entry.is_storage() {
        println!(" {}", entry.clsid().hyphenated());
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Cat { path } => {
            for path in path {
                let (comp_path, inner_path) = split(&path);
                let mut comp = cfb::open(&comp_path).unwrap();
                let mut stream = comp.open_stream(inner_path).unwrap();
                io::copy(&mut stream, &mut io::stdout()).unwrap();
            }
        }
        Command::Chcls { clsid, path } => {
            for path in path {
                let (comp_path, inner_path) = split(&path);
                let mut comp = cfb::open(&comp_path).unwrap();
                comp.set_storage_clsid(inner_path, clsid).unwrap();
                comp.flush().unwrap();
            }
        }
        Command::Ls { long, all, path } => {
            for path in path {
                let (comp_path, inner_path) = split(&path);
                let comp = cfb::open(&comp_path).unwrap();
                let entry = comp.entry(&inner_path).unwrap();
                if entry.is_stream() {
                    list_entry(entry.name(), &entry, long);
                } else {
                    if all {
                        list_entry(".", &entry, long);
                    }
                    for subentry in comp.read_storage(&inner_path).unwrap() {
                        list_entry(subentry.name(), &subentry, long);
                    }
                }
            }
        }
        Command::Dump { path, all } => {
            let mut comp = cfb::open(&path).unwrap();
            let mut entries = comp.read_root_storage().collect::<Vec<_>>();
            if all {
                let output_dir = env::current_dir().unwrap().join("root");
                fs::create_dir(&output_dir).unwrap();
                let root_entry = &comp.root_entry();
                dump_entry_recursively(&mut comp, root_entry, &output_dir);
                return;
            }

            loop {
                for (index, subentry) in
                    entries.clone().into_iter().enumerate()
                {
                    let (name, _) = decode(subentry.name());
                    println!("[{index}] {}", name);
                }
                println!("Inspect?: ");
                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .expect("Failed to read line");
                let input = input.trim();
                if input == "q" {
                    return;
                }

                let selected_index: usize = input
                    .parse()
                    .expect("Selection was not a valid number or 'q'");
                let selection = entries
                    .get(selected_index)
                    .expect("Selected index was invalid");

                if selection.is_storage() {
                    entries = comp
                        .read_storage(selection.name())
                        .expect("Failed to read storage for selection")
                        .collect();
                } else if selection.is_stream() {
                    let mut stream = comp
                        .open_stream(selection.name())
                        .expect("Failed to read stream for selection");
                    println!("Stream dump location: ");
                    let mut input = String::new();
                    io::stdin()
                        .read_line(&mut input)
                        .expect("Failed to read line");
                    let input = input.trim();
                    println!(
                        "Dumping stream [{}] to [{}]",
                        selection.name(),
                        input
                    );
                    let mut new_file = std::fs::File::options()
                        .read(true)
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .create_new(true)
                        .open(input)
                        .expect("Failed to create new file");

                    std::io::copy(&mut stream, &mut new_file)
                        .expect("Failed to copy data from stream");
                    return;
                }
            }
        }
    }
}

fn dump_entry_recursively<T: std::io::Seek + std::io::Read>(
    comp: &mut CompoundFile<T>,
    entry: &cfb::Entry,
    output_dir: &Path,
) {
    if entry.is_root() || entry.is_storage() {
        let entries = if entry.is_root() {
            comp.read_root_storage().collect::<Vec<cfb::Entry>>()
        } else {
            comp.read_storage(entry.name())
                .unwrap()
                .collect::<Vec<cfb::Entry>>()
        };

        for subentry in entries {
            let output_dir = output_dir.join(decode(subentry.name()).0);
            fs::create_dir(output_dir.clone()).unwrap();
            dump_entry_recursively(comp, &subentry, &output_dir);
        }
        return;
    }

    let mut stream = comp
        .open_stream(entry.name())
        .expect("Failed to read stream for selection");
    let name = decode(entry.name()).0;
    let output_location = output_dir.join(format!("{}.dump", name));

    println!("Dumping stream [{}] to [{:#?}]", name, output_location);
    let mut new_file = std::fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .create_new(true)
        .open(output_location)
        .expect("Failed to create new file");

    std::io::copy(&mut stream, &mut new_file)
        .expect("Failed to copy data from stream");
}
