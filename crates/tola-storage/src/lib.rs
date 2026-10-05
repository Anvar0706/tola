//! Storage engine definitions and table schemas for MDBX and append-only blocks.


pub enum Table {
    Accounts,
    Storage,
    TreeNodes,
    BlockHeaders,
    BlockBodies,
    Receipts,
}

pub struct AppendOnlySegment {
    pub segment_id: u32,
    pub byte_offset: u64,
}

pub trait StorageDriver {
    fn put(&mut self, table: Table, key: &[u8], value: &[u8]) -> Result<(), String>;
    fn get(&self, table: Table, key: &[u8]) -> Result<Option<Vec<u8>>, String>;
    fn sync(&mut self) -> Result<(), String>;
}
