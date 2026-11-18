// Compiled with `tsc --noEmit` to check the declarations; never run.
import pinakes, {
  read,
  write,
  findDuplicates,
  type Reference,
  type Reason,
  type Written,
} from "pinakes";
import load from "pinakes/web";

const lib = read(new Uint8Array(), { format: "ris" });
const refs: Reference[] = lib.references;
const out: Written = write(refs, "biblatex");
const bib: string = out.text;
const lost: (string | null)[] = out.diagnostics.map((d) => d.field);
const why = (r: Reason): string => (r.kind === "identifier" ? r.identifier : r.text);
const first = findDuplicates(refs).pairs[0]?.reasons.map(why);
const arxiv: string | undefined = refs[0]?.custom?.arxiv;
void [bib, lost, first, arxiv, pinakes.keyFor(refs[0]!)];
void load().then((p) => p.formats().filter((f) => f.write));
// @ts-expect-error: MEDLINE is read only.
write(refs, "medline");
