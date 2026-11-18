//! The WebAssembly face of Pinakes. Values cross the boundary as JSON text, which the
//! JavaScript wrapper in `js/index.js` parses: references as CSL-JSON items, everything else in
//! the shapes `pinakes` serialises them to. The wrapper is the public interface; these exports
//! are its plumbing.

use pinakes::{Format, IdKind, Reference};
use wasm_bindgen::prelude::*;

fn format(name: &str) -> Result<Format, JsError> {
    name.parse::<Format>()
        .map_err(|e| JsError::new(&e.to_string()))
}

fn json<T: serde::Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).expect("Pinakes values always serialise")
}

/// CSL-JSON items from the caller. Anything the CSL-JSON reader calls an error, such as text
/// that is not JSON, is thrown; warnings are not, since the items were the caller's own.
fn items(text: &str) -> Result<Vec<Reference>, JsError> {
    let lib = pinakes::read_str(text, Some(Format::CslJson));
    match lib
        .diagnostics
        .iter()
        .find(|d| d.severity == pinakes::Severity::Error)
    {
        Some(d) => Err(JsError::new(&format!(
            "references are not valid CSL-JSON: {d}"
        ))),
        None => Ok(lib.references),
    }
}

#[wasm_bindgen(js_name = readJson)]
pub fn read_json(bytes: &[u8], format_name: Option<String>) -> Result<String, JsError> {
    let f = format_name.as_deref().map(format).transpose()?;
    Ok(json(&pinakes::read(bytes, f)))
}

#[wasm_bindgen(js_name = writeJson)]
pub fn write_json(references: &str, format_name: &str) -> Result<String, JsError> {
    pinakes::write(&items(references)?, format(format_name)?)
        .map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen(js_name = assignKeysJson)]
pub fn assign_keys_json(references: &str, replace: bool) -> Result<String, JsError> {
    let mut refs = items(references)?;
    pinakes::assign_keys(&mut refs, replace);
    Ok(json(&refs))
}

#[wasm_bindgen(js_name = keyForJson)]
pub fn key_for_json(reference: &str) -> Result<String, JsError> {
    let refs = items(reference)?;
    Ok(refs.first().map(pinakes::key_for).unwrap_or_default())
}

#[wasm_bindgen(js_name = normaliseJson)]
pub fn normalise_json(references: &str) -> Result<String, JsError> {
    // Reading CSL-JSON normalises; report what it found.
    let lib = pinakes::read_str(references, Some(Format::CslJson));
    Ok(json(&lib))
}

#[wasm_bindgen(js_name = findDuplicatesJson)]
pub fn find_duplicates_json(references: &str) -> Result<String, JsError> {
    Ok(json(&pinakes::find_duplicates(&items(references)?)))
}

/// The normal form of an identifier of a named kind, or an error saying why it is not one.
#[wasm_bindgen(js_name = normaliseId)]
pub fn normalise_id(kind: &str, value: &str) -> Result<String, JsError> {
    let k = IdKind::from_name(kind).ok_or_else(|| {
        JsError::new(&format!(
            "unknown identifier kind {kind:?}; the kinds are doi, pmid, pmcid, isbn, issn and arxiv"
        ))
    })?;
    pinakes::id::normalise(k, value).map_err(|e| JsError::new(&e.to_string()))
}

/// `[kind, value]` for any identifier recognised in the text, or an empty string.
#[wasm_bindgen(js_name = detectIdJson)]
pub fn detect_id_json(value: &str) -> String {
    match pinakes::id::detect(value) {
        Some((k, v)) => json(&(k, v)),
        None => String::new(),
    }
}

#[wasm_bindgen(js_name = formatsJson)]
pub fn formats_json() -> String {
    let list: Vec<_> = Format::ALL
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name(),
                "extension": f.extension(),
                "read": f.can_read(),
                "write": f.can_write(),
            })
        })
        .collect();
    json(&list)
}
