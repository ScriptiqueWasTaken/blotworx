//! Raw definition-string handles (45b8bc/45b8c4). Byte edits are visible;
//! replacing/freeing storage invalidates a handle rather than retaining a copy.
use anyhow::{Context,Result,ensure};
use super::ai_navigation_definitions::Tree;
#[derive(Clone,Copy,PartialEq,Eq)]
pub enum Field { Name, Value, Identifier }
#[derive(Default)]
pub struct Strings {handles:Vec<(usize,Field,usize)>}
impl Strings {
    fn bytes(tree:&Tree,node:usize,field:Field)->Result<Option<&Vec<u8>>> {
        let row=tree.node(node)?;
        Ok(match field{Field::Name=>row.name.as_ref(),Field::Value=>row.value.as_ref(),Field::Identifier=>row.identifier.as_ref()})
    }
    pub fn capture(&mut self,tree:&Tree,node:usize,field:Field)->Result<Option<usize>> {
        let Some(text)=Self::bytes(tree,node,field)? else {return Ok(None)};
        let key=(node,field,text.as_ptr() as usize);
        if let Some(id)=self.handles.iter().position(|&entry|entry==key) {return Ok(Some(id));}
        let id=self.handles.len();self.handles.push(key);Ok(Some(id))
    }
    pub fn text(&self,tree:&Tree,handle:usize)->Result<Vec<u8>> {
        let &(node,field,pointer)=self.handles.get(handle).context("Definition string handle absent")?;
        let text=Self::bytes(tree,node,field)?.context("Definition string buffer removed while in use")?;
        ensure!(text.as_ptr() as usize==pointer,"Definition string buffer replaced while in use");
        Ok(text[..text.iter().position(|&b|b==0).unwrap_or(text.len())].to_vec())
    }
}
