//! Retail graph synchronization (DOL 0x80204c20, 0x80204f38, 0x801ef214).
//! Active-node construction/activation remains the graph scheduler's job.
use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct Point {
    pub event: i32,
    pub time: f32,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Sync {
    pub points: Vec<Point>,
    pub frequency: f32,
    pub time: f32,
    /// Names are intentionally offsets until their producer semantics are known.
    pub field_48: f32,
    pub mapped_events: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub start: i32,
    pub end: i32,
    pub fraction: f32,
}

impl Sync {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.frequency.is_finite() && self.time.is_finite(),
            "Invalid animation sync metadata"
        );
        ensure!(
            self.points.iter().all(|p| p.time.is_finite()),
            "Non-finite sync point"
        );
        Ok(())
    }

    pub fn position(&self, mut time: f32) -> Result<Position> {
        self.validate()?;
        ensure!(
            self.frequency != 0. && time.is_finite(),
            "Cannot map animation time without frequency"
        );
        let mut position = match self.points.as_slice() {
            [] => Position {
                start: -1,
                end: -1,
                fraction: time * self.frequency,
            },
            [point] => {
                if time < point.time {
                    time += 1. / self.frequency;
                }
                Position {
                    start: point.event,
                    end: point.event,
                    fraction: self.frequency * (time - point.time),
                }
            }
            points => {
                let end = points.iter().position(|p| time <= p.time).unwrap_or(0);
                let start = if end == 0 { points.len() - 1 } else { end - 1 };
                let mut start_time = points[start].time;
                let mut end_time = points[end].time;
                if end == 0 {
                    let period = 1. / self.frequency;
                    if time <= end_time {
                        start_time -= period;
                    } else {
                        end_time += period;
                    }
                }
                Position {
                    start: points[start].event,
                    end: points[end].event,
                    fraction: (time - start_time) / (end_time - start_time),
                }
            }
        };
        ensure!(
            position.fraction.is_finite(),
            "Degenerate animation sync interval"
        );
        position.fraction = position.fraction.clamp(0., 1.);
        Ok(position)
    }

    /// Missing both event IDs is a normal synchronization miss, not malformed data.
    /// Retail falls back to the matching endpoint if only one ID is present.
    pub fn local_time(&self, position: Position) -> Result<Option<f32>> {
        self.validate()?;
        ensure!(
            self.frequency != 0. && position.fraction.is_finite(),
            "Invalid synchronization position"
        );
        if position.start == -1 {
            return Ok(Some(position.fraction / self.frequency));
        }
        let period = 1. / self.frequency;
        let mut time = if position.start == position.end {
            let Some(point) = self.points.iter().find(|p| p.event == position.start) else {
                return Ok(None);
            };
            position.fraction.mul_add(period, point.time)
        } else {
            // Different endpoints use the last occurrence; identical endpoints
            // above use the first. Do not deduplicate repeated authored markers.
            let start = self.points.iter().rposition(|p| p.event == position.start);
            let end = self.points.iter().rposition(|p| p.event == position.end);
            match (start, end) {
                (None, None) => return Ok(None),
                (Some(i), None) | (None, Some(i)) => {
                    position.fraction.mul_add(period, self.points[i].time)
                }
                (Some(a), Some(b)) => {
                    let start_time = self.points[a].time;
                    if b > a {
                        // This branch returns before the wrap check in retail.
                        return Ok(Some(
                            position
                                .fraction
                                .mul_add(self.points[b].time - start_time, start_time),
                        ));
                    }
                    position
                        .fraction
                        .mul_add((period + self.points[b].time) - start_time, start_time)
                }
            }
        };
        if time > period {
            time -= period;
        }
        Ok(Some(time))
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Node {
    pub sync: Sync,
    pub parent: usize,
    pub flag_71: bool,
    pub synchronize: bool,
    pub flag_73: bool,
}

/// Children precede parents; the final active node receives the graph timestep.
/// Reuses caller storage and does not impose a limit on active graph nodes.
pub fn timesteps(
    nodes: &[Node],
    graph_dt: f32,
    event_map: &[i32],
    output: &mut Vec<f32>,
) -> Result<()> {
    ensure!(
        graph_dt.is_finite() && graph_dt >= 0.,
        "Invalid graph timestep"
    );
    ensure!(!nodes.is_empty(), "Animation graph has no active root");
    output.resize(nodes.len(), 0.);
    let root = nodes.len() - 1;
    output[root] = graph_dt;
    for i in (0..root).rev() {
        let node = &nodes[i];
        if graph_dt == 0. || node.flag_73 {
            output[i] = 0.;
            continue;
        }
        ensure!(
            node.parent > i && node.parent <= root,
            "Invalid animation sync parent ordering at node {i}"
        );
        let parent = &nodes[node.parent].sync;
        let mut dt = output[node.parent];
        output[i] = if node.synchronize && node.sync.frequency != 0. && parent.frequency != 0. {
            let period = 1. / parent.frequency;
            let cycles = if dt >= period {
                let cycles = (dt / period).floor();
                dt = (-cycles).mul_add(period, dt);
                cycles
            } else {
                0.
            };
            let mut next = parent.time + dt;
            if next > period {
                next = (-(next / period).floor()).mul_add(period, next);
                if next >= parent.time {
                    next = parent.time - 1e-5_f32;
                }
            }
            let mut position = parent.position(next)?;
            if parent.mapped_events && position.start != -1 && !event_map.is_empty() {
                let map = |id: i32| -> Result<i32> {
                    event_map
                        .get(usize::try_from(id).context("Negative mapped sync event")?)
                        .copied()
                        .context("Sync event outside graph mapping")
                };
                position.start = map(position.start)?;
                position.end = map(position.end)?;
            }
            match node.sync.local_time(position)? {
                None => dt,
                Some(mut next) => {
                    let child_period = 1. / node.sync.frequency;
                    if next < node.sync.time {
                        next += child_period;
                    }
                    cycles.mul_add(child_period, next) - node.sync.time
                }
            }
        } else if node.sync.field_48 == 0. && node.flag_71 {
            0.
        } else {
            dt
        };
        ensure!(
            output[i].is_finite(),
            "Non-finite synchronized timestep at node {i}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_retail_graph_timesteps_match_bit_for_bit() {
        #[derive(serde::Deserialize)]
        struct Sample {
            dt: f32,
            nodes: Vec<Node>,
            event_map: Vec<i32>,
            output: Vec<f32>,
        }
        #[derive(serde::Deserialize)]
        struct Fixture {
            samples: Vec<Sample>,
        }
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/animation_sync.json"
        ))
        .unwrap();
        assert_eq!(fixture.samples.len(), 20);
        let mut output = vec![];
        let mut compared = 0;
        for (sample, captured) in fixture.samples.iter().enumerate() {
            timesteps(
                &captured.nodes,
                captured.dt,
                &captured.event_map,
                &mut output,
            )
            .unwrap();
            for (node, (&actual, &expected)) in output.iter().zip(&captured.output).enumerate() {
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "sample {sample}, node {node}"
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 170);
    }
    fn sync(points: &[(i32, f32)]) -> Sync {
        Sync {
            points: points
                .iter()
                .map(|&(event, time)| Point { event, time })
                .collect(),
            frequency: 1.,
            time: 0.,
            field_48: 1.,
            mapped_events: false,
        }
    }
    #[test]
    fn marker_mapping_preserves_cycle_boundary_and_partial_matches() {
        let s = sync(&[(10, 0.25), (20, 0.75)]);
        assert_eq!(
            s.position(0.).unwrap(),
            Position {
                start: 20,
                end: 10,
                fraction: 0.5
            }
        );
        assert_eq!(
            s.position(0.5).unwrap(),
            Position {
                start: 10,
                end: 20,
                fraction: 0.5
            }
        );
        assert_eq!(s.local_time(s.position(1.).unwrap()).unwrap(), Some(1.));
        let p = Position {
            start: 10,
            end: 20,
            fraction: 0.5,
        };
        assert_eq!(sync(&[(20, 0.75)]).local_time(p).unwrap(), Some(0.25));
        assert_eq!(sync(&[(30, 0.75)]).local_time(p).unwrap(), None);
        assert_eq!(
            sync(&[])
                .local_time(Position {
                    start: -1,
                    end: -1,
                    fraction: 1.
                })
                .unwrap(),
            Some(1.)
        );
    }
    #[test]
    fn graph_propagates_cycles_and_validates_parent_order() {
        let mut child = Node {
            sync: sync(&[]),
            parent: 1,
            flag_71: false,
            synchronize: true,
            flag_73: false,
        };
        child.sync.frequency = 2.;
        let root = Node {
            sync: sync(&[]),
            parent: 0,
            flag_71: false,
            synchronize: false,
            flag_73: false,
        };
        let mut out = vec![];
        timesteps(&[child.clone(), root.clone()], 2.25, &[], &mut out).unwrap();
        assert_eq!(out, [1.125, 2.25]);
        child.parent = 0;
        assert!(timesteps(&[child, root], 0.1, &[], &mut out).is_err());
    }

    #[test]
    fn graph_maps_events_and_misses_inherit_the_reduced_parent_delta() {
        let mut child = Node {
            sync: sync(&[(10, 0.25), (20, 0.75)]),
            parent: 1,
            flag_71: false,
            synchronize: true,
            flag_73: false,
        };
        let mut root = Node {
            sync: sync(&[(0, 0.25), (1, 0.75)]),
            parent: 0,
            flag_71: false,
            synchronize: false,
            flag_73: false,
        };
        root.sync.mapped_events = true;
        let mut out = vec![];
        timesteps(&[child.clone(), root.clone()], 0.5, &[10, 20], &mut out).unwrap();
        assert_eq!(out, [0.5, 0.5]);
        child.sync.points = vec![Point {
            event: 30,
            time: 0.25,
        }];
        timesteps(&[child, root], 2.5, &[10, 20], &mut out).unwrap();
        assert_eq!(out, [0.5, 2.5]);
    }

    #[test]
    fn inactive_sync_payload_is_not_interpreted() {
        let mut child = Node {
            sync: sync(&[]),
            parent: 1,
            flag_71: false,
            synchronize: false,
            flag_73: false,
        };
        child.sync.frequency = f32::NAN;
        child.sync.time = f32::NAN;
        child.sync.field_48 = f32::NAN;
        let root = Node {
            sync: sync(&[]),
            parent: 0,
            flag_71: false,
            synchronize: false,
            flag_73: false,
        };
        let mut out = vec![];
        timesteps(&[child.clone(), root.clone()], 0.1, &[], &mut out).unwrap();
        assert_eq!(out, [0.1, 0.1]);
        child.flag_71 = true;
        child.sync.field_48 = 0.;
        timesteps(&[child, root], 0.1, &[], &mut out).unwrap();
        assert_eq!(out, [0., 0.1]);
    }
}
