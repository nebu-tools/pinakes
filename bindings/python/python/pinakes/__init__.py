"""Read, write, normalise and de-duplicate bibliographic references.

References are CSL-JSON items as plain dicts. Fields Pinakes does not know are kept and
written back. Reading never raises for bad input: problems come back as diagnostics.
"""

from __future__ import annotations

import json
from typing import Any, Iterable, Mapping, Optional, Union

from . import _pinakes

__all__ = [
    "read",
    "write",
    "normalise",
    "assign_keys",
    "key_for",
    "find_duplicates",
    "normalise_id",
    "detect_id",
    "formats",
]


def _items(references: Union[Mapping[str, Any], Iterable[Mapping[str, Any]]]) -> str:
    if isinstance(references, Mapping):
        references = [references]
    return json.dumps(list(references), ensure_ascii=False)


def read(data: Union[bytes, str], format: Optional[str] = None) -> dict:
    """Read a file's bytes or text. With ``format`` unset, the format is recognised.

    Returns ``{"format", "references", "diagnostics"}``.
    """
    if isinstance(data, str):
        data = data.encode("utf-8")
    return json.loads(_pinakes.read_json(data, format))


def write(references, format: str) -> str:
    """Write references as ris, bibtex, biblatex, csl-json or hayagriva."""
    return _pinakes.write_json(_items(references), format)


def normalise(references) -> dict:
    """Put identifiers and page ranges in normal form. ``read`` has already done this.

    Returns ``{"references", "diagnostics"}``.
    """
    lib = json.loads(_pinakes.normalise_json(_items(references)))
    return {"references": lib["references"], "diagnostics": lib["diagnostics"]}


def assign_keys(references, replace: bool = False) -> list:
    """Give references without a citation key one; with ``replace``, give every one a new key."""
    return json.loads(_pinakes.assign_keys_json(_items(references), replace))


def key_for(reference) -> str:
    """The key a reference would get with no collisions."""
    return _pinakes.key_for_json(_items(reference))


def find_duplicates(references) -> dict:
    """Propose records that are the same publication. The caller merges.

    Returns ``{"pairs": [...], "groups": [...]}``.
    """
    return json.loads(_pinakes.find_duplicates_json(_items(references)))


def normalise_id(kind: str, value: str) -> str:
    """An identifier's normal form. Raises ``ValueError`` if it is not one."""
    return _pinakes.normalise_id(kind, value)


def detect_id(value: str) -> Optional[dict]:
    """Recognise any identifier in a string: ``{"kind", "value"}`` or ``None``."""
    found = _pinakes.detect_id(value)
    return None if found is None else {"kind": found[0], "value": found[1]}


def formats() -> list:
    """Every format, with whether Pinakes reads and writes it."""
    return json.loads(_pinakes.formats_json())
