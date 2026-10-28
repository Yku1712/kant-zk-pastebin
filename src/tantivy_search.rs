// Tantivy-powered full-text and value search for pastebin.
// Integrates with the existing index.jsonl format to provide fast indexing and search.
use std::path::Path;
use std::sync::{Arc, RwLock};

use tantivy::collector::{Count, TopDocs};
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, STORED, TEXT, Field};
use tantivy::directory::MmapDirectory;
use tantivy::{doc, Index, IndexWriter, Searcher, Term};
use tantivy::schema::{Document, TantivyDocument};

use crate::model::PasteIndex;
use crate::handlers::read_index_entries;

/// Configuration for the Tantivy search index.
#[derive(Debug, Clone)]
pub struct TantivyConfig {
    /// Path where the Tantivy index files are stored.
    pub index_path: String,
    /// Maximum number of entries to index.
    pub max_entries: usize,
    /// Polling interval in seconds for reindexing.
    pub poll_seconds: u64,
}

impl Default for TantivyConfig {
    fn default() -> Self {
        Self {
            index_path: "/mnt/data1/kant/pastebin/.tantivy-index".to_string(),
            max_entries: 50000,
            poll_seconds: 60,
        }
    }
}

/// Main search index wrapper.
pub struct PasteTantivyIndex {
    config: TantivyConfig,
    index: Option<Index>,
    writer: Option<IndexWriter>,
    searcher: Option<Searcher>,
    is_reindexing: Arc<RwLock<bool>>,
    current_generation: u64,
}

impl PasteTantivyIndex {
    /// Create a new search index instance.
    pub fn new(config: TantivyConfig) -> Self {
        Self {
            config,
            index: None,
            writer: None,
            searcher: None,
            is_reindexing: Arc::new(RwLock::new(false)),
            current_generation: 0,
        }
    }

    /// Initialize the Tantivy index with the specified schema.
    pub fn initialize(&mut self) -> tantivy::Result<()> {
        // Create schema
        let mut schema_builder = Schema::builder();

        // Word index fields (for full-text search)
        let title_field = schema_builder.add_text_field("title", TEXT | STORED);
        let description_field = schema_builder.add_text_field("description", TEXT | STORED);
        let keywords_field = schema_builder.add_text_field("keywords", TEXT | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT);
        let filename_field = schema_builder.add_text_field("filename", TEXT | STORED);
        let timestamp_field = schema_builder.add_text_field("timestamp", TEXT | STORED);

        // Value index fields (for exact matches and filtering)
        let id_field = schema_builder.add_text_field("id", TEXT | STORED);
        let cid_field = schema_builder.add_text_field("cid", TEXT | STORED);
        let ipfs_cid_field = schema_builder.add_text_field("ipfs_cid", TEXT | STORED);
        let witness_field = schema_builder.add_text_field("witness", TEXT | STORED);
        let size_field = schema_builder.add_text_field("size", TEXT | STORED);
        let root_field = schema_builder.add_text_field("root", TEXT | STORED);
        let type_field = schema_builder.add_text_field("type", TEXT | STORED);

        // Create index
        let schema = schema_builder.build();
        let index = Index::open_or_create(MmapDirectory::open(&self.config.index_path)?, schema)?;

        self.index = Some(index.clone());

        // Get writer
        let writer = index.writer(50_000_000)?; // 50 MB memory budget
        self.writer = Some(writer);

        // Get searcher
        let searcher = index.reader()?.searcher();
        self.searcher = Some(searcher);

        Ok(())
    }

    /// Index a paste entry.
    pub fn index_paste(&mut self, entry: &PasteIndex) -> tantivy::Result<()> {
        if let Some(writer) = &mut self.writer {
            let schema = self.index.as_ref().unwrap().schema();

            // Get field handles
            let title_field = schema.get_field("title").unwrap();
            let description_field = schema.get_field("description").unwrap();
            let keywords_field = schema.get_field("keywords").unwrap();
            let content_field = schema.get_field("content").unwrap();
            let filename_field = schema.get_field("filename").unwrap();
            let timestamp_field = schema.get_field("timestamp").unwrap();
            let id_field = schema.get_field("id").unwrap();
            let cid_field = schema.get_field("cid").unwrap();
            let ipfs_cid_field = schema.get_field("ipfs_cid").unwrap();
            let witness_field = schema.get_field("witness").unwrap();
            let size_field = schema.get_field("size").unwrap();
            let root_field = schema.get_field("root").unwrap();
            let type_field = schema.get_field("type").unwrap();

            let mut doc = doc!(
                id_field => entry.id.as_str(),
                title_field => entry.title.as_str(),
                description_field => entry.description.as_deref().unwrap_or(""),
                keywords_field => entry.keywords.join(" ").as_str(),
                filename_field => entry.filename.as_str(),
                timestamp_field => entry.timestamp.as_str(),
                cid_field => entry.cid.as_str(),
                ipfs_cid_field => entry.ipfs_cid.as_deref().unwrap_or(""),
                witness_field => entry.witness.as_str(),
                size_field => entry.size.to_string().as_str(),
                root_field => entry.root.as_deref().unwrap_or(""),
                type_field => "paste",
            );

            // Add content if available
            if let Some(content) = crate::storage::load_content(&entry.id) {
                doc.add_text(content_field, &content);
            }

            writer.add_document(doc)?;
            Ok(())
        } else {
            Err(tantivy::TantivyError::InvalidArgument("index not initialized".to_string()))
        }
    }

    /// Commit indexed documents.
    pub fn commit(&mut self) -> tantivy::Result<()> {
        if let Some(writer) = &mut self.writer {
            writer.commit()?;
            // Update searcher
            if let Some(index) = &self.index {
                let searcher = index.reader()?.searcher();
                self.searcher = Some(searcher);
            }
            Ok(())
        } else {
            Err(tantivy::TantivyError::InvalidArgument("index not initialized".to_string()))
        }
    }

    /// Search for entries matching the query using Tantivy.
    pub fn search(
        &self,
        query_text: &str,
        limit: usize,
        _filters: &[(&str, &str)],
    ) -> tantivy::Result<Vec<crate::handlers::SearchResult>> {
        if let Some(searcher) = &self.searcher {
            let schema = searcher.schema();
            let title_field = schema.get_field("title")?;
            let description_field = schema.get_field("description")?;
            let keywords_field = schema.get_field("keywords")?;
            let content_field = schema.get_field("content")?;
            let filename_field = schema.get_field("filename")?;
            let id_field = schema.get_field("id")?;
            let timestamp_field = schema.get_field("timestamp")?;

            let mut results: Vec<crate::handlers::SearchResult> = Vec::new();

            // Create query parser for multiple fields
            let fields = vec![
                title_field,
                description_field,
                keywords_field,
                content_field,
                filename_field,
            ];

            let query_parser = QueryParser::for_index(&self.index.as_ref().unwrap(), fields);

            if let Ok(query) = query_parser.parse_query(query_text) {
                let collector = TopDocs::with_limit(limit);
                let search_results = searcher.search(&query, &collector)?;

                for (_score, doc_address) in search_results {
                    let doc: TantivyDocument = searcher.doc(doc_address).unwrap();
                    let id = doc
                        .get_first(id_field)
                        .and_then(|v| match v {
                            tantivy::schema::OwnedValue::Str(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or("");
                    let title = doc
                        .get_first(title_field)
                        .and_then(|v| match v {
                            tantivy::schema::OwnedValue::Str(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or("");
                    let description = doc
                        .get_first(description_field)
                        .and_then(|v| match v {
                            tantivy::schema::OwnedValue::Str(s) => Some(s.as_str()),
                            _ => None,
                        });
                    let timestamp = doc
                        .get_first(timestamp_field)
                        .and_then(|v| match v {
                            tantivy::schema::OwnedValue::Str(s) => Some(s.as_str()),
                            _ => None,
                        })
                        .unwrap_or("");

                    // Avoid duplicates
                    if !results.iter().any(|r: &crate::handlers::SearchResult| r.id == id) {
                        results.push(crate::handlers::SearchResult {
                            id: id.to_string(),
                            title: title.to_string(),
                            description: description.map(|s| s.to_string()),
                            keywords: Vec::new(),
                            match_type: "content".to_string(),
                            excerpt: String::new(),
                            url: format!("/paste/{}", id),
                            timestamp: timestamp.to_string(),
                            size: 0,
                            source: Some("paste".to_string()),
                            file_path: None,
                        });
                    }
                }
            }

            // Apply limit
            results.truncate(limit);

            Ok(results)
        } else {
            Err(tantivy::TantivyError::InvalidArgument("index not initialized".to_string()))
        }
    }

    /// Reindex entries from the JSONL file.
    pub fn reindex(&mut self, uucp_dir: &str) -> tantivy::Result<()> {
        {
            let mut reindexing = self.is_reindexing.write().unwrap();
            if *reindexing {
                return Ok(());
            }
            *reindexing = true;
        }

        // Clear existing index
        if let Some(writer) = &mut self.writer {
            writer.delete_all_documents()?;
        }

        // Read entries from JSONL
        let entries = read_index_entries(uucp_dir);

        // Index entries
        for entry in entries.iter().take(self.config.max_entries) {
            if let Err(e) = self.index_paste(entry) {
                tracing::error!("Failed to index entry {}: {:?}", entry.id, e);
            }
        }

        // Commit changes
        self.commit()?;

        {
            let mut reindexing = self.is_reindexing.write().unwrap();
            *reindexing = false;
        }
        self.current_generation += 1;

        Ok(())
    }

    /// Get current index generation (for cache invalidation)
    pub fn generation(&self) -> u64 {
        self.current_generation
    }
}

/// Global search index instance.
lazy_static::lazy_static! {
    static ref GLOBAL_SEARCH: Arc<RwLock<PasteTantivyIndex>> = Arc::new(RwLock::new(PasteTantivyIndex::new(TantivyConfig::default())));
}

/// Get the global search index instance.
pub fn get_search_index() -> Arc<RwLock<PasteTantivyIndex>> {
    GLOBAL_SEARCH.clone()
}

/// Initialize the search index.
pub fn initialize_search_index() -> tantivy::Result<()> {
    let search_arc = get_search_index();
    let mut search = search_arc.write().unwrap();
    search.initialize()
}

/// Reindex all entries from the spool.
pub fn reindex_spool(uucp_dir: &str) -> tantivy::Result<()> {
    let search_arc = get_search_index();
    let mut search = search_arc.write().unwrap();
    search.reindex(uucp_dir)
}

/// Search for entries using Tantivy.
pub fn search_entries(
    query: &str,
    limit: usize,
    filters: &[(&str, &str)],
) -> tantivy::Result<Vec<crate::handlers::SearchResult>> {
    let search_arc = get_search_index();
    let search = search_arc.read().unwrap();
    search.search(query, limit, filters)
}

/// Get the index generation for cache invalidation.
pub fn get_index_generation() -> u64 {
    let search_arc = get_search_index();
    let search = search_arc.read().unwrap();
    search.generation()
}