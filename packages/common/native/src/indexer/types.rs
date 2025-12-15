use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
  Exact,
  Pinyin,
  Fuzzy,
  Auto,
}

#[derive(Default, Debug)]
pub struct InMemoryIndex {
  pub docs: HashMap<String, HashMap<String, DocData>>,
  pub inverted: HashMap<String, HashMap<String, TermPosting>>,
  pub term_dict: HashMap<String, HashSet<String>>,
  pub ngram_index: HashMap<String, HashMap<String, Vec<String>>>,
  pub total_lens: HashMap<String, i64>,
  pub dirty: HashMap<String, HashSet<String>>,
  pub deleted: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocData {
  pub content: String,
  pub doc_len: i64,
  pub term_pos: HashMap<String, Vec<(u32, u32)>>,
  #[serde(default)]
  pub term_freqs: HashMap<String, TermFrequency>,
}

#[derive(Default, Debug, Clone)]
pub struct TermPosting {
  pub original: HashMap<String, i64>,
  pub pinyin_full: HashMap<String, i64>,
  pub pinyin_initials: HashMap<String, i64>,
}

impl TermPosting {
  pub fn is_empty(&self) -> bool {
    self.original.is_empty() && self.pinyin_full.is_empty() && self.pinyin_initials.is_empty()
  }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct TermFrequency {
  pub original: u32,
  pub pinyin_full: u32,
  pub pinyin_initials: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotData {
  pub docs: HashMap<String, DocData>,
  #[serde(default)]
  pub term_dict: HashSet<String>,
  #[serde(default)]
  pub ngram_index: HashMap<String, Vec<String>>,
}
