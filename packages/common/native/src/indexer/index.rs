use std::collections::{HashMap, HashSet};

use super::{
  fuzzy::{add_term_to_ngrams, remove_term_from_ngrams},
  tokenizer::{build_pinyin_variants, contains_chinese_chars, tokenize},
  types::{DocData, InMemoryIndex, SnapshotData, TermFrequency},
};

type DirtyDoc = (String, String, String, i64);
type DeletedDoc = HashMap<String, HashSet<String>>;

impl InMemoryIndex {
  pub fn add_doc(&mut self, index_name: &str, doc_id: &str, text: &str, index: bool) {
    let tokens = if index { tokenize(text) } else { vec![] };
    // doc_len should be the number of tokens (including duplicates)
    let doc_len = tokens.len() as i64;

    let mut pos_map: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
    let mut term_freqs: HashMap<String, TermFrequency> = HashMap::new();
    let mut original_terms: HashSet<String> = HashSet::new();
    for token in tokens {
      let term = token.term;
      let start = token.start as u32;
      let end = token.end as u32;

      pos_map.entry(term.clone()).or_default().push((start, end));
      term_freqs.entry(term.clone()).or_default().original += 1;
      original_terms.insert(term.clone());

      if contains_chinese_chars(&term) {
        if let Some((full_pinyin, initials)) = build_pinyin_variants(&term) {
          if !full_pinyin.is_empty() {
            pos_map
              .entry(full_pinyin.clone())
              .or_default()
              .push((start, end));
            term_freqs
              .entry(full_pinyin.clone())
              .or_default()
              .pinyin_full += 1;
          }
          if !initials.is_empty() && initials != full_pinyin {
            pos_map
              .entry(initials.clone())
              .or_default()
              .push((start, end));
            term_freqs.entry(initials).or_default().pinyin_initials += 1;
          }
        }
      }
    }

    if let Some(docs) = self.docs.get_mut(index_name) {
      if let Some(old_data) = docs.remove(doc_id) {
        *self.total_lens.entry(index_name.to_string()).or_default() -= old_data.doc_len;

        self.remove_doc_terms(index_name, doc_id, &old_data);
      }
    }

    let doc_data = DocData {
      content: text.to_string(),
      doc_len,
      term_pos: pos_map,
      term_freqs,
    };

    self
      .docs
      .entry(index_name.to_string())
      .or_default()
      .insert(doc_id.to_string(), doc_data);
    *self.total_lens.entry(index_name.to_string()).or_default() += doc_len;

    let doc_ref = self
      .docs
      .get(index_name)
      .and_then(|docs| docs.get(doc_id))
      .expect("doc inserted");
    let inverted = self.inverted.entry(index_name.to_string()).or_default();
    for (term, freqs) in &doc_ref.term_freqs {
      let entry = inverted.entry(term.clone()).or_default();
      if freqs.original > 0 {
        entry
          .original
          .insert(doc_id.to_string(), freqs.original as i64);
      } else {
        entry.original.remove(doc_id);
      }
      if freqs.pinyin_full > 0 {
        entry
          .pinyin_full
          .insert(doc_id.to_string(), freqs.pinyin_full as i64);
      } else {
        entry.pinyin_full.remove(doc_id);
      }
      if freqs.pinyin_initials > 0 {
        entry
          .pinyin_initials
          .insert(doc_id.to_string(), freqs.pinyin_initials as i64);
      } else {
        entry.pinyin_initials.remove(doc_id);
      }
    }

    self.ensure_original_terms(index_name, original_terms);

    self
      .dirty
      .entry(index_name.to_string())
      .or_default()
      .insert(doc_id.to_string());
    if let Some(deleted) = self.deleted.get_mut(index_name) {
      deleted.remove(doc_id);
    }
  }

  pub fn remove_doc(&mut self, index_name: &str, doc_id: &str) {
    if let Some(docs) = self.docs.get_mut(index_name) {
      if let Some(old_data) = docs.remove(doc_id) {
        *self.total_lens.entry(index_name.to_string()).or_default() -= old_data.doc_len;

        self.remove_doc_terms(index_name, doc_id, &old_data);

        self
          .deleted
          .entry(index_name.to_string())
          .or_default()
          .insert(doc_id.to_string());
        if let Some(dirty) = self.dirty.get_mut(index_name) {
          dirty.remove(doc_id);
        }
      }
    }
  }

  pub fn get_doc(&self, index_name: &str, doc_id: &str) -> Option<String> {
    self
      .docs
      .get(index_name)
      .and_then(|docs| docs.get(doc_id))
      .map(|d| d.content.clone())
  }

  pub fn take_dirty_and_deleted(&mut self) -> (Vec<DirtyDoc>, DeletedDoc) {
    let dirty = std::mem::take(&mut self.dirty);
    let deleted = std::mem::take(&mut self.deleted);

    let mut dirty_data = Vec::new();
    for (index_name, doc_ids) in &dirty {
      if let Some(docs) = self.docs.get(index_name) {
        for doc_id in doc_ids {
          if let Some(data) = docs.get(doc_id) {
            dirty_data.push((
              index_name.clone(),
              doc_id.clone(),
              data.content.clone(),
              data.doc_len,
            ));
          }
        }
      }
    }
    (dirty_data, deleted)
  }

  pub fn get_matches(&self, index_name: &str, doc_id: &str, query: &str) -> Vec<(u32, u32)> {
    let mut matches = Vec::new();
    if let Some(docs) = self.docs.get(index_name) {
      if let Some(doc_data) = docs.get(doc_id) {
        let query_tokens = tokenize(query);
        for token in query_tokens {
          if let Some(positions) = doc_data.term_pos.get(&token.term) {
            matches.extend(positions.iter().cloned());
          }
        }
      }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0));
    matches
  }

  pub fn load_snapshot(&mut self, index_name: &str, snapshot: SnapshotData) {
    let docs = self.docs.entry(index_name.to_string()).or_default();
    let inverted = self.inverted.entry(index_name.to_string()).or_default();
    let total_len = self.total_lens.entry(index_name.to_string()).or_default();
    docs.clear();
    inverted.clear();
    *total_len = 0;

    for (doc_id, doc_data) in snapshot.docs {
      *total_len += doc_data.doc_len;

      for (term, positions) in &doc_data.term_pos {
        let entry = inverted.entry(term.clone()).or_default();
        if let Some(freqs) = doc_data.term_freqs.get(term) {
          if freqs.original > 0 {
            entry.original.insert(doc_id.clone(), freqs.original as i64);
          }
          if freqs.pinyin_full > 0 {
            entry
              .pinyin_full
              .insert(doc_id.clone(), freqs.pinyin_full as i64);
          }
          if freqs.pinyin_initials > 0 {
            entry
              .pinyin_initials
              .insert(doc_id.clone(), freqs.pinyin_initials as i64);
          }
        } else {
          entry
            .original
            .insert(doc_id.clone(), positions.len() as i64);
        }
      }

      docs.insert(doc_id, doc_data);
    }

    if snapshot.term_dict.is_empty() || snapshot.ngram_index.is_empty() {
      self.rebuild_aux_indices(index_name);
    } else {
      self
        .term_dict
        .insert(index_name.to_string(), snapshot.term_dict);
      self
        .ngram_index
        .insert(index_name.to_string(), snapshot.ngram_index);
    }
  }

  pub fn get_snapshot_data(&self, index_name: &str) -> Option<SnapshotData> {
    self.docs.get(index_name).map(|docs| SnapshotData {
      docs: docs.clone(),
      term_dict: self.term_dict.get(index_name).cloned().unwrap_or_default(),
      ngram_index: self
        .ngram_index
        .get(index_name)
        .cloned()
        .unwrap_or_default(),
    })
  }

  fn rebuild_aux_indices(&mut self, index_name: &str) {
    let Some(docs) = self.docs.get(index_name) else {
      return;
    };

    let term_dict = self.term_dict.entry(index_name.to_string()).or_default();
    let ngram_index = self.ngram_index.entry(index_name.to_string()).or_default();
    term_dict.clear();
    ngram_index.clear();

    for doc_data in docs.values() {
      if doc_data.term_freqs.is_empty() {
        for term in doc_data.term_pos.keys() {
          term_dict.insert(term.clone());
          add_term_to_ngrams(ngram_index, term);
        }
        continue;
      }

      for (term, freqs) in &doc_data.term_freqs {
        if freqs.original > 0 {
          term_dict.insert(term.clone());
          add_term_to_ngrams(ngram_index, term);
        }
      }
    }
  }

  fn ensure_original_terms(&mut self, index_name: &str, original_terms: HashSet<String>) {
    if original_terms.is_empty() {
      return;
    }
    let term_dict = self.term_dict.entry(index_name.to_string()).or_default();
    let ngram_index = self.ngram_index.entry(index_name.to_string()).or_default();

    for term in original_terms {
      term_dict.insert(term.clone());
      add_term_to_ngrams(ngram_index, &term);
    }
  }

  fn remove_doc_terms(&mut self, index_name: &str, doc_id: &str, doc_data: &DocData) {
    if let Some(inverted) = self.inverted.get_mut(index_name) {
      let mut remove_terms = Vec::new();
      for term in doc_data.term_pos.keys() {
        if let Some(entry) = inverted.get_mut(term) {
          entry.original.remove(doc_id);
          entry.pinyin_full.remove(doc_id);
          entry.pinyin_initials.remove(doc_id);

          if entry.original.is_empty() {
            if let Some(term_dict) = self.term_dict.get_mut(index_name) {
              if term_dict.remove(term) {
                if let Some(ngram_index) = self.ngram_index.get_mut(index_name) {
                  remove_term_from_ngrams(ngram_index, term);
                }
              }
            }
          }

          if entry.is_empty() {
            remove_terms.push(term.clone());
          }
        }
      }

      for term in remove_terms {
        inverted.remove(&term);
      }
    }
  }
}
