//! PubMed XML, as PubMed's "Save, XML" and EFetch (`retmode=xml`) write it: a
//! `PubmedArticleSet` of `PubmedArticle` and `PubmedBookArticle` elements.

use crate::dates::{month_number, parse_date};
use crate::diag::Diagnostic;
use crate::formats::xml::{self, Element};
use crate::model::{Date, Name, Reference, Ymd};
use crate::names::expand_initials;
use crate::normalise::finish;

pub const FORMAT: &str = "pubmed-xml";

fn author(a: &Element) -> Option<Name> {
    if let Some(c) = a.text_of("CollectiveName") {
        return Some(Name::literal(&c));
    }
    let last = a.text_of("LastName")?;
    let given = a.text_of("ForeName").or_else(|| a.text_of("Initials").map(|i| expand_initials(&i)));
    let mut n = Name::person(&last, given.as_deref());
    n.suffix = a.text_of("Suffix");
    Some(n)
}

fn authors(list: Option<&Element>) -> Vec<Name> {
    list.map(|l| l.all("Author").filter(|a| a.attr("ValidYN") != Some("N")).filter_map(author).collect()).unwrap_or_default()
}

fn pub_date(e: Option<&Element>) -> Option<Date> {
    let e = e?;
    if let Some(md) = e.text_of("MedlineDate") {
        return parse_date(&md);
    }
    let year: i32 = e.text_of("Year")?.trim().parse().ok()?;
    let month = e.text_of("Month").and_then(|m| m.trim().parse::<u8>().ok().filter(|n| (1..=12).contains(n)).or_else(|| month_number(m.trim())));
    let day = e.text_of("Day").and_then(|d| d.trim().parse::<u8>().ok()).filter(|d| (1..=31).contains(d));
    let mut d = Date::ymd(year, month, month.and(day));
    if let Some(season) = e.text_of("Season") {
        d.raw = Some(format!("{year} {season}"));
    }
    Some(d)
}

fn abstract_text(e: Option<&Element>) -> Option<String> {
    let e = e?;
    let parts: Vec<String> = e
        .all("AbstractText")
        .filter_map(|t| {
            let text = t.text(true);
            if text.is_empty() {
                return None;
            }
            Some(match t.attr("Label") {
                Some(l) if !l.is_empty() => format!("{l}: {text}"),
                _ => text,
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

fn ids(r: &mut Reference, list: Option<&Element>) {
    let Some(list) = list else { return };
    for id in list.all("ArticleId") {
        let v = id.text(false);
        if v.is_empty() {
            continue;
        }
        match id.attr("IdType") {
            Some("doi") if r.doi.is_none() => r.doi = Some(v),
            Some("pmc") if r.pmcid.is_none() => r.pmcid = Some(v),
            Some("pubmed") if r.pmid.is_none() => r.pmid = Some(v),
            Some(kind) => r.keep(FORMAT, &format!("ArticleId/{kind}"), &v),
            None => r.keep(FORMAT, "ArticleId", &v),
        }
    }
}

fn keywords(r: &mut Reference, citation: &Element) {
    for list in citation.all("KeywordList") {
        r.keywords.extend(list.all("Keyword").map(|k| k.text(false)).filter(|k| !k.is_empty()));
    }
    if let Some(mesh) = citation.child("MeshHeadingList") {
        for h in mesh.all("MeshHeading") {
            if let Some(d) = h.text_of("DescriptorName") {
                r.keep(FORMAT, "MeshHeading", &d);
            }
        }
    }
}

fn article(pa: &Element, index: usize, diags: &mut Vec<Diagnostic>) -> Reference {
    let mut r = Reference { kind: "article-journal".into(), ..Reference::default() };
    let Some(mc) = pa.child("MedlineCitation") else {
        diags.push(Diagnostic::error("pubmed-xml.no-citation", "A PubmedArticle has no MedlineCitation; kept an empty record").at(pa.line).record(index));
        r.id = (index + 1).to_string();
        finish(&mut r, index, Some(pa.line), diags);
        return r;
    };
    r.pmid = mc.text_of("PMID");
    if let Some(a) = mc.child("Article") {
        r.title = a.child("ArticleTitle").map(|t| t.text(true)).filter(|t| !t.is_empty()).or_else(|| a.text_of("VernacularTitle"));
        if let Some(v) = a.text_of("VernacularTitle") {
            if r.title.as_deref() != Some(v.as_str()) {
                r.keep(FORMAT, "VernacularTitle", &v);
            }
        }
        r.container_title = a.text_of("Journal/Title");
        r.container_title_short = a.text_of("Journal/ISOAbbreviation").or_else(|| mc.text_of("MedlineJournalInfo/MedlineTA"));
        if let Some(j) = a.child("Journal") {
            let issns: Vec<String> = j.all("ISSN").map(|i| i.text(false)).filter(|i| !i.is_empty()).collect();
            r.issn = issns.first().cloned();
            for extra in issns.iter().skip(1) {
                r.keep(FORMAT, "ISSN", extra);
            }
        }
        r.volume = a.text_of("Journal/JournalIssue/Volume");
        r.issue = a.text_of("Journal/JournalIssue/Issue");
        r.issued = pub_date(a.find("Journal/JournalIssue/PubDate")).or_else(|| pub_date(a.child("ArticleDate")));
        if let Some(ad) = pub_date(a.child("ArticleDate")) {
            if r.issued.as_ref() != Some(&ad) {
                r.keep(FORMAT, "ArticleDate", &ad.iso().unwrap_or_default());
            }
        }
        r.page = a.text_of("Pagination/MedlinePgn").or_else(|| match (a.text_of("Pagination/StartPage"), a.text_of("Pagination/EndPage")) {
            (Some(s), Some(e)) => Some(format!("{s}-{e}")),
            (s, _) => s,
        });
        for loc in a.all("ELocationID") {
            let v = loc.text(false);
            match loc.attr("EIdType") {
                Some("doi") if r.doi.is_none() => r.doi = Some(v),
                Some(kind) => r.keep(FORMAT, &format!("ELocationID/{kind}"), &v),
                None => r.keep(FORMAT, "ELocationID", &v),
            }
        }
        r.abstract_text = abstract_text(a.child("Abstract"));
        r.author = authors(a.child("AuthorList"));
        r.language = a.text_of("Language");
        if let Some(pts) = a.child("PublicationTypeList") {
            r.publication_types = pts.all("PublicationType").map(|p| p.text(false)).filter(|p| !p.is_empty()).collect();
        }
        if let Some(banks) = a.child("DataBankList") {
            for bank in banks.all("DataBank") {
                let name = bank.text_of("DataBankName").unwrap_or_default();
                if let Some(accs) = bank.child("AccessionNumberList") {
                    for acc in accs.all("AccessionNumber") {
                        r.keep(FORMAT, &format!("DataBank/{name}"), &acc.text(false));
                    }
                }
            }
        }
    } else {
        diags.push(Diagnostic::warning("pubmed-xml.no-article", "A MedlineCitation has no Article").at(mc.line).record(index));
    }
    if let Some(c) = mc.text_of("MedlineJournalInfo/Country") {
        r.keep(FORMAT, "Country", &c);
    }
    keywords(&mut r, mc);
    ids(&mut r, pa.find("PubmedData/ArticleIdList"));
    if r.issued.is_none() {
        // A record in process may carry only its history dates.
        if let Some(h) = pa.find("PubmedData/History") {
            if let Some(d) = h.all("PubMedPubDate").find(|d| d.attr("PubStatus") == Some("pubmed")) {
                r.issued = d.text_of("Year").and_then(|y| y.parse().ok()).map(|y| Date { start: Some(Ymd { year: y, month: None, day: None }), ..Date::default() });
            }
        }
    }
    r.id = r.pmid.clone().unwrap_or_else(|| (index + 1).to_string());
    finish(&mut r, index, Some(pa.line), diags);
    r
}

fn book(pb: &Element, index: usize, diags: &mut Vec<Diagnostic>) -> Reference {
    let mut r = Reference { kind: "book".into(), ..Reference::default() };
    let doc = pb.child("BookDocument");
    if let Some(d) = doc {
        r.pmid = d.text_of("PMID");
        let book = d.child("Book");
        let chapter_title = d.child("ArticleTitle").map(|t| t.text(true)).filter(|t| !t.is_empty());
        let book_title = book.and_then(|b| b.child("BookTitle")).map(|t| t.text(true)).filter(|t| !t.is_empty());
        match chapter_title {
            Some(t) => {
                r.kind = "chapter".into();
                r.title = Some(t);
                r.container_title = book_title;
            }
            None => r.title = book_title,
        }
        if let Some(b) = book {
            r.publisher = b.text_of("Publisher/PublisherName");
            r.publisher_place = b.text_of("Publisher/PublisherLocation");
            r.issued = pub_date(b.child("PubDate"));
            r.isbn = b.all("Isbn").map(|i| i.text(false)).find(|i| !i.is_empty());
            r.edition = b.text_of("Edition");
            r.collection_title = b.text_of("CollectionTitle");
            for list in b.all("AuthorList") {
                if list.attr("Type") == Some("editors") {
                    r.editor = authors(Some(list));
                } else if r.author.is_empty() && chapter_title_is_none(d) {
                    r.author = authors(Some(list));
                }
            }
        }
        for list in d.all("AuthorList") {
            if list.attr("Type") == Some("editors") {
                r.editor = authors(Some(list));
            } else {
                r.author = authors(Some(list));
            }
        }
        r.abstract_text = abstract_text(d.child("Abstract"));
        r.language = d.text_of("Language");
        if let Some(pts) = d.child("PublicationType") {
            r.publication_types.push(pts.text(false));
        }
        ids(&mut r, d.child("ArticleIdList"));
        keywords(&mut r, d);
    } else {
        diags.push(Diagnostic::error("pubmed-xml.no-citation", "A PubmedBookArticle has no BookDocument; kept an empty record").at(pb.line).record(index));
    }
    ids(&mut r, pb.find("PubmedBookData/ArticleIdList"));
    r.id = r.pmid.clone().unwrap_or_else(|| (index + 1).to_string());
    finish(&mut r, index, Some(pb.line), diags);
    r
}

fn chapter_title_is_none(d: &Element) -> bool {
    d.child("ArticleTitle").is_none()
}

/// Read PubMed XML.
pub fn read(text: &str, diags: &mut Vec<Diagnostic>) -> Vec<Reference> {
    let doc = xml::parse(text, diags);
    let mut items: Vec<&Element> = Vec::new();
    // Articles may sit under PubmedArticleSet, or be the root themselves.
    for top in doc.elements() {
        match top.name.as_str() {
            "PubmedArticle" | "PubmedBookArticle" => items.push(top),
            _ => items.extend(top.elements().filter(|e| matches!(e.name.as_str(), "PubmedArticle" | "PubmedBookArticle"))),
        }
    }
    if items.is_empty() {
        if doc.elements().any(|e| e.name == "eSearchResult") {
            diags.push(Diagnostic::error("pubmed-xml.esearch", "This is an ESearch result (a list of PMIDs), not records; fetch them with EFetch"));
        } else if !text.trim().is_empty() {
            diags.push(Diagnostic::error("pubmed-xml.no-articles", "Found no PubmedArticle or PubmedBookArticle elements"));
        }
    }
    items
        .into_iter()
        .enumerate()
        .map(|(i, e)| if e.name == "PubmedArticle" { article(e, i, diags) } else { book(e, i, diags) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"<?xml version="1.0" ?>
<!DOCTYPE PubmedArticleSet PUBLIC "-//NLM//DTD PubMedArticle, 1st January 2024//EN" "https://dtd.nlm.nih.gov/ncbi/pubmed/out/pubmed_240101.dtd">
<PubmedArticleSet>
<PubmedArticle>
  <MedlineCitation Status="MEDLINE" Owner="NLM">
    <PMID Version="1">31000001</PMID>
    <Article PubModel="Print-Electronic">
      <Journal>
        <ISSN IssnType="Electronic">1365-2656</ISSN>
        <JournalIssue CitedMedium="Internet"><Volume>88</Volume><Issue>4</Issue>
          <PubDate><Year>2019</Year><Month>Apr</Month></PubDate></JournalIssue>
        <Title>Journal of animal ecology</Title>
        <ISOAbbreviation>J Anim Ecol</ISOAbbreviation>
      </Journal>
      <ArticleTitle>Night flight of <i>Bombyx</i> moths &amp; their predators.</ArticleTitle>
      <Pagination><MedlinePgn>501-12</MedlinePgn></Pagination>
      <ELocationID EIdType="doi" ValidYN="Y">10.5555/jae.2019.0001</ELocationID>
      <Abstract>
        <AbstractText Label="BACKGROUND" NlmCategory="BACKGROUND">Moths fly.</AbstractText>
        <AbstractText Label="RESULTS">They fly at night.</AbstractText>
      </Abstract>
      <AuthorList CompleteYN="Y">
        <Author ValidYN="Y"><LastName>Okafor</LastName><ForeName>Ada N</ForeName><Initials>AN</Initials></Author>
        <Author ValidYN="Y"><CollectiveName>Lunar Moth Survey Group</CollectiveName></Author>
      </AuthorList>
      <Language>eng</Language>
      <PublicationTypeList><PublicationType UI="D016428">Journal Article</PublicationType></PublicationTypeList>
    </Article>
    <MedlineJournalInfo><Country>England</Country><MedlineTA>J Anim Ecol</MedlineTA></MedlineJournalInfo>
  </MedlineCitation>
  <PubmedData><ArticleIdList>
    <ArticleId IdType="pubmed">31000001</ArticleId>
    <ArticleId IdType="doi">10.5555/jae.2019.0001</ArticleId>
    <ArticleId IdType="pmc">PMC6000001</ArticleId>
  </ArticleIdList></PubmedData>
</PubmedArticle>
</PubmedArticleSet>
"#;

    #[test]
    fn reads_an_article() {
        let mut d = Vec::new();
        let refs = read(ONE, &mut d);
        assert_eq!(refs.len(), 1, "{d:?}");
        let r = &refs[0];
        assert_eq!(r.pmid.as_deref(), Some("31000001"));
        assert_eq!(r.pmcid.as_deref(), Some("PMC6000001"));
        assert_eq!(r.doi.as_deref(), Some("10.5555/jae.2019.0001"));
        assert_eq!(r.title.as_deref(), Some("Night flight of <i>Bombyx</i> moths & their predators."));
        assert_eq!(r.author.len(), 2);
        assert_eq!(r.author[1].literal.as_deref(), Some("Lunar Moth Survey Group"));
        assert_eq!(r.issued.as_ref().unwrap().start.unwrap().month, Some(4));
        assert_eq!(r.abstract_text.as_deref(), Some("BACKGROUND: Moths fly.\n\nRESULTS: They fly at night."));
        assert_eq!(r.issn.as_deref(), Some("1365-2656"));
    }
}
