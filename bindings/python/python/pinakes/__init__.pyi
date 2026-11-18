from typing import Any, Iterable, Literal, Mapping, Optional, TypedDict, Union

FormatName = Literal[
    "ris",
    "bibtex",
    "biblatex",
    "medline",
    "pubmed-xml",
    "csl-json",
    "wos",
    "wos-tab",
    "csv",
    "hayagriva",
]
WritableFormat = Literal["ris", "bibtex", "biblatex", "csl-json", "hayagriva"]
IdKind = Literal["doi", "pmid", "pmcid", "isbn", "issn", "arxiv"]
# How two records relate. Only "duplicate" joins records into groups.
Relation = Literal["duplicate", "version", "notice", "comment", "part", "translation"]

# A CSL-JSON item. What CSL has no variable for goes under "custom". Members Pinakes does not
# model, in the item and in its names and dates, are kept and written back. In a name list,
# {"literal": "others"} stands for the names left out (BibTeX's "and others", RIS's "et al.").
Reference = dict[str, Any]
References = Union[Mapping[str, Any], Iterable[Mapping[str, Any]]]

class Diagnostic(TypedDict):
    severity: Literal["info", "warning", "error"]
    code: str
    message: str
    line: Optional[int]
    record: Optional[int]

class Library(TypedDict):
    format: Optional[FormatName]
    references: list[Reference]
    diagnostics: list[Diagnostic]

class Normalised(TypedDict):
    references: list[Reference]
    diagnostics: list[Diagnostic]

class Reason(TypedDict, total=False):
    kind: Literal["identifier", "title", "year", "first-author"]
    text: str
    identifier: IdKind
    value: str
    score: float
    year: int
    family: str

class Candidate(TypedDict):
    a: int
    b: int
    relation: Relation
    score: float
    exact: bool
    reasons: list[Reason]

class Group(TypedDict):
    members: list[int]
    exact: bool

class Duplicates(TypedDict):
    pairs: list[Candidate]
    groups: list[Group]

class FoundId(TypedDict):
    kind: IdKind
    value: str

class FormatInfo(TypedDict):
    name: FormatName
    extension: str
    read: bool
    write: bool

def read(data: Union[bytes, str], format: Optional[FormatName] = None) -> Library: ...
def write(references: References, format: WritableFormat) -> str: ...
def normalise(references: References) -> Normalised: ...
def assign_keys(references: References, replace: bool = False) -> list[Reference]: ...
def key_for(reference: Mapping[str, Any]) -> str: ...
def find_duplicates(references: References) -> Duplicates: ...
def normalise_id(kind: IdKind, value: str) -> str: ...
def detect_id(value: str) -> Optional[FoundId]: ...
def formats() -> list[FormatInfo]: ...
