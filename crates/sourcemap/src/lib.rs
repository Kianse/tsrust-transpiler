use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapping {
    pub src_file: String,
    pub src_start: usize,
    pub src_end: usize,
    pub dst_start: usize,
    pub dst_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMap {
    pub version: u32,
    pub sources: Vec<String>,
    pub mappings: Vec<Mapping>,
}

impl Default for SourceMap {
    fn default() -> Self {
        Self {
            version: 1,
            sources: vec![],
            mappings: vec![],
        }
    }
}
