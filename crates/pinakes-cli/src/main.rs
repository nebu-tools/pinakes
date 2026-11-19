//! `pinakes convert`, `pinakes check` and `pinakes dedupe`.
//!
//! Every command reads the files it is given, or standard input for `-` or no file at all, and
//! prints each diagnostic to standard error as `file:line: severity code: message`. It exits 1
//! when any input had an error, and 2 when a file could not be opened or written. `convert`
//! also prints, always, each value the output format could not hold as it was; with
//! `--strict` any of those makes it exit 1 too.

use clap::{Args, Parser, Subcommand};
use pinakes::{Format, Library, Reference, Severity};
use serde_json::json;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "pinakes",
    version,
    about = "Read, write and de-duplicate bibliographic files"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read files in any format and write them in one.
    Convert {
        #[command(flatten)]
        input: Input,
        /// The format to write: ris, bibtex, biblatex, csl-json or hayagriva.
        #[arg(short, long)]
        to: Format,
        /// Write here instead of to standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Give every record without a citation key one, as `author + year + title word`.
        #[arg(long)]
        keys: bool,
        /// Replace every citation key with a generated one.
        #[arg(long)]
        rekey: bool,
        /// Exit 1 when the output format cannot hold any value as it was.
        #[arg(long)]
        strict: bool,
    },
    /// Report what Pinakes makes of files, without writing them.
    Check {
        #[command(flatten)]
        input: Input,
        /// Print one JSON object per file instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Propose records, across all the files, that are the same publication.
    Dedupe {
        #[command(flatten)]
        input: Input,
        /// Print the candidates and groups as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct Input {
    /// Files to read; `-` or none reads standard input.
    files: Vec<PathBuf>,
    /// Read every file as this format instead of recognising it.
    #[arg(short, long)]
    from: Option<Format>,
    /// Print warnings and notes as well as errors.
    #[arg(short, long)]
    verbose: bool,
}

/// One file read.
struct Source {
    name: String,
    lib: Library,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("pinakes: {e}");
            ExitCode::from(2)
        }
    }
}

/// Ok(false) when an input had an error.
fn run(cli: Cli) -> Result<bool, String> {
    match cli.command {
        Command::Convert {
            input,
            to,
            output,
            keys,
            rekey,
            strict,
        } => {
            if !to.can_write() {
                return Err(format!("{}", pinakes::write(&[], to).unwrap_err()));
            }
            let sources = read_all(&input)?;
            let clean = report(&sources, input.verbose);
            let mut refs: Vec<Reference> =
                sources.into_iter().flat_map(|s| s.lib.references).collect();
            if keys || rekey {
                pinakes::assign_keys(&mut refs, rekey);
            }
            let written = pinakes::write(&refs, to).map_err(|e| e.to_string())?;
            let name = output
                .as_ref()
                .map_or("<stdout>".to_string(), |p| p.display().to_string());
            print_diagnostics(&name, &written.diagnostics, true);
            let faithful = !strict || written.diagnostics.is_empty();
            let text = written.text;
            match output {
                Some(path) => std::fs::write(&path, text)
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?,
                None => io::stdout()
                    .write_all(text.as_bytes())
                    .map_err(|e| format!("cannot write: {e}"))?,
            }
            Ok(clean && faithful)
        }
        Command::Check { input, json } => {
            let sources = read_all(&input)?;
            if json {
                for s in &sources {
                    let line = json!({
                        "file": s.name,
                        "format": s.lib.format,
                        "records": s.lib.references.len(),
                        "diagnostics": s.lib.diagnostics,
                    });
                    println!("{line}");
                }
                return Ok(sources.iter().all(|s| !s.lib.has_errors()));
            }
            let clean = report(&sources, true);
            for s in &sources {
                let count = |sev| {
                    s.lib
                        .diagnostics
                        .iter()
                        .filter(|d| d.severity == sev)
                        .count()
                };
                println!(
                    "{}: {} as {}; {}, {}",
                    s.name,
                    plural(s.lib.references.len(), "record"),
                    s.lib.format.map_or("an unknown format", Format::name),
                    plural(count(Severity::Error), "error"),
                    plural(count(Severity::Warning), "warning"),
                );
            }
            Ok(clean)
        }
        Command::Dedupe { input, json } => {
            let sources = read_all(&input)?;
            let clean = report(&sources, input.verbose);
            // Each record is shown as file#n, n counting from 1 within its file.
            let mut labels = Vec::new();
            let mut refs = Vec::new();
            for s in sources {
                for (i, r) in s.lib.references.into_iter().enumerate() {
                    labels.push((s.name.clone(), i + 1));
                    refs.push(r);
                }
            }
            let dups = pinakes::find_duplicates(&refs);
            if json {
                let records: Vec<_> = labels
                    .iter()
                    .zip(&refs)
                    .map(|((file, n), r)| json!({"file": file, "number": n, "id": r.id, "title": r.title}))
                    .collect();
                println!(
                    "{}",
                    json!({"records": records, "pairs": dups.pairs, "groups": dups.groups})
                );
                return Ok(clean);
            }
            let label = |i: usize| format!("{}#{}", labels[i].0, labels[i].1);
            let describe = |i: usize| {
                let r = &refs[i];
                let year = r.year().map(|y| format!(" ({y})")).unwrap_or_default();
                format!(
                    "{}  {}{year}",
                    label(i),
                    r.title.as_deref().unwrap_or("[no title]")
                )
            };
            for p in &dups.pairs {
                let reasons: Vec<String> = p.reasons.iter().map(|r| r.describe()).collect();
                println!(
                    "{} {:.0}: {}",
                    p.relation.name(),
                    p.score,
                    reasons.join(", ")
                );
                println!("  {}", describe(p.a));
                println!("  {}", describe(p.b));
            }
            for g in &dups.groups {
                let members: Vec<String> = g.members.iter().map(|&i| label(i)).collect();
                let kind = if g.exact { "exact group" } else { "group" };
                println!("{kind}: {}", members.join(" "));
            }
            eprintln!(
                "{}, {}, {}",
                plural(refs.len(), "record"),
                plural(dups.pairs.len(), "candidate pair"),
                plural(dups.groups.len(), "group")
            );
            Ok(clean)
        }
    }
}

fn read_all(input: &Input) -> Result<Vec<Source>, String> {
    let stdin = [PathBuf::from("-")];
    let files = if input.files.is_empty() {
        &stdin[..]
    } else {
        &input.files[..]
    };
    files
        .iter()
        .map(|path| read_one(path, input.from))
        .collect()
}

fn read_one(path: &PathBuf, from: Option<Format>) -> Result<Source, String> {
    let (name, bytes) = if path.as_os_str() == "-" {
        let mut b = Vec::new();
        io::stdin()
            .read_to_end(&mut b)
            .map_err(|e| format!("cannot read standard input: {e}"))?;
        ("<stdin>".to_string(), b)
    } else {
        let b = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        (path.display().to_string(), b)
    };
    let mut lib = pinakes::read(&bytes, from);
    // Content that names no format, such as a bare list, falls back to the file's extension.
    if lib.format.is_none() && lib.references.is_empty() {
        let by_ext = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Format::from_extension)
            .filter(|f| f.can_read());
        if let Some(f) = by_ext {
            lib = pinakes::read(&bytes, Some(f));
        }
    }
    Ok(Source { name, lib })
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Print diagnostics: errors always, the rest when `all`. True when there were no errors.
fn report(sources: &[Source], all: bool) -> bool {
    let mut clean = true;
    for s in sources {
        clean &= !s.lib.has_errors();
        print_diagnostics(&s.name, &s.lib.diagnostics, all);
    }
    clean
}

/// Print one file's diagnostics: errors always, the rest when `all`.
fn print_diagnostics(name: &str, diagnostics: &[pinakes::Diagnostic], all: bool) {
    let stderr = io::stderr();
    let mut err = stderr.lock();
    for d in diagnostics {
        if !all && d.severity != Severity::Error {
            continue;
        }
        let at = d.line.map(|l| format!(":{l}")).unwrap_or_default();
        let _ = writeln!(
            err,
            "{name}{at}: {} {}: {}",
            d.severity.as_str(),
            d.code,
            d.message
        );
    }
}
