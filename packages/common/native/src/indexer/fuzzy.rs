use std::collections::{HashMap, HashSet};

use strsim::normalized_levenshtein;

use super::tokenizer::generate_trigrams;

const FUZZY_SIM_THRESHOLD: f64 = 0.6;
const FUZZY_MAX_CANDIDATES: usize = 20;

pub fn collect_fuzzy_candidates(
  ngram_index: &HashMap<String, Vec<String>>,
  term_dict: &HashSet<String>,
  term: &str,
) -> Vec<(String, f64)> {
  let trigrams = generate_trigrams(term);
  if trigrams.is_empty() {
    return collect_from_term_dict(term_dict, term);
  }

  let mut counts: HashMap<String, usize> = HashMap::new();
  for trigram in trigrams {
    if let Some(terms) = ngram_index.get(&trigram) {
      for candidate in terms {
        *counts.entry(candidate.clone()).or_insert(0) += 1;
      }
    }
  }

  let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
  ranked.sort_by(|a, b| b.1.cmp(&a.1));
  ranked.truncate(FUZZY_MAX_CANDIDATES);

  let mut filtered = Vec::new();
  for (candidate, _) in ranked {
    let similarity = normalized_levenshtein(&candidate, term);
    if similarity >= FUZZY_SIM_THRESHOLD {
      filtered.push((candidate, similarity));
    }
  }
  filtered
}

pub fn add_term_to_ngrams(index: &mut HashMap<String, Vec<String>>, term: &str) {
  if term.chars().count() < 3 {
    return;
  }
  for trigram in generate_trigrams(term) {
    let terms = index.entry(trigram).or_default();
    if !terms.contains(&term.to_string()) {
      terms.push(term.to_string());
    }
  }
}

pub fn remove_term_from_ngrams(index: &mut HashMap<String, Vec<String>>, term: &str) {
  if term.chars().count() < 3 {
    return;
  }
  for trigram in generate_trigrams(term) {
    if let Some(terms) = index.get_mut(&trigram) {
      terms.retain(|t| t != term);
      if terms.is_empty() {
        index.remove(&trigram);
      }
    }
  }
}

fn collect_from_term_dict(term_dict: &HashSet<String>, term: &str) -> Vec<(String, f64)> {
  let mut candidates: Vec<(String, f64)> = term_dict
    .iter()
    .filter_map(|candidate| {
      let similarity = normalized_levenshtein(candidate, term);
      (similarity >= FUZZY_SIM_THRESHOLD).then_some((candidate.clone(), similarity))
    })
    .collect();
  candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
  candidates.truncate(FUZZY_MAX_CANDIDATES);
  candidates
}
