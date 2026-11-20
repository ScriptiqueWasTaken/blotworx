//! Graph-manager name lookup47c850. Registration order supplies IDs; no
//! built-in constant identifies GotoTraversal independently of that order.
//! Comparison80024a50 folds ASCII A-Z through the initial retail C-locale
//! table; signed high bytes bypass conversion. It is not ordinary strcmp.
use anyhow::{ensure,Context as _,Result};

#[derive(Default,Debug,serde::Serialize)]
pub struct Registry {
    names:Vec<Vec<u8>>,
}
fn c_string(bytes:&[u8])->&[u8] {
    &bytes[..bytes.iter().position(|&b|b==0).unwrap_or(bytes.len())]
}
pub(super) fn names_equal(left:&[u8],right:&[u8])->bool {
    c_string(left).eq_ignore_ascii_case(c_string(right))
}
impl Registry {
    /// Call only after the corresponding traversal entry was registered.
    /// Preserve duplicates: native lookup selects the earliest matching name.
    pub fn append(&mut self,name:&[u8])->Result<u32> {
        let index=u32::try_from(self.names.len()).context("Traversal registry exceeds native word indices")?;
        ensure!(index!=u32::MAX,"Traversal registry exhausted native valid IDs");
        self.names.push(c_string(name).to_vec());
        Ok(index)
    }
    pub fn find(&self,name:Option<&[u8]>)->u32 {
        let Some(name)=name else {return u32::MAX};
        let name=c_string(name);
        self.names.iter().position(|stored|names_equal(stored,name))
            .map_or(u32::MAX,|index|index as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_traversal_name_lookup_matches_null_empty_duplicates_case_and_bytes()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_traversal_registry.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let mut registry=Registry::default();
            let decode=|v:&serde_json::Value|v.as_array().unwrap().iter().map(|b|b.as_u64().unwrap() as u8).collect::<Vec<_>>();
            for (index,name) in case["names"].as_array().unwrap().iter().enumerate() {
                assert_eq!(registry.append(&decode(name))?,index as u32);
            }
            let query=(!case["query"].is_null()).then(||decode(&case["query"]));
            assert_eq!(registry.find(query.as_deref()),case["result"].as_u64().unwrap() as u32,"{case}");
        }
        Ok(())
    }
}
