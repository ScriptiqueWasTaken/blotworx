
use super::ai_find_nodes::*;
use anyhow::Result;
use super::ai_nearest_search as nearest;
use nearest::{tests::Replay, Host as _};
use serde_json::{json, Value};
impl Host for Replay<'_> {
    fn acceptance_policy(&mut self, _: &mut State, parent: usize) -> Result<u32> {
        self.trace.push(json!(["current", parent]));
        Ok(0x62000000)
    }
    fn goto_cost(&mut self, _: usize, _: usize, _: u32, _: u32, _: u32, _: u32) -> Result<u32> {
        anyhow::bail!("Composed oracle supplies collection, not goto-cost admission")
    }
}
#[test]
fn original_nearest_wrapper_and_complete_search_compose_with_pending_and_fallback_state(
) -> Result<()> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tools/blotworx/fixtures/ai_find_nodes_composed.json"
    ))?;
    for (index, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let input = &case["input"];
        let wrapper = &case["wrapper"];
        let mut h = Replay::new(input)?;
        let mut s = State::construct(0x6d000000, 0x61000000, b"reachable");
        s.start_flags = 11;
        s.goal_flags = 22;
        h.write(0x610001ac, 0x6e000000)?;
        h.write(0x61000098, input["limit"].as_u64().unwrap() as u32)?;
        h.write(0x62000100, wrapper["start"].as_u64().unwrap() as u32)?;
        h.write(0x62000300, 0x4321)?;
        for i in 0..3 {
            h.write(0x62000204 + i * 4, (4. + i as f32).to_bits())?;
        }
        if let Some(marker) = wrapper["marker"].as_u64() {
            h.write(0x62000200 + 16 + 7 * 4, 0x6f000000)?;
            h.write(0x6f000004, marker as u32)?;
        }
        let result = s.query(
            &mut h,
            Context {
                search: nearest::Context {
                    frame: 0x70001000,
                    point_kind: 0,
                    marker_kind: 0x4ebe80,
                    spatial_class: 0x11864,
                },
                frame: 0x70000000,
                callback_key: 1,
            },
            Args {
                start: 0x62000000,
                goal: 0x62000200,
                graph: 0x60000000,
                workspace: if input["use_default"].as_bool().unwrap() {
                    0
                } else {
                    0x63000000
                },
                start_node: 0x62000100,
                goal_node: 0x62000300,
            },
        )?;
        assert_eq!(json!(u32::from(result)), case["result"], "result {index}");
        assert_eq!(json!(h.read(0x62000100)?), case["node"], "node {index}");
        assert_eq!(json!(h.read(0x62000300)?), case["goal"], "goal {index}");
        assert_eq!(
            json!([s.start_flags, s.goal_flags]),
            case["flags"],
            "flags {index}"
        );
        let workspace: Vec<_> = (0..6)
            .map(|i| h.read(0x63000000 + i * 4).unwrap())
            .collect();
        assert_eq!(json!(workspace), case["workspace"], "workspace {index}");
        assert_eq!(json!(h.trace), case["trace"], "trace {index}");
    }
    Ok(())
}
