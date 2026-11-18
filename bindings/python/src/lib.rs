//! The native half of the Python package. Values cross as JSON text, which
//! `python/pinakes/__init__.py` turns into dicts and lists: references as CSL-JSON items,
//! everything else in the shapes `pinakes` serialises them to. That module is the public
//! interface; these functions are its plumbing.

use pinakes::{Format, IdKind, Reference};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

fn format(name: &str) -> PyResult<Format> {
    name.parse::<Format>()
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

fn json<T: serde::Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v).expect("Pinakes values always serialise")
}

/// CSL-JSON items from the caller; an error from the CSL-JSON reader is raised.
fn items(text: &str) -> PyResult<Vec<Reference>> {
    let lib = pinakes::read_str(text, Some(Format::CslJson));
    match lib
        .diagnostics
        .iter()
        .find(|d| d.severity == pinakes::Severity::Error)
    {
        Some(d) => Err(PyValueError::new_err(format!(
            "references are not valid CSL-JSON: {d}"
        ))),
        None => Ok(lib.references),
    }
}

#[pyfunction]
#[pyo3(signature = (data, format_name=None))]
fn read_json(py: Python<'_>, data: &[u8], format_name: Option<&str>) -> PyResult<String> {
    let f = format_name.map(format).transpose()?;
    let data = data.to_vec();
    Ok(py.detach(move || json(&pinakes::read(&data, f))))
}

#[pyfunction]
fn write_json(references: &str, format_name: &str) -> PyResult<String> {
    pinakes::write(&items(references)?, format(format_name)?)
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn normalise_json(references: &str) -> String {
    json(&pinakes::read_str(references, Some(Format::CslJson)))
}

#[pyfunction]
fn assign_keys_json(references: &str, replace: bool) -> PyResult<String> {
    let mut refs = items(references)?;
    pinakes::assign_keys(&mut refs, replace);
    Ok(json(&refs))
}

#[pyfunction]
fn key_for_json(reference: &str) -> PyResult<String> {
    Ok(items(reference)?
        .first()
        .map(pinakes::key_for)
        .unwrap_or_default())
}

#[pyfunction]
fn find_duplicates_json(py: Python<'_>, references: &str) -> PyResult<String> {
    let refs = items(references)?;
    Ok(py.detach(move || json(&pinakes::find_duplicates(&refs))))
}

#[pyfunction]
fn normalise_id(kind: &str, value: &str) -> PyResult<String> {
    let k = IdKind::from_name(kind).ok_or_else(|| {
        PyValueError::new_err(format!(
            "unknown identifier kind {kind:?}; the kinds are doi, pmid, pmcid, isbn, issn and arxiv"
        ))
    })?;
    pinakes::id::normalise(k, value).map_err(|e| PyValueError::new_err(e.to_string()))
}

#[pyfunction]
fn detect_id(value: &str) -> Option<(&'static str, String)> {
    pinakes::id::detect(value).map(|(k, v)| (k.name(), v))
}

#[pyfunction]
fn formats_json() -> String {
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

#[pymodule]
fn _pinakes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(read_json, m)?)?;
    m.add_function(wrap_pyfunction!(write_json, m)?)?;
    m.add_function(wrap_pyfunction!(normalise_json, m)?)?;
    m.add_function(wrap_pyfunction!(assign_keys_json, m)?)?;
    m.add_function(wrap_pyfunction!(key_for_json, m)?)?;
    m.add_function(wrap_pyfunction!(find_duplicates_json, m)?)?;
    m.add_function(wrap_pyfunction!(normalise_id, m)?)?;
    m.add_function(wrap_pyfunction!(detect_id, m)?)?;
    m.add_function(wrap_pyfunction!(formats_json, m)?)?;
    Ok(())
}
