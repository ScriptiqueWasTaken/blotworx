//! EM1 Wii version-46 music graph. Preserve the authored clip timeline,
//! selectors and cue rules so playback remains driven by game state.
use super::{
    audio_bank::{Bank, NodePrefix, NodeTail, Object}, audio_file::Spec,
    audio_loop::Layout, audio_output::{Output, Voice}, audio_playback::Clip,
    audio_signal::Signal,
};
use anyhow::{Context, Result, ensure};
use std::{collections::{BTreeMap, BTreeSet, VecDeque}, path::Path, sync::Arc};

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let data = self.0.get(..count).context("Truncated Wii music record")?;
        self.0 = &self.0[count..];
        Ok(data)
    }
    fn u8(&mut self) -> Result<u8> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16> { Ok(u16::from_be_bytes(self.take(2)?.try_into()?)) }
    fn u32(&mut self) -> Result<u32> { Ok(u32::from_be_bytes(self.take(4)?.try_into()?)) }
    fn i32(&mut self) -> Result<i32> { Ok(self.u32()? as i32) }
    fn f64(&mut self) -> Result<f64> {
        let value = f64::from_be_bytes(self.take(8)?.try_into()?);
        ensure!(value.is_finite(), "Nonfinite music time");
        Ok(value)
    }
    fn count(&mut self, item_size: usize) -> Result<usize> {
        let count = self.u32()? as usize;
        ensure!(count <= self.0.len() / item_size, "Truncated Wii music list");
        Ok(count)
    }
}

#[derive(Clone, Copy)]
struct Meter { grid: f64, offset: f64, tempo: f64, beats: u8, unit: u8 }
fn parameters<'a>(data: &'a [u8], feedback: bool) -> Result<(NodePrefix<'a>, Reader<'a>)> {
    let node = NodePrefix::parse(data)?;
    let (_, _, tail) = node.positioning_voice_settings()?;
    let (_, suffix) = NodeTail::parse(tail, feedback)?;
    Ok((node, Reader(suffix)))
}
fn music_parameters(data: &[u8], feedback: bool) -> Result<(Vec<u32>, Option<Meter>, Reader<'_>)> {
    let (_, mut data) = parameters(data, feedback)?;
    let count = data.count(4)?;
    let children = (0..count).map(|_| data.u32()).collect::<Result<Vec<_>>>()?;
    let meter = Meter { grid: data.f64()?, offset: data.f64()?,
        tempo: f32::from_bits(data.u32()?) as f64, beats: data.u8()?, unit: data.u8()? };
    let override_meter = data.u8()? != 0;
    let stingers = data.count(20)?;
    data.take(stingers * 20)?;
    Ok((children, override_meter.then_some(meter), data))
}

#[derive(Clone, Copy)]
struct Fade { duration: i32, curve: u8, offset: i32 }
impl Fade {
    fn read(data: &mut Reader<'_>) -> Result<Self> {
        let duration = data.i32()?;
        let curve = data.u32()?;
        let offset = data.i32()?;
        ensure!(duration >= 0 && curve <= 8, "Invalid musical transition fade");
        Ok(Self { duration, curve: curve as u8, offset })
    }
}
#[derive(Clone, Copy)]
struct Transition {
    source: u32, destination: u32, out: Fade, into: Fade,
    sync: u32, cue: u32, jump: u32, entry: u16, pre_entry: bool, post_exit: bool,
    intermediate: Option<(u32, Fade, Fade)>,
}
fn transitions(data: &mut Reader<'_>) -> Result<Vec<Transition>> {
    let count = data.count(79)?;
    (0..count).map(|_| {
        let source = data.u32()?;
        let destination = data.u32()?;
        let out = Fade::read(data)?;
        let sync = data.u32()?;
        let post_exit = data.u8()? != 0;
        let into = Fade::read(data)?;
        let cue = data.u32()?;
        let jump = data.u32()?;
        let entry = data.u16()?;
        let pre_entry = data.u8()? != 0;
        let enabled = data.u8()? != 0;
        let bridge = data.u32()?;
        // Disabled transition objects can contain arbitrary fade fields.
        let intermediate = if enabled {
            let fades = (Fade::read(data)?, Fade::read(data)?);
            Some((bridge, fades.0, fades.1))
        } else {
            data.take(24)?;
            None
        };
        data.take(2)?;
        Ok(Transition { source, destination, out, into, sync, cue, jump, entry,
            pre_entry, post_exit, intermediate })
    }).collect()
}

struct Item {
    id: u32, segment: u32, mode: u32, loops: u16, weight: u16, avoid: usize,
    weighted: bool, shuffle: bool, children: Vec<Item>,
}
impl Item {
    fn read(data: &mut Reader<'_>, left: &mut usize, depth: usize) -> Result<Self> {
        ensure!(*left > 0 && depth < 32, "Invalid music playlist hierarchy");
        *left -= 1;
        let segment = data.u32()?;
        let id = data.u32()?;
        let count = data.count(24)?;
        let mode = data.u32()?;
        let loops = data.u16()?;
        let weight = data.u16()?;
        let avoid = data.u16()? as usize;
        let weighted = data.u8()? != 0;
        let shuffle = data.u8()? != 0;
        let children = (0..count).map(|_| Self::read(data, left, depth + 1)).collect::<Result<_>>()?;
        Ok(Self { id, segment, mode, loops, weight, avoid, weighted, shuffle, children })
    }
}

#[derive(Default)]
struct ItemPosition {
    plays: u16,
    index: usize,
    active: Option<(usize, Box<ItemPosition>)>,
    history: VecDeque<usize>,
    shuffled: BTreeSet<usize>,
}
impl ItemPosition {
    fn next(&mut self, item: &Item, random: &mut dyn FnMut() -> u16) -> Option<u32> {
        if item.children.is_empty() {
            if item.loops != 0 && self.plays >= item.loops { return None; }
            self.plays = self.plays.saturating_add(1);
            return (item.segment != 0).then_some(item.segment);
        }
        for _ in 0..=item.children.len() {
            if let Some((index, child)) = &mut self.active {
                if let Some(segment) = child.next(&item.children[*index], random) { return Some(segment); }
                self.active = None;
            }
            if item.loops != 0 && self.plays >= item.loops { return None; }
            let index = if item.mode < 2 {
                self.index
            } else {
                self.select_random(item, random)?
            };
            self.index = (self.index + 1) % item.children.len();
            if item.mode & 1 != 0 || self.index == 0 { self.plays = self.plays.saturating_add(1); }
            self.active = Some((index, Box::default()));
        }
        None
    }
    fn select_random(&mut self, item: &Item, random: &mut dyn FnMut() -> u16) -> Option<usize> {
        if self.shuffled.len() == item.children.len() { self.shuffled.clear(); }
        let eligible = |i| !self.history.contains(&i) && !self.shuffled.contains(&i);
        let weight = |i: usize| if item.weighted { u32::from(item.children[i].weight) } else { 1 };
        let mut choices = (0..item.children.len()).filter(|&i| eligible(i)).collect::<Vec<_>>();
        if choices.is_empty() { choices = (0..item.children.len()).filter(|i| !self.shuffled.contains(i)).collect(); }
        let total: u32 = choices.iter().map(|&i| weight(i)).sum();
        if total == 0 { return None; }
        let mut pick = (u64::from(random()) * u64::from(total) / 32768) as u32;
        let selected = choices.into_iter().find(|&i| {
            let value = weight(i);
            if pick < value { true } else { pick -= value; false }
        })?;
        if item.shuffle { self.shuffled.insert(selected); }
        self.history.push_back(selected);
        while self.history.len() > item.avoid.min(item.children.len().saturating_sub(1)) {
            self.history.pop_front();
        }
        Some(selected)
    }
}

#[derive(Clone, Copy)]
struct TrackClip { file: u32, at: f64, start: f64, end: f64, duration: f64, gain: f32 }
fn track(object: &Object<'_>, feedback: bool) -> Result<Vec<TrackClip>> {
    ensure!(object.kind == 11, "Expected Wii music track");
    let mut data = Reader(object.payload);
    let count = data.count(25)?;
    let mut files = BTreeMap::new();
    for _ in 0..count {
        let codec = data.u32()?;
        let stream = data.u32()?;
        data.take(8)?;
        let source = data.u32()?;
        let file = data.u32()?;
        if stream != 1 { data.take(8)?; }
        data.u8()?;
        ensure!(codec == 0x50001 && stream == 1, "Unsupported music media codec/stream {codec:08x}/{stream}");
        files.insert(source, file);
    }
    let count = data.count(40)?;
    let mut clips = Vec::with_capacity(count);
    for _ in 0..count {
        let subtrack = data.u32()?;
        let source = data.u32()?;
        let at = data.f64()?;
        let start = data.f64()?;
        let end = data.f64()?;
        let duration = data.f64()?;
        ensure!(subtrack == 0, "Music random/sequence subtracks are not connected");
        clips.push(TrackClip { file: *files.get(&source).context("Music clip source missing")?, at, start, end, duration, gain: 0. });
    }
    if count != 0 { ensure!(data.u32()? == 1, "Multiple music subtracks require selection"); }
    let (parameters, _) = parameters(data.0, feedback)?;
    for clip in &mut clips { clip.gain = parameters.ranges[0][0]; }
    Ok(clips)
}

struct Segment { duration: f64, cues: Vec<(u32, f64)>, clips: Vec<TrackClip> }
impl Segment {
    fn entry(&self) -> f64 { self.cues.first().map_or(0., |&(_, time)| time) }
    fn exit(&self) -> f64 { self.cues.last().map_or(self.duration, |&(_, time)| time) }
}
enum Kind {
    Segment(Segment),
    Switch { kind: u32, group: u32, default: u32, links: BTreeMap<u32, u32> },
    Playlist(Item),
}
struct Node {
    kind: Kind, meter: Option<Meter>, rules: Vec<Transition>, gain: f32,
}
pub(super) struct Graph {
    root: u32,
    nodes: BTreeMap<u32, Node>,
}
impl Graph {
    pub(super) fn read(bank: &Bank<'_>, id: u32) -> Result<Self> {
        let mut graph = Self { root: id, nodes: BTreeMap::new() };
        graph.add(bank, id, 0)?;
        Ok(graph)
    }
    fn add(&mut self, bank: &Bank<'_>, id: u32, depth: usize) -> Result<()> {
        if self.nodes.contains_key(&id) { return Ok(()); }
        ensure!(depth < 32, "Music graph is too deep");
        let object = bank.objects.get(&id).with_context(|| format!("Missing Wii music node {id:08x}"))?;
        let (children, meter, mut data) = music_parameters(object.payload, bank.feedback)?;
        let gain = NodePrefix::parse(object.payload)?.ranges[0][0];
        let mut rules = Vec::new();
        let kind = match object.kind {
            10 => {
                let duration = data.f64()?;
                let count = data.count(12)?;
                let cues = (0..count).map(|_| Ok((data.u32()?, data.f64()?))).collect::<Result<Vec<_>>>()?;
                let mut clips = Vec::new();
                for child in children { clips.extend(track(bank.objects.get(&child).context("Wii music track missing")?, bank.feedback)?); }
                Kind::Segment(Segment { duration, cues, clips })
            }
            12 | 13 => {
                rules = transitions(&mut data)?;
                for child in children { self.add(bank, child, depth + 1)?; }
                for id in rules.iter().filter_map(|rule| rule.intermediate.map(|bridge| bridge.0)) {
                    if id != 0 { self.add(bank, id, depth + 1)?; }
                }
                if object.kind == 12 {
                    let kind = data.u32()?;
                    let group = data.u32()?;
                    let default = data.u32()?;
                    data.u8()?;
                    let count = data.count(8)?;
                    let links = (0..count).map(|_| Ok((data.u32()?,data.u32()?))).collect::<Result<_>>()?;
                    Kind::Switch { kind, group, default, links }
                } else {
                    let mut count = data.count(24)?;
                    let item = Item::read(&mut data, &mut count, 0)?;
                    ensure!(count == 0, "Music playlist counts disagree");
                    Kind::Playlist(item)
                }
            }
            _ => anyhow::bail!("Unsupported Wii music class {}", object.kind),
        };
        ensure!(data.0.is_empty(), "Unexpected Wii music node {id:08x} tail of {} bytes", data.0.len());
        self.nodes.insert(id, Node { kind, meter, rules, gain });
        Ok(())
    }
    fn route(&self, value: &mut impl FnMut(u32, u32) -> Result<Option<u32>>) -> Result<Vec<u32>> {
        let mut path = Vec::new();
        let mut id = self.root;
        while path.len() < 32 {
            path.push(id);
            let node = self.nodes.get(&id).context("Unresolved music branch")?;
            let Kind::Switch { kind, group, default, links } = &node.kind else { return Ok(path); };
            let selected = value(*kind, *group)?.unwrap_or(*default);
            let next = links.get(&selected).or_else(|| links.get(default)).copied().unwrap_or(0);
            if next == 0 { return Ok(path); }
            id = next;
        }
        anyhow::bail!("Wii music switch cycle")
    }
}

struct Playing {
    graph: Arc<Graph>,
    owner: Option<Arc<super::audio_events::RegisteredObject>>,
    path: Vec<u32>,
    cursor: ItemPosition,
    segment: Option<u32>,
    start_position: f64,
    switch_at: Option<f64>,
    voices: Vec<Voice>,
    began_tick: u32,
    duration: f64,
    bridge: Option<(u32, Fade)>,
    entry: u16,
    rule: Option<Transition>,
}
impl Playing {
    fn position(&self, tick: u32) -> f64 {
        self.start_position + self.voices.first().map_or(
            f64::from(tick.wrapping_sub(self.began_tick)) * 2.,
            |voice| voice.rendered_frames() as f64 * (1000. / 48000.),
        )
    }
    fn finished(&self, tick: u32) -> bool {
        if self.voices.is_empty() { return self.position(tick) >= self.duration; }
        self.voices.iter().all(Voice::done)
    }
    fn meter(&self, segment: u32) -> Option<Meter> {
        self.graph.nodes[&segment].meter.or_else(|| self.path.iter().rev().find_map(|id| self.graph.nodes[id].meter))
    }
    fn rule(&self, route: &[u32]) -> Option<Transition> {
        let current = self.segment.unwrap_or(0);
        let from = |id| id == u32::MAX || id == current || self.path.contains(&id);
        let to = |id| id == u32::MAX || route.contains(&id);
        self.path.iter().find_map(|id| self.graph.nodes[id].rules.iter().rev()
            .find(|rule| from(rule.source) && to(rule.destination)).copied())
    }
    fn switch_time(&self, rule: Option<Transition>, now: f64) -> f64 {
        let Some(id) = self.segment else { return now };
        let Kind::Segment(segment) = &self.graph.nodes[&id].kind else { return now };
        let Some(rule) = rule else { return segment.exit().max(now) };
        let meter = self.meter(id);
        let quantized = |period: f64, offset: f64| {
            if period > 0. && period.is_finite() {
                let origin = segment.entry() + offset;
                origin + ((now - origin) / period).ceil() * period
            } else { segment.exit() }
        };
        let next = match rule.sync {
            0 => now,
            1 => meter.map_or(segment.exit(), |m| quantized(m.grid, m.offset)),
            2 | 3 => meter.map_or(segment.exit(), |m| {
                let beat = 60000. / m.tempo * 4. / f64::from(m.unit);
                quantized(if rule.sync == 2 { beat * f64::from(m.beats) } else { beat }, 0.)
            }),
            4 | 5 => segment.cues.iter().find(|&&(cue, time)| time > now
                && if rule.cue != 0 { cue == rule.cue } else { rule.sync != 5 || cue > 1 })
                .map_or(segment.exit(), |&(_, time)| time),
            6 => segment.entry(),
            8 => f64::INFINITY,
            _ => segment.exit(),
        };
        next.max(now)
    }
}

pub(super) struct Runtime {
    state: BTreeMap<u32, u32>,
    active: BTreeMap<(u32, Option<u32>), Playing>,
    fading: Vec<Voice>,
    media: BTreeMap<u32, Vec<Arc<Spec>>>,
    root: Option<std::path::PathBuf>,
    paused: bool,
    volume: f32,
    started: u64,
    transitions: u64,
    errors: BTreeMap<u32, String>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self { state: BTreeMap::new(), active: BTreeMap::new(), fading: Vec::new(), media: BTreeMap::new(),
            root: None, paused: false, volume: 1., started: 0, transitions: 0, errors: BTreeMap::new() }
    }
}
impl Runtime {
    pub(super) fn set_root(&mut self, path: &Path) { self.root = Some(path.into()); }
    pub(super) fn set_state(&mut self, group: u32, value: u32) { self.state.insert(group, value); }
    pub(super) fn has_node(&self, id: u32) -> bool { self.active.values().any(|playing| playing.graph.nodes.contains_key(&id)) }
    pub(super) fn play(&mut self, graph: Arc<Graph>, owner: Option<Arc<super::audio_events::RegisteredObject>>, tick: u32) {
        let key = (graph.root, owner.as_ref().map(|value| value.handle));
        self.active.entry(key).or_insert_with(|| Playing {
            graph, owner, path: Vec::new(), cursor: ItemPosition::default(), segment: None,
            start_position: 0., switch_at: None, voices: Vec::new(), began_tick: tick, duration: 0.,
            bridge: None, entry: 0, rule: None,
        });
    }
    pub(super) fn stop(&mut self, node: u32, owner: Option<u32>, curve: u8, duration: i32, output: &mut Output) -> Result<()> {
        let keys = self.active.iter().filter(|((_, key), value)| (owner.is_none() || owner == *key)
            && value.graph.nodes.contains_key(&node)).map(|(&key, _)| key).collect::<Vec<_>>();
        for key in keys {
            if let Some(mut playing) = self.active.remove(&key) {
                self.fade(&mut playing, Fade { duration, curve, offset: 0 }, output)?;
            }
        }
        Ok(())
    }
    pub(super) fn set_controls(&mut self, paused: bool, volume: i32, output: &mut Output) -> Result<()> {
        let volume = volume.clamp(0,100) as f32 / 100.;
        if paused != self.paused {
            for playing in self.active.values() { for voice in &playing.voices { voice.set_paused(paused); } }
            for voice in &self.fading { voice.set_paused(paused); }
            self.paused = paused;
        }
        if volume != self.volume {
            for playing in self.active.values() { for voice in &playing.voices { output.set_level(voice, volume)?; } }
            for voice in &self.fading { output.set_level(voice, volume)?; }
            self.volume = volume;
        }
        Ok(())
    }
    fn fade(&mut self, playing: &mut Playing, fade: Fade, output: &mut Output) -> Result<()> {
        for voice in playing.voices.drain(..) {
            if !voice.done() && fade.duration > 0 {
                output.stop(&voice, fade.curve, fade.duration)?;
                self.fading.push(voice);
            }
        }
        Ok(())
    }
    pub(super) fn update(&mut self, output: &mut Output, random: &mut dyn FnMut() -> u16) {
        self.fading.retain(|voice| !voice.done());
        if self.paused { return; }
        for (key, mut playing) in std::mem::take(&mut self.active) {
            match self.advance(&mut playing, output, random) {
                Ok(true) => { self.active.insert(key, playing); }
                Ok(false) => {}
                Err(error) => { self.errors.insert(key.0, format!("{error:#}")); }
            }
        }
    }
    fn advance(&mut self, playing: &mut Playing, output: &mut Output, random: &mut dyn FnMut() -> u16) -> Result<bool> {
        let tick = output.tick();
        let path = playing.graph.route(&mut |kind, group| match kind {
            0 => playing.owner.as_ref().map(|owner| owner.switch_value(group)).unwrap_or(Ok(None)),
            1 => Ok(self.state.get(&group).copied()),
            _ => anyhow::bail!("Unknown Wii music sync group kind {kind}"),
        })?;
        let now = playing.position(tick);
        let mut switch = None;
        if playing.path != path {
            let rule = playing.rule(&path);
            let when = playing.switch_at.unwrap_or_else(|| playing.switch_time(rule, now));
            playing.switch_at = Some(when);
            let old_finished = playing.segment.is_none() || playing.finished(tick);
            if now >= when || old_finished && when.is_finite() {
                if let Some(rule) = rule { self.fade(playing, rule.out, output)?; }
                else { playing.voices.clear(); }
                playing.cursor = ItemPosition::default();
                playing.path = path;
                playing.segment = None;
                playing.switch_at = None;
                playing.rule = rule;
                playing.entry = rule.map_or(0, |value| value.entry);
                switch = rule;
                self.transitions += 1;
            }
        } else { playing.switch_at = None; }
        if playing.segment.is_some() && !playing.finished(tick) { return Ok(true); }
        if let Some((segment, fade)) = playing.bridge.take() {
            return self.start_segment(playing, segment, Some(fade), false, false, 0., output);
        }
        let branch = *playing.path.last().context("Empty music route")?;
        let node = &playing.graph.nodes[&branch];
        let next = match &node.kind {
            Kind::Segment(_) if playing.segment.is_none() => Some(branch),
            Kind::Playlist(item) => {
                if let Some(jump) = switch.map(|rule| rule.jump).filter(|&jump| jump != 0 && jump != u32::MAX) {
                    playing.cursor.next(item, random).filter(|&segment| segment == jump)
                        .or_else(|| item.children.iter().find(|item| item.id == jump || item.segment == jump).map(|item| item.segment))
                } else { playing.cursor.next(item, random) }
            }
            Kind::Switch { .. } => { playing.segment = None; return Ok(true); }
            _ => None,
        };
        let Some(next) = next else { playing.voices.clear(); return Ok(playing.switch_at.is_some()); };
        if let Some(rule) = switch {
            if let Some((bridge, enter, leave)) = rule.intermediate.filter(|value| value.0 != 0) {
                playing.bridge = Some((next, leave));
                return self.start_segment(playing, bridge, Some(enter), true, true, 0., output);
            }
        }
        let start = if playing.entry == 1 { now } else { 0. };
        self.start_segment(playing, next, switch.map(|rule| rule.into), switch.is_some_and(|rule| rule.pre_entry),
            playing.rule.is_some_and(|rule| rule.post_exit), start, output)
    }
    fn start_segment(&mut self, playing: &mut Playing, id: u32, fade: Option<Fade>, pre: bool, post: bool, start: f64,
        output: &mut Output) -> Result<bool> {
        let Kind::Segment(segment) = &playing.graph.nodes.get(&id).context("Music segment missing from graph")?.kind else {
            anyhow::bail!("Music playlist target is not a segment");
        };
        let beginning = if pre { 0. } else if start > segment.entry() && start < segment.exit() { start } else { segment.entry() };
        let ending = if post { segment.duration } else { segment.exit() };
        let path_gain: f32 = playing.path.iter().filter(|&&node| node != id).map(|node| playing.graph.nodes[node].gain).sum();
        let root = self.root.as_deref().context("Wii music media root unavailable")?;
        playing.voices.clear();
        for clip in &segment.clips {
            if let std::collections::btree_map::Entry::Vacant(slot) = self.media.entry(clip.file) {
                let path = root.join(format!("{}.wav",clip.file));
                slot.insert(Spec::channels(&path)?.into_iter().map(Arc::new).collect());
            }
            for (channel, spec) in self.media[&clip.file].iter().enumerate() {
                let rate = spec.definition.sample_rate;
                let frames = |ms: f64| (ms.max(0.) * f64::from(rate) / 1000.).round() as u64;
                let clip_begin = clip.at.max(beginning);
                let clip_end = ending.min(clip.at + clip.duration - clip.start - clip.end);
                let samples = u64::from(spec.definition.sample_count);
                let offset = frames(clip.start + clip_begin - clip.at).min(samples);
                let length = frames(clip_end - clip_begin).min(samples - offset);
                if length == 0 { continue; }
                let lead = frames(clip_begin - beginning);
                let pan = if self.media[&clip.file].len() == 1 { [1.,1.,0.,0.] }
                    else if channel == 0 { [1.,0.,0.,0.] } else { [0.,1.,0.,0.] };
                let clip = Clip {
                    samples: Arc::from([]), rate, sound: id, gain_db: clip.gain + path_gain + playing.graph.nodes[&id].gain,
                    channel_gains: pan, maximum_instances: 0, lineage: Arc::from([]), pitch_ratio: 1.,
                    pitch_cents: 0., low_pass: 0., random_ranges: Arc::from([]), pitch_controls: Arc::from([]),
                    spatial: None, admission: None, layout: Some(Layout::new((lead + length) as usize,None,1)?),
                    signal: Signal::MusicFile { spec: spec.clone(), offset, lead, length }, source_passes: 1,
                };
                // An immediate transition has no envelope. Passing a zero
                // duration to the voice mixer aborts the entire music cue.
                let attack = fade.and_then(|fade| {
                    let duration = fade.duration + fade.offset.min(0);
                    (duration > 0).then_some((fade.curve, duration))
                });
                let voice = output.start_with_fade(&clip, None, 0., attack)?;
                output.set_level(&voice, self.volume)?;
                playing.voices.push(voice);
                self.started += 1;
            }
        }
        playing.segment = Some(id);
        playing.start_position = beginning;
        playing.began_tick = output.tick();
        playing.duration = ending;
        Ok(true)
    }
    pub(super) fn summary(&self) -> serde_json::Value {
        serde_json::json!({"started_voices":self.started,"state_switches":self.transitions,"paused":self.paused,"errors":self.errors,
            "active":self.active.values().map(|playing|serde_json::json!({"root":playing.graph.root,"segment":playing.segment,
                "path":playing.path,"rendered_frames":playing.voices.iter().map(Voice::rendered_frames).max().unwrap_or(0)})).collect::<Vec<_>>()})
    }
}

#[cfg(debug_assertions)]
pub(super) fn validate() -> Result<()> {
    use std::{thread, time::{Duration, Instant}};
    let root = std::path::PathBuf::from(std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?).join("Audio/Wii");
    let bytes: Arc<[u8]> = std::fs::read(root.join("Global.bnk"))?.into();
    let bank = Bank::parse(&bytes)?;
    for id in [0x0a4b437f, 0x328a3549] {
        Graph::read(&bank, id).with_context(|| format!("Original DBC music root {id:08x}"))?;
    }
    let mut audio = super::audio_playback::Playback::load(bytes)?;
    audio.set_media_root(root);
    audio.post_on("Play_music_darkbeautycastle", 1)?;
    audio.post_on("Play_music_darkbeautycastle_Visit1", 1)?;
    let wait = |audio: &mut super::audio_playback::Playback, minimum_voices: u64| -> Result<()> {
        let stop = Instant::now() + Duration::from_secs(4);
        while Instant::now() < stop {
            audio.poll()?;
            let music = audio.music_summary();
            ensure!(music["errors"].as_object().is_some_and(|errors| errors.is_empty()), "{music}");
            if music["started_voices"].as_u64().unwrap_or(0) >= minimum_voices
                && music["active"].as_array().is_some_and(|active| active.iter().any(|track| track["rendered_frames"].as_u64().unwrap_or(0) > 512)) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        anyhow::bail!("The original music stream did not advance: {}",audio.music_summary())
    };
    wait(&mut audio, 2)?;
    let before = audio.music_summary();
    audio.set_state("Music_Combat", "Level_2")?;
    wait(&mut audio, 4)?;
    let after = audio.music_summary();
    ensure!(before["active"] != after["active"], "Authored music state did not switch");
    audio.set_game_music(true, 100)?;
    thread::sleep(Duration::from_millis(50));
    let paused = audio.music_summary();
    thread::sleep(Duration::from_millis(50));
    ensure!(paused == audio.music_summary(), "Music progressed across pause");
    audio.set_game_music(false, 100)?;
    audio.post_on("Stop_music_darkbeautycastle_Visit1", 1)?;
    audio.poll()?;
    println!("Original DBC music before switch {before}");
    println!("Original DBC music after switch {after}");
    Ok(())
}
