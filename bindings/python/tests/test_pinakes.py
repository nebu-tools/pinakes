import json
import pathlib

import pytest

import pinakes

FIXTURES = pathlib.Path(__file__).resolve().parents[3] / "fixtures"
RIS = "TY  - JOUR\nTI  - Moths at dusk\nAU  - Adeyemi, Ada\nPY  - 2019\nDO  - https://doi.org/10.5555/X\nER  - \n"


def test_read_recognises_the_format_and_normalises_identifiers():
    lib = pinakes.read(RIS)
    assert lib["format"] == "ris"
    assert lib["diagnostics"] == []
    [ref] = lib["references"]
    assert ref["DOI"] == "10.5555/x"
    assert ref["author"] == [{"family": "Adeyemi", "given": "Ada"}]


def test_read_takes_bytes_in_any_encoding():
    lib = pinakes.read((FIXTURES / "kittum/wos_export_tab.utf16.txt").read_bytes())
    assert lib["format"] == "wos-tab"
    assert lib["references"]
    latin1 = pinakes.read(b"TY  - JOUR\r\nTI  - Caf\xe9\r\nER  - \r\n")
    assert latin1["references"][0]["title"] == "Café"


def test_bad_input_gives_diagnostics_with_lines():
    lib = pinakes.read("TY  - JOUR\nTI  - One\nER  - \nTY  - JOUR\nTI  - Two\n")
    assert len(lib["references"]) == 2
    [d] = [d for d in lib["diagnostics"] if d["code"] == "ris.missing-er"]
    assert (d["severity"], d["line"], d["record"]) == ("warning", 4, 1)
    assert pinakes.read(b"%PDF-1.7 ...")["diagnostics"][0]["code"] == "format.binary"
    assert pinakes.read(RIS, format="medline")["diagnostics"][0]["code"] == "format.mismatch"
    with pytest.raises(ValueError, match="unknown format"):
        pinakes.read(RIS, format="endnote-xml")


def test_write_every_writable_format():
    refs = pinakes.read(RIS)["references"]
    assert pinakes.write(refs, "biblatex").startswith("@article{adeyemi2019moths,")
    assert pinakes.write(refs, "ris").startswith("TY  - JOUR\n")
    assert pinakes.write(refs, "hayagriva").startswith("adeyemi2019moths:\n")
    assert json.loads(pinakes.write(refs, "csl-json")) == refs
    assert "{10.5555/x}" in pinakes.write(refs[0], "bibtex")
    with pytest.raises(ValueError, match="does not write"):
        pinakes.write(refs, "medline")


def test_unknown_fields_survive():
    item = {"id": "x", "type": "book", "title": "Tides", "x-shelf": "B4", "custom": {"my-flag": True}}
    [back] = pinakes.read(pinakes.write([item], "csl-json"))["references"]
    assert back["x-shelf"] == "B4"
    assert back["custom"]["my-flag"] is True


def test_keys():
    refs = [
        {"id": "1", "type": "article-journal", "title": "The moths of the forest", "author": [{"family": "Lind"}], "issued": {"date-parts": [[2020]]}},
        {"id": "2", "type": "article-journal", "title": "Moths of the forest edge", "author": [{"family": "Lind"}], "issued": {"date-parts": [[2020]]}},
        {"id": "3", "type": "book", "title": "Kept", "citation-key": "mine"},
    ]
    assert pinakes.key_for(refs[0]) == "lind2020moths"
    keyed = pinakes.assign_keys(refs)
    assert [r["citation-key"] for r in keyed] == ["lind2020moths", "lind2020mothsa", "mine"]
    assert pinakes.assign_keys(refs, replace=True)[2]["citation-key"] != "mine"


def test_find_duplicates():
    refs = pinakes.read((FIXTURES / "kittum/bats_central.ris").read_bytes())["references"]
    refs += pinakes.read((FIXTURES / "kittum/bats_embase.ris").read_bytes())["references"]
    dups = pinakes.find_duplicates(refs)
    first = dups["pairs"][0]
    assert (first["a"], first["b"], first["relation"], first["exact"]) == (0, 6, "duplicate", True)
    assert first["reasons"][0] == {
        "kind": "identifier",
        "identifier": "doi",
        "value": "10.1000/bat.2019.001",
        "text": "Same DOI 10.1000/bat.2019.001",
    }
    assert {"members": [0, 6], "exact": True} in dups["groups"]


def test_name_and_date_members_and_others_survive():
    item = {
        "id": "x",
        "type": "book",
        "title": "Tides",
        "author": [{"family": "Lind", "given": "K", "static-ordering": True}, {"literal": "others"}],
        "issued": {"date-parts": [[2020]], "season": 2},
    }
    for fmt in ["csl-json", "bibtex"]:
        back = pinakes.read(pinakes.write([item], fmt))["references"][0]
        assert back["author"][-1] == {"literal": "others"}, fmt
    back = pinakes.read(pinakes.write([item], "csl-json"))["references"][0]
    assert back["author"][0]["static-ordering"] is True
    assert back["issued"]["season"] == 2


def test_chapter_and_book_are_a_part():
    base = {"title": "Moths at the forest edge", "DOI": "10.5555/x", "author": [{"family": "Adeyemi"}], "issued": {"date-parts": [[2019]]}}
    dups = pinakes.find_duplicates([{**base, "id": "a", "type": "chapter"}, {**base, "id": "b", "type": "book"}])
    assert dups["pairs"][0]["relation"] == "part"
    assert dups["groups"] == []


def test_identifiers():
    assert pinakes.normalise_id("doi", "https://doi.org/10.5555/ABC") == "10.5555/abc"
    assert pinakes.normalise_id("isbn", "0-306-40615-2") == "9780306406157"
    with pytest.raises(ValueError, match="not a valid PMID"):
        pinakes.normalise_id("pmid", "PMC123")
    assert pinakes.detect_id("arXiv:2101.00001v2") == {"kind": "arxiv", "value": "2101.00001v2"}
    assert pinakes.detect_id("moths") is None


def test_normalise_reports_what_it_moved():
    out = pinakes.normalise({"id": "a", "type": "article-journal", "title": "T", "DOI": "doi:10.5555/Q", "PMID": "PMC77"})
    assert out["references"][0]["DOI"] == "10.5555/q"
    assert out["references"][0]["PMCID"] == "PMC77"
    assert out["diagnostics"][0]["code"] == "id.pmcid-as-pmid"


def test_formats():
    by_name = {f["name"]: f for f in pinakes.formats()}
    assert by_name["ris"]["write"] and not by_name["medline"]["write"]
    assert not by_name["hayagriva"]["read"]
