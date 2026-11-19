//! Whatever any reader makes of the bytes, every writer's output reads back as the same number
//! of records in the writer's own format.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pinakes::Format;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, bytes)) = data.split_first() else {
        return;
    };
    let readers: Vec<Format> = Format::ALL.into_iter().filter(|f| f.can_read()).collect();
    // One value of the first byte leaves the format to the sniffer.
    let from = readers.get(pick as usize % (readers.len() + 1)).copied();
    let lib = pinakes::read(bytes, from);
    for to in Format::ALL
        .into_iter()
        .filter(|f| f.can_write() && f.can_read())
    {
        let out = pinakes::write(&lib.references, to).unwrap().text;
        let back = pinakes::read(out.as_bytes(), Some(to));
        assert_eq!(
            back.references.len(),
            lib.references.len(),
            "{to}: {:?}\n{out}",
            back.diagnostics
        );
    }
});
