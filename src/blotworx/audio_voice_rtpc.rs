//! Resource RTPC subscription stage8035b460..8035b5ac (legacy host name
//! initialize_effects). This is subscription dispatch, not plugin DSP creation.
use super::audio_voice_bindings::State;
use anyhow::Result;
use std::sync::Arc;
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub id: u32,
    pub rendered: u32,
    pub words: [u32; 6],
}
pub trait Control {
    fn node_records(&mut self, node: u32) -> Result<Option<Arc<[Record]>>>;
    fn source_records(&mut self, node: u32) -> Result<Option<Arc<[Record]>>>;
    /// Words preserve native track+8/+c/+10/+1c/+14/+18 order. The concrete
    /// host owns conversion tables and subscription/evaluation semantics.
    fn subscribe_resource(
        &mut self,
        resource: u32,
        object: u32,
        words: [u32; 6],
        kind: u32,
    ) -> Result<u32>;
}
pub fn initialize(state: &State, node: u32, object: u32, host: &mut impl Control) -> Result<u32> {
    let mut status = 1;
    let records = host.node_records(node)?;
    if let (Some(records), Some(resource)) = (records, state.node_resource) {
        for record in records.iter() {
            if record.id == state.node_id && record.rendered == 0 {
                status = host.subscribe_resource(resource, object, record.words, 0)?;
            }
        }
    }
    // This query occurs even when the node list/resource was absent. Source
    // records do not apply the node list's rendered-word filter.
    if let Some(records) = host.source_records(node)? {
        for record in records.iter() {
            if record.id == state.source_id {
                if let Some(resource) = state.source_resource {
                    status = host.subscribe_resource(resource, object, record.words, 0)?;
                }
            }
        }
    }
    // Original loops continue on non-1 statuses; the final matched call wins.
    Ok(status)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Host {
        node: Option<Arc<[Record]>>,
        source: Option<Arc<[Record]>>,
        statuses: Vec<u32>,
        index: usize,
        calls: Vec<Value>,
    }
    impl Control for Host {
        fn node_records(&mut self, _: u32) -> Result<Option<Arc<[Record]>>> {
            self.calls.push(json!({"op":"node_records"}));
            Ok(self.node.clone())
        }
        fn source_records(&mut self, _: u32) -> Result<Option<Arc<[Record]>>> {
            self.calls.push(json!({"op":"source_records"}));
            Ok(self.source.clone())
        }
        fn subscribe_resource(
            &mut self,
            resource: u32,
            object: u32,
            words: [u32; 6],
            kind: u32,
        ) -> Result<u32> {
            self.calls.push(json!({"op":"subscribe","resource":resource,"object":object,"words":words,"kind":kind}));
            let status = self.statuses[self.index % self.statuses.len()];
            self.index += 1;
            Ok(status)
        }
    }
    #[test]
    fn original_resource_rtpc_lists_filters_arguments_and_last_status() -> Result<()> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_voice_rtpc.json"
        ))?;
        for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
            let parse = |name: &str| -> Result<Option<Arc<[Record]>>> {
                Ok(
                    serde_json::from_value::<Option<Vec<Record>>>(case[name].clone())?
                        .map(Arc::from),
                )
            };
            let mut host = Host {
                node: parse("node_records")?,
                source: parse("source_records")?,
                statuses: serde_json::from_value(case["statuses"].clone())?,
                index: 0,
                calls: Vec::new(),
            };
            let state = State {
                node_id: 44,
                source_id: 55,
                node_resource: if case["node_resource"] == true {
                    Some(101)
                } else {
                    None
                },
                source_resource: if case["source_resource"] == true {
                    Some(102)
                } else {
                    None
                },
                ..Default::default()
            };
            assert_eq!(
                initialize(&state, 0x72010000, 0x72020000, &mut host)? as u64,
                case["status"].as_u64().unwrap(),
                "case {index}"
            );
            assert_eq!(json!(host.calls), case["calls"], "case {index}");
        }
        Ok(())
    }
}
