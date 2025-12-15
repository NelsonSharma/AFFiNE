use std::{
  cmp::Ordering,
  collections::{HashMap, HashSet},
};

use super::{
  fuzzy::collect_fuzzy_candidates,
  tokenizer::{tokenize, Token},
  types::{InMemoryIndex, SearchMode, TermPosting},
};

const BM25_K1: f64 = 1.2;
const BM25_B: f64 = 0.75;
const FUZZY_WEIGHT: f64 = 0.7;
const PINYIN_FULL_WEIGHT: f64 = 0.9;
const PINYIN_INITIAL_WEIGHT: f64 = 0.8;

struct TermView<'a> {
  term: String,
  postings: &'a HashMap<String, i64>,
  weight: f64,
}

#[derive(Clone, Copy)]
pub(super) enum PostingSelector {
  Original,
  PinyinFull,
  PinyinInitials,
}

impl PostingSelector {
  fn map<'a>(&self, entry: &'a TermPosting) -> Option<(&'a HashMap<String, i64>, f64)> {
    match self {
      PostingSelector::Original => (!entry.original.is_empty()).then_some((&entry.original, 1.0)),
      PostingSelector::PinyinFull => {
        (!entry.pinyin_full.is_empty()).then_some((&entry.pinyin_full, PINYIN_FULL_WEIGHT))
      }
      PostingSelector::PinyinInitials => (!entry.pinyin_initials.is_empty())
        .then_some((&entry.pinyin_initials, PINYIN_INITIAL_WEIGHT)),
    }
  }
}

impl InMemoryIndex {
  pub fn search(&self, index_name: &str, query: &str) -> Vec<(String, f64)> {
    self.search_with_mode(index_name, query, SearchMode::Auto)
  }

  pub fn search_with_mode(
    &self,
    index_name: &str,
    query: &str,
    mode: SearchMode,
  ) -> Vec<(String, f64)> {
    if query == "*" || query.is_empty() {
      if let Some(docs) = self.docs.get(index_name) {
        return docs.keys().map(|k| (k.clone(), 1.0)).collect();
      }
      return vec![];
    }

    let query_terms = tokenize(query);
    if query_terms.is_empty() {
      return vec![];
    }

    match mode {
      SearchMode::Exact => self.bm25_search(index_name, &query_terms, PostingSelector::Original),
      SearchMode::Pinyin => self.pinyin_search(index_name, &query_terms),
      SearchMode::Fuzzy => self.fuzzy_search(index_name, &query_terms),
      SearchMode::Auto => {
        let exact = self.bm25_search(index_name, &query_terms, PostingSelector::Original);
        if !exact.is_empty() {
          return exact;
        }

        if is_ascii_alpha_query(&query_terms) {
          let pinyin = self.pinyin_search(index_name, &query_terms);
          if !pinyin.is_empty() {
            return pinyin;
          }
        }

        self.fuzzy_search(index_name, &query_terms)
      }
    }
  }

  fn bm25_search(
    &self,
    index_name: &str,
    query_terms: &[Token],
    selector: PostingSelector,
  ) -> Vec<(String, f64)> {
    if query_terms.is_empty() {
      return vec![];
    }

    let inverted = match self.inverted.get(index_name) {
      Some(i) => i,
      None => return vec![],
    };

    let docs = match self.docs.get(index_name) {
      Some(d) => d,
      None => return vec![],
    };

    let mut candidates: Option<HashSet<String>> = None;
    let mut term_views: Vec<TermView<'_>> = Vec::new();

    for token in query_terms {
      let entry = match inverted.get(&token.term) {
        Some(e) => e,
        None => return vec![],
      };
      let Some((doc_map, weight)) = selector.map(entry) else {
        return vec![];
      };

      let docs_for_term: HashSet<String> = doc_map.keys().cloned().collect();
      match &mut candidates {
        None => candidates = Some(docs_for_term),
        Some(existing) => existing.retain(|id| docs_for_term.contains(id)),
      }
      if candidates.as_ref().is_some_and(|c| c.is_empty()) {
        return vec![];
      }

      term_views.push(TermView {
        term: token.term.clone(),
        postings: doc_map,
        weight,
      });
    }

    let candidates = match candidates {
      Some(c) if !c.is_empty() => c,
      _ => return vec![],
    };

    let total_len = *self.total_lens.get(index_name).unwrap_or(&0);
    let n = docs.len() as f64;
    if n <= 0.0 {
      return vec![];
    }
    let avgdl = if n > 0.0 { total_len as f64 / n } else { 0.0 };

    let mut idfs = HashMap::new();
    for view in &term_views {
      let n_q = view.postings.len() as f64;
      let idf = ((n - n_q + 0.5) / (n_q + 0.5) + 1.0).ln();
      idfs.insert(view.term.clone(), idf);
    }

    let mut scores: Vec<(String, f64)> = Vec::with_capacity(candidates.len());
    for doc_id in candidates {
      let Some(doc_data) = docs.get(&doc_id) else {
        continue;
      };

      let mut score = 0.0;
      for view in &term_views {
        if let Some(freq) = view.postings.get(&doc_id) {
          let idf = *idfs.get(&view.term).unwrap_or(&0.0);
          score += bm25_component(*freq as f64, doc_data.doc_len as f64, avgdl, idf) * view.weight;
        }
      }

      if score > 0.0 {
        scores.push((doc_id, score));
      }
    }

    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scores
  }

  fn pinyin_search(&self, index_name: &str, query_terms: &[Token]) -> Vec<(String, f64)> {
    if !is_ascii_alpha_query(query_terms) {
      return vec![];
    }

    let full = self.bm25_search(index_name, query_terms, PostingSelector::PinyinFull);
    if !full.is_empty() {
      return full;
    }

    self.bm25_search(index_name, query_terms, PostingSelector::PinyinInitials)
  }

  fn fuzzy_search(&self, index_name: &str, query_terms: &[Token]) -> Vec<(String, f64)> {
    if query_terms.is_empty() || !is_ascii_alpha_query(query_terms) {
      return vec![];
    }

    let docs = match self.docs.get(index_name) {
      Some(d) => d,
      None => return vec![],
    };

    let inverted = match self.inverted.get(index_name) {
      Some(i) => i,
      None => return vec![],
    };

    let ngram_index = match self.ngram_index.get(index_name) {
      Some(idx) => idx,
      None => return vec![],
    };
    let term_dict = match self.term_dict.get(index_name) {
      Some(dict) => dict,
      None => return vec![],
    };

    let total_len = *self.total_lens.get(index_name).unwrap_or(&0);
    let n = docs.len() as f64;
    if n <= 0.0 {
      return vec![];
    }
    let avgdl = if n > 0.0 { total_len as f64 / n } else { 0.0 };

    let mut doc_scores: HashMap<String, f64> = HashMap::new();

    for token in query_terms {
      let candidates = collect_fuzzy_candidates(ngram_index, term_dict, &token.term);
      for (candidate_term, similarity) in candidates {
        if let Some(entry) = inverted.get(&candidate_term) {
          let doc_map = &entry.original;
          if doc_map.is_empty() {
            continue;
          }
          let n_q = doc_map.len() as f64;
          let idf = ((n - n_q + 0.5) / (n_q + 0.5) + 1.0).ln();

          for (doc_id, freq) in doc_map {
            if let Some(doc_data) = docs.get(doc_id) {
              let term_score = bm25_component(*freq as f64, doc_data.doc_len as f64, avgdl, idf)
                * FUZZY_WEIGHT
                * similarity;
              if term_score > 0.0 {
                *doc_scores.entry(doc_id.clone()).or_default() += term_score;
              }
            }
          }
        }
      }
    }

    let mut scores: Vec<(String, f64)> = doc_scores.into_iter().collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    scores
  }
}

pub(super) fn is_ascii_alpha_query(tokens: &[Token]) -> bool {
  !tokens.is_empty()
    && tokens
      .iter()
      .all(|token| token.term.chars().all(|c| c.is_ascii_alphabetic()))
}

pub(super) fn bm25_component(freq: f64, doc_len: f64, avgdl: f64, idf: f64) -> f64 {
  if freq <= 0.0 || idf <= 0.0 {
    return 0.0;
  }
  let norm_dl = if avgdl > 0.0 { doc_len / avgdl } else { 0.0 };
  let numerator = freq * (BM25_K1 + 1.0);
  let denominator = freq + BM25_K1 * (1.0 - BM25_B + BM25_B * norm_dl);
  if denominator == 0.0 {
    0.0
  } else {
    idf * (numerator / denominator)
  }
}
