//! Common changed-world-property effects351b70/351c88/351d70.
use anyhow::Result;
pub trait Backend {
    fn mark_children(&mut self) -> Result<()>;
    fn related(&mut self, owner_offset: usize) -> Result<Option<usize>>;
    fn mark(&mut self, record: usize, byte_offset: usize, value: u8) -> Result<()>;
}
pub fn publish(backend: &mut impl Backend) -> Result<()> {
    backend.mark_children()?;
    if let Some(record) = backend.related(0x134)? {
        backend.mark(record, 0x56, 1)?;
    }
    if let Some(record) = backend.related(0x138)? {
        backend.mark(record, 0x3e, 1)?;
    }
    Ok(())
}
