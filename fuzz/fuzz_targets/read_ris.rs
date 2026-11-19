//! The ris reader never panics or hangs, whatever the bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = pinakes::read(data, Some(pinakes::Format::Ris));
});
