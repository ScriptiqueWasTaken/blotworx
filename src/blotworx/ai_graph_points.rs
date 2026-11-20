//! Point lookup474fb4/475068 and their original ordered-table helpers.
//! Buffers represent native normalized graph memory, not an HPD file decoder.
use anyhow::{Context as _,Result};
#[derive(Clone,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct Data {
    pub external_count:u32, //resource20
    pub stride:u32, //resource4 ->7c
    pub internal:Vec<u8>, //resource2c
    pub external:Option<Vec<u8>>, //resource34
}
#[derive(Clone,Copy,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
#[serde(rename_all="snake_case")]
pub enum Table {Internal,External}
#[derive(Clone,Copy,Debug,PartialEq,serde::Serialize,serde::Deserialize)]
pub struct Location {pub table:Table,pub offset:u32}
#[derive(Clone,Copy)]
pub enum Layout {Fixed32,Dynamic}
fn read(bytes:&[u8],offset:u32)->Result<u32> {
    let offset=offset as usize;let end=offset.checked_add(4).context("Graph point offset overflow")?;
    Ok(u32::from_be_bytes(bytes.get(offset..end).context("Graph point lookup exceeded its table")?.try_into()?))
}
fn search(bytes:&[u8],stride:u32,key:u32,mut low:u32,mut high:u32)->Result<Option<u32>> {
    loop {
        let first=low.wrapping_mul(stride);
        if read(bytes,first)?==key {return Ok(Some(first))}
        if low>=high {return Ok(None)}
        let last=high.wrapping_mul(stride);
        if read(bytes,last)?==key {return Ok(Some(last))}
        let mid=low.wrapping_add(high)>>1;
        let value=read(bytes,mid.wrapping_mul(stride))?;
        if value>key {high=mid;low=low.wrapping_add(1);} else {low=mid;high=high.wrapping_sub(1);}
    }
}
/// Concrete lookups share the same ordered search without copying table bytes.
pub(super) fn external_lookup(bytes:&[u8],stride:u32,count:u32,key:u32)->Result<Option<u32>> {
    let last=count.wrapping_sub(1);
    if read(bytes,0)?>key || read(bytes,last.wrapping_mul(stride))?<key {return Ok(None)}
    search(bytes,stride,key,0,last)
}
pub(super) fn find_tables(internal:&[u8],external:Option<&[u8]>,stride:u32,internal_count:u32,external_count:u32,node:u32)->Result<Option<Location>> {
    let last=internal_count.wrapping_sub(1);let key=node&0x7fffffff;
    if read(internal,0)?<=key && read(internal,last.wrapping_mul(stride))?>=key {
        if let Some(offset)=search(internal,stride,key,0,last)? {return Ok(Some(Location{table:Table::Internal,offset}))}
    }
    let Some(external)=external else{return Ok(None)};
    Ok(external_lookup(external,stride,external_count,node|0x80000000)?.map(|offset|Location{table:Table::External,offset}))
}
impl Data {
    pub fn node_id(&self,location:Location)->Result<u32> {
        let bytes=match location.table {Table::Internal=>&self.internal,Table::External=>self.external.as_ref().context("Graph ID refers to an absent external table")?};
        Ok(read(bytes,location.offset)?&0x7fffffff)
    }
    /// Count is virtual18 and is mandatory. It can change the resource:
    /// capture20 before the call, but read stride/tables and fallback20 after.
    pub fn find(&mut self,layout:Layout,node:u32,count:&mut impl FnMut(&mut Self)->Result<u32>)->Result<Option<Location>> {
        let external_count=self.external_count;
        let total=count(self)?;
        let stride=match layout {Layout::Fixed32=>32,Layout::Dynamic=>self.stride};
        find_tables(&self.internal,self.external.as_deref(),stride,total.wrapping_sub(external_count),self.external_count,node)
    }
    pub fn position(&self,location:Location)->Result<[u32;3]> {
        let table=match location.table {Table::Internal=>&self.internal,Table::External=>self.external.as_ref().context("Graph point refers to an absent external table")?};
        let at=location.offset;
        Ok([read(table,at.checked_add(4).context("Graph point coordinate overflow")?)?,
            read(table,at.checked_add(8).context("Graph point coordinate overflow")?)?,
            read(table,at.checked_add(12).context("Graph point coordinate overflow")?)?])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    #[test]
    fn original_fixed_and_dynamic_point_lookup_order_matches()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/ai_graph_points.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let c=&case["input"];let mut data:Data=serde_json::from_value(c["data"].clone())?;let mut trace=Vec::new();
            let layout=if c["layout"]=="fixed" {Layout::Fixed32} else {Layout::Dynamic};
            let location=data.find(layout,c["node"].as_u64().unwrap() as u32,&mut |data|{
                trace.push(json!(["count"]));
                if let Some(value)=c["change_external_count"].as_u64(){data.external_count=value as u32;}
                if let Some(value)=c["change_stride"].as_u64(){data.stride=value as u32;}
                Ok(c["total"].as_u64().unwrap() as u32)
            })?;
            assert_eq!(serde_json::to_value(location)?,case["location"],"{c}");
            assert_eq!(trace,case["trace"].as_array().unwrap().to_vec(),"{c}");
            assert_eq!(serde_json::to_value(&data)?,case["data"],"{c}");
            let position=location.map(|l|data.position(l)).transpose()?;
            assert_eq!(serde_json::to_value(position)?,case["position"],"{c}");
        }
        Ok(())
    }
    #[test]
    fn malformed_table_counts_report_errors_instead_of_false_lookup_success()->Result<()> {
        let mut data=Data{external_count:0,stride:32,internal:Vec::new(),external:None};
        assert!(data.find(Layout::Fixed32,1,&mut |_|Ok(0)).is_err());
        data.internal=vec![0;32];
        assert!(data.find(Layout::Fixed32,1,&mut |_|Ok(2)).is_err());
        assert!(data.position(Location{table:Table::External,offset:0}).is_err());
        Ok(())
    }
}
