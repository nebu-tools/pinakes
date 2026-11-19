//! Generate a large RIS file and time reading it and finding its duplicates.
//!
//! ```sh
//! cargo run --release -p pinakes --example bench -- 100000 fixtures/generated/bench.ris
//! cargo run --release -p pinakes --example bench -- 100000 fixtures/generated/hard.ris adversarial
//! ```
//!
//! The file is written once, then read from disk. One record in twenty is a copy of an earlier
//! one: half keep the DOI and change the title's case and punctuation, half drop every
//! identifier and keep the title, year and first author. Records are journal articles in three
//! export dialects (EndNote, Ovid, Zotero). Titles are drawn from a fixed list of ecology and
//! astronomy words and a long tail of made-up ones. The generator is seeded, so every run
//! writes the same file.
//!
//! With `adversarial`, 20,000 records that defeat blocking are added: 10,000 titled
//! "Editorial", by the same few authors over the same years, a tenth without an author or year;
//! and 10,000 conference abstracts whose titles differ only in an abstract number, a week count
//! and one word, so every pair scores above 90 and the rules must turn most of them away.

use std::fmt::Write as _;
use std::time::Instant;

const WORDS: &[&str] = &[
    "moth",
    "dusk",
    "forest",
    "edge",
    "tide",
    "pool",
    "comet",
    "orbit",
    "lichen",
    "glacier",
    "pollen",
    "meadow",
    "heron",
    "nebula",
    "spectra",
    "dune",
    "reef",
    "coral",
    "sediment",
    "beetle",
    "canopy",
    "aurora",
    "quasar",
    "estuary",
    "fern",
    "migration",
    "parallax",
    "wetland",
    "basalt",
    "plankton",
    "eclipse",
    "moss",
    "kelp",
    "owl",
    "nocturnal",
    "survey",
    "variable",
    "star",
    "dwarf",
    "galaxy",
    "soil",
    "river",
    "delta",
    "grazing",
    "fire",
    "succession",
    "drought",
    "photometry",
    "meteor",
    "transit",
];
const FAMILIES: &[&str] = &[
    "Adeyemi",
    "Lind",
    "Ødegård",
    "García Márquez",
    "Nakamura",
    "van der Berg",
    "Kowalczyk",
    "O'Brien",
    "Haddad",
    "Chen",
    "Müller",
    "Silva",
    "Szymanski",
    "Adeyemi",
    "Rossi",
    "Kim",
];
const GIVEN: &[&str] = &[
    "Ada", "B.", "Chiara", "D. E.", "Emeka", "Fumiko", "G", "Hana",
];
const JOURNALS: &[&str] = &[
    "Journal of Field Ecology",
    "Monthly Notes on Variable Stars",
    "Wetland Studies",
    "Annals of Small Bodies",
];

/// A small, fixed pseudo-random sequence (xorshift), so every run writes the same file.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

struct Rec {
    title: String,
    authors: Vec<String>,
    year: u32,
    journal: &'static str,
    doi: Option<String>,
    an: Option<String>,
}

const SYLLABLES: &[&str] = &[
    "ka", "lo", "mi", "ner", "os", "pha", "qui", "ra", "sel", "tu", "ve", "xan", "yo", "zer",
    "bri", "cto", "dal", "fen", "gor", "hil",
];

/// A title word. Word frequencies fall off the way they do in real titles: the fixed list is
/// common, and 8000 made-up words from three syllables form a long tail.
fn word(rng: &mut Rng) -> String {
    let u = (rng.next() % 1_000_000) as f64 / 1e6;
    let rank = (8050.0 * u * u * u) as usize;
    if rank < WORDS.len() {
        return WORDS[rank].to_string();
    }
    let r = rank - WORDS.len();
    [r % 20, r / 20 % 20, r / 400 % 20]
        .iter()
        .map(|&i| SYLLABLES[i])
        .collect()
}

fn title(rng: &mut Rng) -> String {
    let n = 6 + (rng.next() % 8) as usize;
    let mut words: Vec<String> = (0..n).map(|_| word(rng)).collect();
    words.insert(2, format!("{}", rng.next() % 1000));
    let mut t = words.join(" ");
    t[..1].make_ascii_uppercase();
    t
}

fn write_record(out: &mut String, r: &Rec, dialect: u64, rng: &mut Rng) {
    let _ = writeln!(out, "TY  - JOUR");
    match dialect {
        // EndNote: T1, A1, JF, Y1 with slashes.
        0 => {
            let _ = writeln!(out, "T1  - {}", r.title);
            for a in &r.authors {
                let _ = writeln!(out, "A1  - {a}");
            }
            let _ = writeln!(out, "JF  - {}", r.journal);
            let _ = writeln!(out, "Y1  - {}///", r.year);
        }
        // Ovid/Embase: wrapped title, DB line.
        1 => {
            let (a, b) = r.title.split_at(r.title.len() / 2);
            let b = b.trim_start();
            let _ = writeln!(out, "TI  - {a}\n      {b}");
            for a in &r.authors {
                let _ = writeln!(out, "AU  - {a}");
            }
            let _ = writeln!(out, "JO  - {}", r.journal);
            let _ = writeln!(out, "PY  - {}", r.year);
            let _ = writeln!(out, "DB  - Embase");
        }
        // Zotero and Mendeley: the plain tags.
        _ => {
            let _ = writeln!(out, "TI  - {}", r.title);
            for a in &r.authors {
                let _ = writeln!(out, "AU  - {a}");
            }
            let _ = writeln!(out, "T2  - {}", r.journal);
            let _ = writeln!(out, "PY  - {}", r.year);
            let _ = writeln!(out, "DA  - {}/{:02}/01", r.year, 1 + rng.next() % 12);
        }
    }
    let _ = writeln!(out, "VL  - {}", 1 + rng.next() % 60);
    let sp = 1 + rng.next() % 900;
    let _ = writeln!(out, "SP  - {sp}\nEP  - {}", sp + 1 + rng.next() % 20);
    if let Some(d) = &r.doi {
        if dialect == 0 {
            let _ = writeln!(out, "DO  - https://doi.org/{d}");
        } else {
            let _ = writeln!(out, "DO  - {d}");
        }
    }
    if let Some(an) = &r.an {
        let _ = writeln!(out, "AN  - {an}");
    }
    let _ = writeln!(
        out,
        "AB  - A study of the {} near the {}.\nKW  - {}\nKW  - {}",
        rng.pick(WORDS),
        rng.pick(WORDS),
        rng.pick(WORDS),
        rng.pick(WORDS)
    );
    let _ = writeln!(out, "ER  - \n");
}

fn adversarial(out: &mut String, rng: &mut Rng) {
    for i in 0..10_000u64 {
        let rec = Rec {
            title: "Editorial".into(),
            authors: if i % 10 == 0 {
                vec![]
            } else {
                vec![format!("{}, {}", rng.pick(FAMILIES), rng.pick(GIVEN))]
            },
            year: 1990 + (rng.next() % 35) as u32,
            journal: rng.pick(JOURNALS),
            doi: None,
            an: None,
        };
        let mut text = String::new();
        write_record(&mut text, &rec, rng.next() % 3, rng);
        if i % 10 == 5 {
            // No year either.
            text = text
                .lines()
                .filter(|l| !l.starts_with("PY") && !l.starts_with("Y1") && !l.starts_with("DA"))
                .map(|l| format!("{l}\n"))
                .collect();
        }
        out.push_str(&text);
    }
    for i in 0..10_000u64 {
        let rec = Rec {
            title: format!(
                "Abstract {}: outcomes of {} counts at the forest edge in a {}-week field survey",
                1000 + i,
                rng.pick(WORDS),
                4 + rng.next() % 3 * 4
            ),
            authors: vec![format!("{}, {}", rng.pick(FAMILIES), rng.pick(GIVEN))],
            year: 2020 + (rng.next() % 2) as u32,
            journal: "Annals of Small Bodies",
            doi: None,
            an: None,
        };
        write_record(out, &rec, rng.next() % 3, rng);
    }
}

fn generate(n: usize) -> (String, usize) {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut made: Vec<Rec> = Vec::with_capacity(n);
    let mut out = String::with_capacity(n * 600);
    let mut copies = 0;
    for i in 0..n {
        let rec = if i > 100 && rng.next() % 20 == 0 {
            copies += 1;
            let src = &made[(rng.next() % made.len() as u64) as usize];
            let keep_doi = rng.next() % 2 == 0 && src.doi.is_some();
            Rec {
                title: if keep_doi {
                    src.title.to_uppercase() + "."
                } else {
                    src.title.replace(' ', "  ") + ":"
                },
                authors: src.authors.clone(),
                year: src.year,
                journal: src.journal,
                doi: if keep_doi { src.doi.clone() } else { None },
                an: None,
            }
        } else {
            let k = 1 + (rng.next() % 6) as usize;
            Rec {
                title: title(&mut rng),
                authors: (0..k)
                    .map(|_| format!("{}, {}", rng.pick(FAMILIES), rng.pick(GIVEN)))
                    .collect(),
                year: 1990 + (rng.next() % 35) as u32,
                journal: rng.pick(JOURNALS),
                doi: (rng.next() % 10 < 7).then(|| format!("10.5555/{i:07}")),
                an: (rng.next() % 4 == 0).then(|| format!("{}", 30_000_000 + i)),
            }
        };
        write_record(&mut out, &rec, rng.next() % 3, &mut rng);
        made.push(rec);
    }
    if std::env::args().nth(3).as_deref() == Some("adversarial") {
        adversarial(&mut out, &mut rng);
    }
    (out, copies)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let path = args
        .next()
        .unwrap_or_else(|| "fixtures/generated/bench.ris".into());
    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir).expect("create the output directory");
    }
    let (text, copies) = generate(n);
    std::fs::write(&path, &text).expect("write the RIS file");
    eprintln!(
        "wrote {path}: {n} records, {copies} copies, {:.1} MB",
        text.len() as f64 / 1e6
    );
    drop(text);

    let bytes = std::fs::read(&path).expect("read the RIS file back");
    let t = Instant::now();
    let lib = pinakes::read(&bytes, None);
    let read_time = t.elapsed();
    drop(bytes);
    let after_read = peak();
    let t = Instant::now();
    let dups = pinakes::find_duplicates(&lib.references);
    let dedupe_time = t.elapsed();
    let exact = dups.pairs.iter().filter(|p| p.exact()).count();
    println!(
        "read     {:>8.0} ms  {} records, {} diagnostics",
        read_time.as_secs_f64() * 1e3,
        lib.references.len(),
        lib.diagnostics.len()
    );
    println!(
        "dedupe   {:>8.0} ms  {} pairs ({exact} by identifier), {} groups",
        dedupe_time.as_secs_f64() * 1e3,
        dups.pairs.len(),
        dups.groups.len()
    );
    if let (Some(read), Some(all)) = (after_read, peak()) {
        println!("peak     {read} kB after reading, {all} kB after deduplicating");
    }
}

/// The process's peak resident memory in kB, where the system reports it (Linux).
fn peak() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    line.trim_start_matches("VmHWM:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()
}
