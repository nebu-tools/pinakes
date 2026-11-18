// Runs under `node --test test/` and `bun test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import pinakes, {
  read,
  write,
  normalise,
  assignKeys,
  keyFor,
  findDuplicates,
  normaliseId,
  detectId,
  formats,
} from "../js/index.js";

const fixture = (path) => readFileSync(new URL(`../../../fixtures/${path}`, import.meta.url));

const RIS = "TY  - JOUR\nTI  - Moths at dusk\nAU  - Adeyemi, Ada\nPY  - 2019\nDO  - https://doi.org/10.5555/X\nER  - \n";

test("read recognises the format and normalises identifiers", () => {
  const lib = read(RIS);
  assert.equal(lib.format, "ris");
  assert.equal(lib.references.length, 1);
  assert.equal(lib.references[0].DOI, "10.5555/x");
  assert.deepEqual(lib.references[0].author, [{ family: "Adeyemi", given: "Ada" }]);
  assert.deepEqual(lib.diagnostics, []);
});

test("read takes bytes, including Windows-1252 and UTF-16 files", () => {
  const lib = read(fixture("kittum/wos_export_tab.utf16.txt"));
  assert.equal(lib.format, "wos-tab");
  assert.ok(lib.references.length > 0);
  const latin1 = read(new Uint8Array([...new TextEncoder().encode("TY  - JOUR\nTI  - Caf"), 0xe9, ...new TextEncoder().encode("\nER  - \n")]));
  assert.equal(latin1.references[0].title, "Café");
});

test("bad input gives diagnostics with lines, not exceptions", () => {
  const lib = read("TY  - JOUR\nTI  - One\nER  - \nTY  - JOUR\nTI  - Two\n");
  assert.equal(lib.references.length, 2);
  const d = lib.diagnostics.find((d) => d.code === "ris.missing-er");
  assert.equal(d.severity, "warning");
  assert.equal(d.line, 4);
  assert.equal(d.record, 1);

  const binary = read(new Uint8Array([0x50, 0x4b, 3, 4, 0, 0]));
  assert.equal(binary.diagnostics[0].code, "format.binary");
  const mismatch = read(RIS, { format: "medline" });
  assert.equal(mismatch.diagnostics[0].code, "format.mismatch");
});

test("write in every writable format, and refuse the others", () => {
  const refs = read(RIS).references;
  assert.match(write(refs, "biblatex"), /^@article\{adeyemi2019moths,/);
  assert.match(write(refs, "bibtex"), /doi += \{10\.5555\/x\}/);
  assert.match(write(refs, "ris"), /^TY {2}- JOUR\n/);
  assert.match(write(refs, "hayagriva"), /^adeyemi2019moths:\n/);
  assert.deepEqual(JSON.parse(write(refs, "csl-json")), refs);
  assert.throws(() => write(refs, "medline"), /does not write/);
  assert.throws(() => write(refs, "nonsense"), /unknown format/);
  assert.throws(() => write("{not json", "ris"), /not valid CSL-JSON/);
});

test("unknown CSL-JSON fields survive a round trip", () => {
  const item = { id: "x", type: "book", title: "Tides", "x-shelf": "B4", custom: { "my-flag": true } };
  const back = read(write([item], "csl-json")).references[0];
  assert.equal(back["x-shelf"], "B4");
  assert.equal(back.custom["my-flag"], true);
});

test("keys: generated, collision-free, and overridable", () => {
  const refs = [
    { id: "1", type: "article-journal", title: "The moths of the forest", author: [{ family: "Lind" }], issued: { "date-parts": [[2020]] } },
    { id: "2", type: "article-journal", title: "Moths of the forest edge", author: [{ family: "Lind" }], issued: { "date-parts": [[2020]] } },
    { id: "3", type: "book", title: "Kept", "citation-key": "mine" },
  ];
  assert.equal(keyFor(refs[0]), "lind2020moths");
  const keyed = assignKeys(refs);
  assert.deepEqual(keyed.map((r) => r["citation-key"]), ["lind2020moths", "lind2020mothsa", "mine"]);
  const replaced = assignKeys(refs, { replace: true });
  assert.notEqual(replaced[2]["citation-key"], "mine");
});

test("findDuplicates gives candidates with reasons and groups", () => {
  const refs = [...read(fixture("kittum/bats_central.ris")).references, ...read(fixture("kittum/bats_embase.ris")).references];
  const dups = findDuplicates(refs);
  const first = dups.pairs[0];
  assert.equal(first.a, 0);
  assert.equal(first.b, 6);
  assert.equal(first.relation, "duplicate");
  assert.equal(first.exact, true);
  assert.deepEqual(first.reasons[0], {
    kind: "identifier",
    identifier: "doi",
    value: "10.1000/bat.2019.001",
    text: "Same DOI 10.1000/bat.2019.001",
  });
  assert.ok(dups.groups.some((g) => g.members.join() === "0,6"));
});

test("identifiers", () => {
  assert.equal(normaliseId("doi", "https://doi.org/10.5555/ABC"), "10.5555/abc");
  assert.equal(normaliseId("isbn", "0-306-40615-2"), "9780306406157");
  assert.throws(() => normaliseId("pmid", "PMC123"), /not a valid PMID/);
  assert.throws(() => normaliseId("ssn", "1"), /unknown identifier kind/);
  assert.deepEqual(detectId("arXiv:2101.00001v2"), { kind: "arxiv", value: "2101.00001v2" });
  assert.equal(detectId("moths"), null);
});

test("normalise reports what it changed", () => {
  const out = normalise({ id: "a", type: "article-journal", title: "T", DOI: "doi:10.5555/Q", PMID: "PMC77" });
  assert.equal(out.references[0].DOI, "10.5555/q");
  assert.equal(out.references[0].PMCID, "PMC77");
  assert.equal(out.diagnostics[0].code, "id.pmcid-as-pmid");
});

test("formats lists what reads and writes", () => {
  const byName = Object.fromEntries(formats().map((f) => [f.name, f]));
  assert.equal(byName.ris.write, true);
  assert.equal(byName.medline.write, false);
  assert.equal(byName.hayagriva.read, false);
  assert.equal(pinakes.read, read);
});
