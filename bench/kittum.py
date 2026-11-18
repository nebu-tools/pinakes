"""Time kittum's RIS reader and duplicate finder on the file the Pinakes benchmark writes.

kittum is not a dependency. Take its server source from a git archive, outside any checkout:

    git -C ../kittum archive origin/main apps/server | tar -x -C /tmp/kittum
    uv venv /tmp/kittum-venv && VIRTUAL_ENV=/tmp/kittum-venv uv pip install rispy rapidfuzz pydantic rfc8785
    /tmp/kittum-venv/bin/python -I bench/kittum.py /tmp/kittum/apps/server/src fixtures/generated/bench.ris 500 1000

It reads the whole file, then finds duplicates among the first N records for each N given.
kittum compares every pair, so N above a few thousand takes hours. With --groups=PATH it also
writes kittum's groups for the last N as JSON lists of record indices, ascending, which can be
compared with the `groups` that `pinakes dedupe --json` prints for the same records.

kittum's package __init__ modules import its web and database layers, so this script loads the
two modules it needs directly and stubs the rest.
"""

import hashlib
import json
import sys
import time
import types


def load(src):
    sys.path.insert(0, src)
    for name in ("kittum.search", "kittum.corpus"):
        module = types.ModuleType(name)
        module.__path__ = [f"{src}/kittum/{name.split('.')[1]}"]
        sys.modules[name] = module
    sys.modules["kittum.search.pubmed"] = types.SimpleNamespace(parse_pubmed_xml=None)
    sys.modules["kittum.search.service"] = types.SimpleNamespace(record_execution=None)
    from kittum.search.imports import parse_ris
    from kittum.search.models import SourceRecord

    sys.modules["kittum.search"].SourceRecord = SourceRecord
    from kittum.corpus.dedupe import generate_link_proposals

    return parse_ris, generate_link_proposals


def main():
    src, path, *rest = sys.argv[1:]
    sizes = [int(a) for a in rest if a.isdigit()]
    groups_path = next((a.split("=", 1)[1] for a in rest if a.startswith("--groups=")), None)
    parse_ris, generate_link_proposals = load(src)

    text = open(path, encoding="utf-8").read()
    t = time.perf_counter()
    records = parse_ris(text, hashlib.sha256(text.encode()).hexdigest(), "ris")
    print(f"read     {1e3 * (time.perf_counter() - t):8.0f} ms  {len(records)} records", flush=True)

    for n in sizes:
        sample = records[:n]
        t = time.perf_counter()
        proposals = generate_link_proposals(sample)
        elapsed = time.perf_counter() - t
        same = [p for p in proposals if p.relation == "same_record"]
        pairs = n * (n - 1) // 2
        print(
            f"dedupe   {1e3 * elapsed:8.0f} ms  first {n} records, {len(same)} groups, "
            f"{1e6 * elapsed / max(pairs, 1):.0f} µs a pair",
            flush=True,
        )
        if groups_path and n == sizes[-1]:
            index = {r.occurrence_id: i for i, r in enumerate(sample)}
            groups = sorted(sorted(index[o] for o in p.occurrence_ids) for p in same)
            with open(groups_path, "w") as f:
                json.dump(groups, f)


if __name__ == "__main__":
    main()
