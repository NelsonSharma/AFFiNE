mod fuzzy;
mod index;
mod search;
mod tokenizer;
mod types;

pub use types::{DocData, InMemoryIndex, SearchMode, SnapshotData};

#[cfg(test)]
mod tests {
  use super::*;

  const INDEX: &str = "test-index";
  const DOC_CN: &str = "doc-cn";
  const DOC_EN: &str = "doc-en";

  fn assert_contains_doc(results: &[(String, f64)], doc_id: &str) {
    assert!(
      results.iter().any(|(id, _)| id == doc_id),
      "expected results to contain doc {doc_id}, got {:?}",
      results
    );
  }

  #[test]
  fn chinese_full_pinyin_search() {
    let mut index = InMemoryIndex::default();
    index.add_doc(INDEX, DOC_CN, "你好世界", true);

    let hits = index.search(INDEX, "nihao");
    assert_contains_doc(&hits, DOC_CN);
  }

  #[test]
  fn chinese_initials_search() {
    let mut index = InMemoryIndex::default();
    index.add_doc(INDEX, DOC_CN, "你好世界", true);

    let hits = index.search(INDEX, "nh");
    assert_contains_doc(&hits, DOC_CN);
  }

  #[test]
  fn english_fuzzy_search() {
    let mut index = InMemoryIndex::default();
    index.add_doc(INDEX, DOC_EN, "fuzzy search handles typos", true);

    let hits = index.search(INDEX, "fuzze");
    assert_contains_doc(&hits, DOC_EN);
  }

  #[test]
  fn fuzzy_search_handles_short_terms() {
    let mut index = InMemoryIndex::default();
    index.add_doc(INDEX, DOC_EN, "go go", true);

    let hits = index.search_with_mode(INDEX, "go", SearchMode::Fuzzy);
    assert_contains_doc(&hits, DOC_EN);
  }

  #[test]
  fn pinyin_highlight_uses_original_positions() {
    let mut index = InMemoryIndex::default();
    index.add_doc(INDEX, DOC_CN, "你好世界", true);

    let direct = index.get_matches(INDEX, DOC_CN, "你好");
    assert!(
      !direct.is_empty(),
      "expected direct chinese match to have positions"
    );

    let pinyin = index.get_matches(INDEX, DOC_CN, "nihao");
    assert_eq!(pinyin, direct);
  }

  #[test]
  fn exact_search_prefers_original_terms() {
    let mut index = InMemoryIndex::default();
    index.add_doc(INDEX, DOC_EN, "nihao greeting", true);
    index.add_doc(INDEX, DOC_CN, "你好世界", true);

    let exact_hits = index.search_with_mode(INDEX, "nihao", SearchMode::Exact);
    assert_contains_doc(&exact_hits, DOC_EN);
    assert!(
      exact_hits.iter().all(|(id, _)| id == DOC_EN),
      "expected exact search to ignore pinyin matches, got {:?}",
      exact_hits
    );

    let auto_hits = index.search(INDEX, "nihao");
    assert_contains_doc(&auto_hits, DOC_EN);
    assert!(
      auto_hits.iter().all(|(id, _)| id != DOC_CN),
      "auto search should stop at exact matches"
    );

    let pinyin_hits = index.search_with_mode(INDEX, "nihao", SearchMode::Pinyin);
    assert_contains_doc(&pinyin_hits, DOC_CN);
  }

  #[test]
  fn removing_doc_cleans_aux_indices() {
    let mut index = InMemoryIndex::default();
    index.add_doc(INDEX, DOC_EN, "token removal check", true);

    index.remove_doc(INDEX, DOC_EN);

    if let Some(term_dict) = index.term_dict.get(INDEX) {
      assert!(
        !term_dict.contains("token"),
        "term_dict should drop removed terms"
      );
    }

    if let Some(ngram_index) = index.ngram_index.get(INDEX) {
      let still_contains = ngram_index
        .values()
        .any(|terms| terms.iter().any(|term| term == "token"));
      assert!(!still_contains, "ngrams should remove term entries");
    }
  }
}
