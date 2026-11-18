// Types for the Pinakes JavaScript interface. References are CSL-JSON items; fields Pinakes
// does not know are kept and written back.

export type FormatName =
  | "ris"
  | "bibtex"
  | "biblatex"
  | "medline"
  | "pubmed-xml"
  | "csl-json"
  | "wos"
  | "wos-tab"
  | "csv"
  | "hayagriva";

/** Formats Pinakes writes. */
export type WritableFormat = "ris" | "bibtex" | "biblatex" | "csl-json" | "hayagriva";

/**
 * A CSL-JSON name. `{ literal: "others" }` stands for the names a list leaves out (BibTeX's
 * `and others`, RIS's `et al.`). Members Pinakes does not model (`static-ordering`,
 * `comma-suffix`, ...) are kept and written back.
 */
export interface CslName {
  family?: string;
  given?: string;
  "non-dropping-particle"?: string;
  "dropping-particle"?: string;
  suffix?: string;
  literal?: string;
  [field: string]: unknown;
}

/** A CSL-JSON date. Members Pinakes does not model are kept and written back. */
export interface CslDate {
  "date-parts"?: (number | string)[][];
  literal?: string;
  season?: number | string;
  circa?: boolean | string | number;
  raw?: string;
  [field: string]: unknown;
}

/**
 * A CSL-JSON item. What CSL has no variable for (arXiv ID, accession number, database, the
 * fields a source format had that Pinakes did not map) goes under `custom`, as CSL 1.0.2 allows.
 */
export interface Reference {
  id: string;
  type: string;
  "citation-key"?: string;
  title?: string;
  author?: CslName[];
  editor?: CslName[];
  issued?: CslDate;
  accessed?: CslDate;
  "container-title"?: string;
  volume?: string;
  issue?: string;
  page?: string;
  publisher?: string;
  DOI?: string;
  PMID?: string;
  PMCID?: string;
  ISBN?: string;
  ISSN?: string;
  URL?: string;
  abstract?: string;
  keyword?: string;
  note?: string;
  custom?: {
    arxiv?: string;
    "accession-number"?: string;
    database?: string;
    "publication-type"?: string[];
    "source-fields"?: { format: string; name: string; value: string }[];
    [field: string]: unknown;
  };
  [field: string]: unknown;
}

export interface Diagnostic {
  severity: "info" | "warning" | "error";
  /** A stable code such as `ris.missing-er` or `id.invalid-doi`. */
  code: string;
  message: string;
  /** 1-based line in the decoded input. */
  line: number | null;
  /** 0-based index of the record in `references`. */
  record: number | null;
}

export interface Library {
  /** The format read: the one asked for, or the one recognised. */
  format: FormatName | null;
  references: Reference[];
  diagnostics: Diagnostic[];
}

export type IdKind = "doi" | "pmid" | "pmcid" | "isbn" | "issn" | "arxiv";

export type Reason =
  | { kind: "identifier"; identifier: IdKind; value: string; text: string }
  | { kind: "title"; score: number; text: string }
  | { kind: "year"; year: number; text: string }
  | { kind: "first-author"; family: string; text: string };

/**
 * How two records relate. Only `duplicate` joins records into groups; the others are shown so
 * the caller can decide.
 * - `version`: a preprint and its journal version.
 * - `notice`: a correction, retraction or expression of concern and the record it is about.
 * - `comment`: a comment, letter or reply and the article it answers.
 * - `part`: a chapter or conference paper and the book or proceedings that holds it.
 * - `translation`: the same work in two languages.
 */
export type Relation = "duplicate" | "version" | "notice" | "comment" | "part" | "translation";

export interface Candidate {
  /** Index of the first record; `a < b`. */
  a: number;
  b: number;
  relation: Relation;
  /** 100 for an identifier match, otherwise the title similarity. */
  score: number;
  /** Whether an identifier alone made this a duplicate. */
  exact: boolean;
  reasons: Reason[];
}

export interface Group {
  members: number[];
  exact: boolean;
}

export interface Duplicates {
  pairs: Candidate[];
  groups: Group[];
}

export interface FormatInfo {
  name: FormatName;
  extension: string;
  read: boolean;
  write: boolean;
}

export interface Pinakes {
  /** Read a file's bytes or text. Never throws for bad input; problems are diagnostics. */
  read(input: Uint8Array | string, options?: { format?: FormatName }): Library;
  /** Write references. Throws for a format Pinakes only reads. */
  write(references: Reference | Reference[], format: WritableFormat): string;
  /** Put identifiers and page ranges in normal form. `read` has already done this. */
  normalise(references: Reference | Reference[]): {
    references: Reference[];
    diagnostics: Diagnostic[];
  };
  /** Give references without a citation key one; with `replace`, give every reference a new one. */
  assignKeys(references: Reference[], options?: { replace?: boolean }): Reference[];
  /** The key a reference would get with no collisions. */
  keyFor(reference: Reference): string;
  /** Propose records that are the same publication. The caller merges. */
  findDuplicates(references: Reference[]): Duplicates;
  /** An identifier's normal form. Throws if it is not one. */
  normaliseId(kind: IdKind, value: string): string;
  /** Recognise any identifier in a string. */
  detectId(value: string): { kind: IdKind; value: string } | null;
  formats(): FormatInfo[];
}

export declare const read: Pinakes["read"];
export declare const write: Pinakes["write"];
export declare const normalise: Pinakes["normalise"];
export declare const assignKeys: Pinakes["assignKeys"];
export declare const keyFor: Pinakes["keyFor"];
export declare const findDuplicates: Pinakes["findDuplicates"];
export declare const normaliseId: Pinakes["normaliseId"];
export declare const detectId: Pinakes["detectId"];
export declare const formats: Pinakes["formats"];
declare const pinakes: Pinakes;
export default pinakes;
