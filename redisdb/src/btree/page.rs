use std::fmt;

pub const PAGE_SIZE: usize = 4096;
pub type PageId = u64;

#[derive(Clone)]
pub struct Page {
    pub id: PageId,
    pub data: Box<[u8; PAGE_SIZE]>,
    pub dirty: bool,
}

impl Page {
    pub fn new(id: PageId) -> Self {
        Page {
            id,
            data: Box::new([0u8; PAGE_SIZE]),
            dirty: false,
        }
    }

    pub fn read_u64(&self, offset: usize) -> u64 {
        u64::from_le_bytes(self.data[offset..offset + 8].try_into().unwrap())
    }

    pub fn write_u64(&mut self, offset: usize, val: u64) {
        self.data[offset..offset + 8].copy_from_slice(&val.to_le_bytes());
        self.dirty = true;
    }

    pub fn read_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    pub fn write_u32(&mut self, offset: usize, val: u32) {
        self.data[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
        self.dirty = true;
    }

    pub fn read_slice(&self, offset: usize, len: usize) -> &[u8] {
        &self.data[offset..offset + len]
    }

    pub fn write_slice(&mut self, offset: usize, data: &[u8]) {
        self.data[offset..offset + data.len()].copy_from_slice(data);
        self.dirty = true;
    }

    pub fn serialize(&self) -> Vec<u8> {
        self.data.to_vec()
    }

    pub fn deserialize(id: PageId, data: &[u8]) -> Self {
        let mut page = Page::new(id);
        page.data.copy_from_slice(data.try_into().unwrap());
        page
    }
}

impl fmt::Debug for Page {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Page(id={}, dirty={}, data[0..8]={:x?})",
            self.id,
            self.dirty,
            &self.data[..8]
        )
    }
}
