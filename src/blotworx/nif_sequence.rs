//! EM1 20.6.5.0 sequence resources. Decoding does not activate a sequence.
//! Layout checked against NiSequenceData::LoadBinary and original Wii assets.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::nif_file::{NiObject, NifFile};
#[path = "nif_spline.rs"]
mod spline;

/// AnimationComponentGB's local clock, separate from NiActorManager playback.
/// The caller supplies the original owner clock and component update gate;
/// visibility/pose gating occurs AFTER advance and must not be folded into it.
/// REL2f5c44..5cac initializes, REL2f5d34..5d4c advances, and
/// REL2f5e9c..5ec0 chooses the time passed to the actor manager.
#[derive(Debug, Clone)]
pub struct ComponentClock {
    accumulated: f32,
    pub use_delta_time: bool,
}

impl Default for ComponentClock {
    fn default() -> Self {
        // JPSSceneGraph constructor REL34ffa8 initializes +0x60 to zero;
        // typed property constructor REL203624 writes Use Delta Time=true.
        Self {
            accumulated: 0.0,
            use_delta_time: true,
        }
    }
}

impl ComponentClock {
    /// This is lifecycle initialization, not a reset on each sequence request.
    /// Original owner time is double precision, then rounded by frsp (1eff10).
    pub fn initialize(&mut self, start_using_global_time: bool, owner_time: f64) {
        self.accumulated = if start_using_global_time {
            owner_time as f32
        } else {
            0.0
        };
    }

    /// Run with the owner-specific delta, not the renderer's frame duration.
    /// Original fadds rounds every accumulation to single precision. No
    /// frequency multiplier, nonnegative clamp, or visibility test is applied.
    pub fn advance(&mut self, component_can_update: bool, owner_delta: f32) {
        if component_can_update {
            self.accumulated += owner_delta;
        }
    }

    /// Initial target application uses zero for the non-delta-time branch.
    pub fn initial_update_time(&self) -> f32 {
        if self.use_delta_time {
            self.accumulated
        } else {
            0.0
        }
    }

    /// Periodic updates instead retain their caller-supplied time.
    pub fn periodic_update_time(&self, caller_time: f32) -> f32 {
        if self.use_delta_time {
            self.accumulated
        } else {
            caller_time
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FadeDirection {
    In,
    Out,
}

/// NiControllerSequence::ComputeScaledTime (DOL803e49d8), independent of
/// activation offset, time synchronization and evaluator sampling.
#[derive(Debug, Clone)]
pub struct SequenceTime {
    last_time: f32,
    weighted_time: f32,
}
impl Default for SequenceTime {
    fn default() -> Self {
        Self {
            last_time: -f32::MAX,
            weighted_time: 0.,
        }
    }
}
impl SequenceTime {
    /// Frequency may change between updates; accumulated time must survive.
    /// Queries with store=false must not consume a clock update.
    pub fn compute(
        &mut self,
        time: f32,
        frequency: f32,
        duration: f32,
        cycle_type: u32,
        store: bool,
    ) -> Result<f32> {
        ensure!(
            time.is_finite() && frequency.is_finite() && duration.is_finite() && duration >= 0.,
            "Invalid sequence time, frequency or duration"
        );
        let (delta, previous) = if self.last_time == -f32::MAX {
            (time, 0.)
        } else {
            (time - self.last_time, self.weighted_time)
        };
        let weighted = delta.mul_add(frequency, previous); // Original fmadds.
        ensure!(weighted.is_finite(), "Sequence weighted time overflow");
        let mut scaled = weighted;
        if cycle_type == 0 {
            if duration != 0. {
                // Keep the first end frame; this is not rem_euclid. The Wii
                // call uses double fmod followed by frsp, retaining signed zero.
                if scaled != duration {
                    scaled = ((scaled as f64) % (duration as f64)) as f32;
                }
                if scaled < 0. {
                    scaled += duration;
                }
            } else {
                scaled = 0.;
            }
        }
        // The original release path clamps all non-loop values; it does not
        // implement a ping-pong/reverse branch for sequence cycle type 1.
        if scaled > duration {
            scaled = duration;
        } else if scaled < 0. {
            scaled = 0.;
        }
        if store {
            self.last_time = time;
            self.weighted_time = weighted;
        }
        Ok(scaled)
    }
}

/// Ordinary, non-held, unsynchronized sequence activation clock. Actor
/// callbacks and synchronized/held destinations are separate lifecycle paths,
/// not silently approximated here.
#[derive(Debug, Clone)]
pub struct UnsynchronizedClock {
    pub frequency: f32,
    offset: f32,
    pending_start: f32,
    scaled: SequenceTime,
}
impl UnsynchronizedClock {
    pub(super) fn weighted_time(&self) -> Option<f32> {
        (self.scaled.last_time != -f32::MAX).then_some(self.scaled.weighted_time)
    }

    pub fn new(start_frame: f32, frequency: f32) -> Result<Self> {
        ensure!(
            start_frame.is_finite() && frequency.is_finite() && frequency != 0.,
            "Invalid activation frame/frequency"
        );
        // DOL803e3494: convert requested frame to unscaled time at activation.
        let pending_start = start_frame / frequency;
        ensure!(pending_start.is_finite(), "Activation start time overflow");
        Ok(Self {
            frequency,
            offset: -f32::MAX,
            pending_start,
            scaled: SequenceTime::default(),
        })
    }
    /// Clock portion of CrossFade's immediate deactivate/reactivate(CURRENT_FRAME)
    /// pair. This does not perform its state changes or activation callbacks.
    pub fn prepare_crossfade_source(&mut self) -> Result<()> {
        // DOL803e4380..43bc adjusts offset using weighted (unwrapped) time,
        // not the last visible/clamped clip frame.
        if self.scaled.last_time != -f32::MAX
            && self.offset != -f32::MAX
            && self.scaled.weighted_time != -f32::MAX
        {
            ensure!(
                self.frequency.is_finite() && self.frequency != 0.,
                "Invalid source reactivation frequency"
            );
            let offset =
                self.offset + (self.scaled.weighted_time / self.frequency - self.scaled.last_time);
            ensure!(offset.is_finite(), "Source reactivation offset overflow");
            self.offset = offset;
        }
        // Deactivate clears destination frame; CURRENT_FRAME skips replacing
        // the retained offset. Reactivate resets last time at DOL803e4154.
        self.pending_start = -f32::MAX;
        self.scaled.last_time = -f32::MAX;
        Ok(())
    }
    pub fn sample(&mut self, global_time: f32, duration: f32, cycle_type: u32) -> Result<f32> {
        ensure!(global_time.is_finite(), "Nonfinite activation update time");
        // DOL803e2858..2898, then unsynchronized UpdateInternal803e2b64.
        // Keep each fadds; algebraically subtracting the first timestamp and
        // adding the start frame afterwards has different rounding.
        if self.offset == -f32::MAX {
            self.offset = -global_time;
        }
        if self.pending_start != -f32::MAX {
            self.offset += self.pending_start;
            self.pending_start = -f32::MAX;
        }
        self.scaled.compute(
            self.offset + global_time,
            self.frequency,
            duration,
            cycle_type,
            true,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FadeSample {
    Weight(f32),
    /// The sequence lifecycle handles activation/deactivation and callbacks.
    /// In particular the outgoing sequence receives a final sample at end_time.
    Complete {
        end_time: f32,
    },
}

/// Positive-duration sequence fade envelope (DOL803e28a0..28b8,
/// 803e2928..2948 and 803e2a20..2a40). Each sequence owns its own anchor;
/// source and destination must not implicitly share a request-time clock.
#[derive(Debug, Clone)]
pub struct FadeEnvelope {
    duration: f32,
    interval: Option<(f32, f32)>,
}
impl FadeEnvelope {
    pub fn new(duration: f32) -> Result<Self> {
        ensure!(duration.is_finite() && duration>0.,"Fade envelope requires positive finite duration; immediate activation/deactivation is a separate lifecycle path");
        Ok(Self {
            duration,
            interval: None,
        })
    }
    pub fn sample(&mut self, time: f32, direction: FadeDirection) -> Result<FadeSample> {
        ensure!(time.is_finite(), "Nonfinite sequence fade time");
        // Original start sentinel is resolved on first Update. Preserve the
        // rounding of both fadds and each fsubs/fdivs, including cancellation.
        let (start, end) = *self
            .interval
            .get_or_insert_with(|| (time, time + self.duration));
        if time >= end {
            return Ok(FadeSample::Complete { end_time: end });
        }
        let numerator = match direction {
            FadeDirection::In => time - start,
            FadeDirection::Out => end - time,
        };
        Ok(FadeSample::Weight(numerator / (end - start)))
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Sequence {
    pub name: Option<String>,
    pub evaluators: Vec<u32>,
    pub text_keys: Option<u32>,
    pub duration: f32,
    pub cycle_type: u32,
    pub frequency: f32,
    pub accumulation_root: Option<String>,
    pub flags: u32,
}

impl Sequence {
    // NiTextKeyExtraData loads its NiExtraData name, count, then time/string
    // pairs. END_OF_SEQUENCE uses the authored "end" key.
    pub(super) fn end_key(&self, nif: &NifFile) -> Result<Option<f32>> {
        let Some(block) = self.text_keys else { return Ok(None) };
        let Some(NiObject::Unknown(raw)) = nif.blocks.get(block as usize) else {
            anyhow::bail!("Missing raw animation text keys");
        };
        let bytes = &raw.raw_data;
        ensure!(raw.type_name == "NiTextKeyExtraData" && bytes.len() >= 8,
            "Invalid animation text keys");
        let word = |bytes: &[u8]| {
            let bytes = bytes.try_into().unwrap();
            if nif.source_is_little_endian { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) }
        };
        let count = word(&bytes[4..8]) as usize;
        ensure!(count.checked_mul(8) == Some(bytes.len() - 8), "Invalid animation text key count");
        let mut end = None;
        for key in bytes[8..].chunks_exact(8) {
            let time = f32::from_bits(word(&key[..4]));
            ensure!(time.is_finite(), "Nonfinite animation text key");
            let index = word(&key[4..]);
            if index != u32::MAX
                && nif.string_table.get(index as usize).context("Missing animation text key string")? == "end"
            {
                end = Some(end.map_or(time, |previous: f32| previous.min(time)));
            }
        }
        Ok(end)
    }
}

/// Sequence-data map ordering recovered from DOL803b54e4 and 803b71f0.
/// Caller supplies the manager's bucket count; construction policy and
/// controller-manager registration are separate from this container.
pub struct SequenceRegistry {
    bucket_count: std::num::NonZeroU32,
    buckets: std::collections::BTreeMap<u32, Vec<(u32, std::sync::Arc<Sequence>)>>,
}
impl Default for SequenceRegistry {
    fn default() -> Self {
        // Original actor constructor: DOL803b16ac/16cc. Bucket count is
        // independent of entry count; sequences are never capped at37.
        Self::new(std::num::NonZeroU32::new(37).unwrap())
    }
}
impl SequenceRegistry {
    pub const INVALID_ID: u32 = 0xffff_fffe;
    pub fn new(bucket_count: std::num::NonZeroU32) -> Self {
        Self {
            bucket_count,
            buckets: Default::default(),
        }
    }
    pub fn set(&mut self, id: u32, sequence: std::sync::Arc<Sequence>) {
        let bucket = self
            .buckets
            .entry(id % self.bucket_count.get())
            .or_default();
        if let Some(entry) = bucket.iter_mut().find(|entry| entry.0 == id) {
            entry.1 = sequence;
        } else {
            bucket.insert(0, (id, sequence));
        }
    }
    pub fn get(&self, id: u32) -> Option<&std::sync::Arc<Sequence>> {
        self.buckets
            .get(&(id % self.bucket_count.get()))?
            .iter()
            .find(|entry| entry.0 == id)
            .map(|entry| &entry.1)
    }
    pub fn remove(&mut self, id: u32) -> Option<std::sync::Arc<Sequence>> {
        let key = id % self.bucket_count.get();
        let bucket = self.buckets.get_mut(&key)?;
        let index = bucket.iter().position(|entry| entry.0 == id)?;
        let removed = bucket.remove(index).1;
        if bucket.is_empty() {
            self.buckets.remove(&key);
        }
        Some(removed)
    }
    pub fn find_name(&self, name: &str) -> u32 {
        self.buckets
            .values()
            .flat_map(|bucket| bucket.iter())
            .find(|(_, sequence)| sequence.name.as_deref() == Some(name))
            .map_or(Self::INVALID_ID, |(id, _)| *id)
    }
}

/// EM1 resource completion (REL2305b4..2305e8) obtains these IDs from
/// NiKFMTool's 37-bucket map, then requests each KF and registers its sequence.
/// Repeated serialized IDs replace the value in place; new IDs prepend.
pub fn kfm_load_order(
    manager: &epic_mickey_lib_rs::kfm_file::KfmFile,
) -> Vec<&epic_mickey_lib_rs::kfm_file::KfmSequence> {
    let mut buckets: std::collections::BTreeMap<
        u32,
        Vec<&epic_mickey_lib_rs::kfm_file::KfmSequence>,
    > = Default::default();
    for entry in &manager.sequences {
        let bucket = buckets.entry(entry.sequence_id % 37).or_default();
        if let Some(old) = bucket
            .iter_mut()
            .find(|old| old.sequence_id == entry.sequence_id)
        {
            *old = entry;
        } else {
            bucket.insert(0, entry);
        }
    }
    buckets.into_values().flatten().collect()
}

/// Immutable sequence resources prepared before controller/pose binding.
/// KF block links remain scoped to their owning file. This does not activate
/// sequences or replace the controller manager's registration lifecycle.
pub struct SequenceResources {
    registry: SequenceRegistry,
    sources: std::collections::BTreeMap<u32, std::sync::Arc<NifFile>>,
    manager: epic_mickey_lib_rs::kfm_file::KfmFile,
}

/// Effective KFM transition, with default-policy references resolved. Values
/// 4/5 in an edge are references to header policies, not playback blend modes.
pub struct TransitionPolicy<'a> {
    pub kind: u32,
    pub duration: f32,
    pub blend_pairs: &'a [epic_mickey_lib_rs::kfm_file::KfmBlendPair],
    pub chain: &'a [epic_mickey_lib_rs::kfm_file::KfmChainEntry],
}

pub fn resolve_transition(
    manager: &epic_mickey_lib_rs::kfm_file::KfmFile,
    source: u32,
    destination: u32,
) -> Result<Option<TransitionPolicy<'_>>> {
    let source_count = manager
        .sequences
        .iter()
        .filter(|sequence| sequence.sequence_id == source)
        .count();
    if source_count == 0 {
        return Ok(None);
    }
    // The shared decoder currently flattens transitions by source ID. It
    // cannot distinguish separate serialized definitions of a repeated ID.
    // Do not silently merge maps that the native loader replaces wholesale.
    ensure!(
        source_count == 1,
        "Repeated KFM source ID {source}: per-definition transition ownership is not preserved"
    );
    // DOL803f5154..5164 replaces the value of an existing destination key.
    let Some(edge) = manager
        .transitions
        .iter()
        .rev()
        .find(|edge| edge.src_id == source && edge.des_id == destination)
    else {
        return Ok(None);
    };
    // DOL803f4df4..4e10 stores a pointer to one of two shared default objects.
    // Missing edges are disallowed; their absence does NOT select a default.
    let (kind, duration, blend_pairs, chain) = match edge.trans_type {
        4 => (manager.sync_type, manager.sync_duration, &[][..], &[][..]),
        5 => (
            manager.nonsync_type,
            manager.nonsync_duration,
            &[][..],
            &[][..],
        ),
        _ => (
            edge.trans_type,
            edge.duration,
            edge.blend_pairs.as_slice(),
            edge.chain.as_slice(),
        ),
    };
    Ok(Some(TransitionPolicy {
        kind,
        duration,
        blend_pairs,
        chain,
    }))
}
/// Resolve a Wii virtual asset name, not a host Windows filesystem path.
/// Wii IsRelative (DOL80503544) recognizes only leading slash/backslash.
fn kf_asset_path(manager_path: &str, filename: &str) -> Result<String> {
    ensure!(!filename.is_empty(), "Empty KF filename");
    let filename = filename.replace('\\', "/");
    let manager = manager_path.replace('\\', "/");
    let joined = if filename.starts_with('/') {
        filename
    } else {
        match manager.rsplit_once('/') {
            Some((directory, _)) => format!("{directory}/{filename}"),
            None => filename,
        }
    };
    let mut parts = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                ensure!(parts.pop().is_some(), "KF path escapes the game asset root");
            }
            _ => parts.push(part),
        }
    }
    ensure!(!parts.is_empty(), "KF path resolves to asset root");
    Ok(parts.join("/"))
}
impl SequenceResources {
    pub fn load_from_assets(
        manager_path: &str,
        files: &[epic_mickey_lib_rs::packfile::VirtualFile],
    ) -> Result<Self> {
        let lookup = |path: &str| -> Result<&epic_mickey_lib_rs::packfile::VirtualFile> {
            let key = path.replace('\\', "/").to_ascii_lowercase();
            let exact=files.iter().find(|file|file.path.replace('\\', "/").eq_ignore_ascii_case(&key));
            if let Some(file)=exact{return Ok(file);}
            // PC KFM references retain the platform extension (.kf_pcr),
            // while the source pack index publishes the same resource as.kf.
            // An exact platform asset always wins over this logical-name alias.
            let fallback=if let Some(logical)=key.strip_suffix(".kf_pcr") {
                format!("{logical}.kf")
            }else{format!("{key}_wii")};
            files.iter().find(|file|file.path.replace('\\', "/").eq_ignore_ascii_case(&fallback))
                .with_context(|| format!("Missing animation asset {path}"))
        };
        let file = lookup(manager_path)?;
        let manager = epic_mickey_lib_rs::kfm_file::KfmFile::load_bytes(&file.data)
            .map_err(anyhow::Error::msg)?;
        Self::load(&manager, |filename| {
            let path = kf_asset_path(&file.path, filename)?;
            let source = lookup(&path)?;
            Ok(std::sync::Arc::new(
                NifFile::load_bytes(&source.data).map_err(anyhow::Error::msg)?,
            ))
        })
    }

    pub fn load(
        manager: &epic_mickey_lib_rs::kfm_file::KfmFile,
        mut resolve: impl FnMut(&str) -> Result<std::sync::Arc<NifFile>>,
    ) -> Result<Self> {
        let mut registry = SequenceRegistry::default();
        let mut sources = std::collections::BTreeMap::new();
        let mut files: std::collections::HashMap<String, std::sync::Arc<NifFile>> =
            Default::default();
        for entry in kfm_load_order(manager) {
            let source = if let Some(source) = files.get(&entry.filename) {
                source.clone()
            } else {
                let source = resolve(&entry.filename).with_context(|| {
                    format!(
                        "KF resource {} for sequence {}",
                        entry.filename, entry.sequence_id
                    )
                })?;
                files.insert(entry.filename.clone(), source.clone());
                source
            };
            let name = (!entry.sequence_name.is_empty()).then_some(entry.sequence_name.as_str());
            let sequence =
                Sequence::from_stream(&source, name, entry.anim_index)?.with_context(|| {
                    format!(
                        "Missing sequence {} ({:?}, index {}) in {}",
                        entry.sequence_id, name, entry.anim_index, entry.filename
                    )
                })?;
            registry.set(entry.sequence_id, std::sync::Arc::new(sequence));
            sources.insert(entry.sequence_id, source);
        }
        Ok(Self {
            registry,
            sources,
            manager: manager.clone(),
        })
    }
    /// Retain authored transition/default policies for actor playback. Merely
    /// preparing KF sequences must not discard the manager that connects them.
    pub fn manager(&self) -> &epic_mickey_lib_rs::kfm_file::KfmFile {
        &self.manager
    }
    pub fn transition(
        &self,
        source: u32,
        destination: u32,
    ) -> Result<Option<TransitionPolicy<'_>>> {
        resolve_transition(&self.manager, source, destination)
    }
    pub fn sequences(&self) -> impl Iterator<Item = (u32, &Sequence)> {
        self.sources.keys().filter_map(|&id| {
            self.registry
                .get(id)
                .map(|sequence| (id, sequence.as_ref()))
        })
    }
    pub fn find_name(&self, name: &str) -> u32 {
        self.registry.find_name(name)
    }
    pub fn get(&self, id: u32) -> Option<(&Sequence, &NifFile)> {
        Some((
            self.registry.get(id)?.as_ref(),
            self.sources.get(&id)?.as_ref(),
        ))
    }
}

/// Five fixed-string IDs are the native binding key; preserve nulls and case.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Binding {
    pub object: Option<String>,
    pub property: Option<String>,
    pub controller: Option<String>,
    pub controller_id: Option<String>,
    pub evaluator_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct MorphController {
    pub target: u32,
    pub target_count: u32,
    pub interpolators: Vec<Option<u32>>,
    pub target_names: Vec<Option<String>>,
}

#[derive(Debug, serde::Serialize)]
pub struct MorphElement {
    pub semantic: String,
    pub index: u32,
    pub normalize: u32,
}
#[derive(Debug, serde::Serialize)]
pub struct MorphModifier {
    pub submit_points: Vec<u16>,
    pub complete_points: Vec<u16>,
    pub flags: u8,
    pub target_count: u16,
    pub elements: Vec<MorphElement>,
}
impl MorphModifier {
    pub fn load(nif: &NifFile, block: u32) -> Result<Self> {
        ensure!(
            nif.version == 0x14060500,
            "Unverified morph-modifier version"
        );
        let NiObject::Unknown(raw) = nif
            .blocks
            .get(block as usize)
            .context("Missing morph modifier")?
        else {
            anyhow::bail!("Expected raw morph modifier")
        };
        ensure!(
            raw.type_name == "NiMorphMeshModifier",
            "Expected morph mesh modifier"
        );
        let mut at = 0usize;
        let mut read = |width: usize| -> Result<u32> {
            let bytes = raw
                .raw_data
                .get(at..at + width)
                .context("Truncated morph modifier")?;
            at += width;
            Ok(if nif.source_is_little_endian {
                bytes.iter().rev().fold(0, |v, b| (v << 8) | u32::from(*b))
            } else {
                bytes.iter().fold(0, |v, b| (v << 8) | u32::from(*b))
            })
        };
        let mut sync = Vec::new();
        for _ in 0..2 {
            let count = read(4)? as usize;
            ensure!(
                count <= raw.raw_data.len() / 2,
                "Morph synchronization count exceeds block"
            );
            sync.push(
                (0..count)
                    .map(|_| Ok(read(2)? as u16))
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        let flags = read(1)? as u8;
        let target_count = read(2)? as u16;
        let count = read(4)? as usize;
        ensure!(
            count <= raw.raw_data.len() / 12,
            "Morph element count exceeds block"
        );
        let mut elements = Vec::with_capacity(count);
        for _ in 0..count {
            let semantic = nif
                .string_table
                .get(read(4)? as usize)
                .context("Missing morph semantic")?
                .clone();
            elements.push(MorphElement {
                semantic,
                index: read(4)?,
                normalize: read(4)?,
            });
        }
        ensure!(at == raw.raw_data.len(), "Trailing morph modifier bytes");
        let complete_points = sync.pop().unwrap();
        let submit_points = sync.pop().unwrap();
        Ok(Self {
            submit_points,
            complete_points,
            flags,
            target_count,
            elements,
        })
    }
}

/// Decode a named float32 position stream using its authored submesh region.
/// No preview fallback values are permitted in runtime animation resources.
pub fn position_stream(
    nif: &NifFile,
    mesh: u32,
    semantic: &str,
    index: u32,
    submesh: usize,
) -> Result<Vec<[f32; 3]>> {
    let Some(NiObject::NiMesh(mesh)) = nif.blocks.get(mesh as usize) else {
        anyhow::bail!("Missing morph mesh")
    };
    ensure!(
        submesh < usize::from(mesh.submesh_count),
        "Morph submesh outside mesh"
    );
    let matches = mesh
        .stream_refs
        .iter()
        .flat_map(|r| {
            r.semantics
                .iter()
                .enumerate()
                .filter(move |(_, s)| s.name == semantic && s.index == index)
                .map(move |(i, _)| (r, i))
        })
        .collect::<Vec<_>>();
    ensure!(
        matches.len() == 1,
        "Expected one {semantic}{index} stream, found {}",
        matches.len()
    );
    let (reference, component) = matches[0];
    ensure!(
        reference.is_per_instance == 0,
        "Instanced morph stream requires instance binding"
    );
    let Some(NiObject::NiDataStream(stream)) = nif.blocks.get(reference.stream_link as usize)
    else {
        anyhow::bail!("Missing morph data stream")
    };
    ensure!(
        stream.component_formats.len() == reference.semantics.len(),
        "Morph semantic/format count mismatch"
    );
    ensure!(
        stream.component_formats[component] == 0x00030437,
        "Unsupported morph position format {:08x}",
        stream.component_formats[component]
    );
    let stride = epic_mickey_lib_rs::nif_file::vertex_stride(stream);
    let offset = epic_mickey_lib_rs::nif_file::sem_offset(stream, component);
    let region = stream
        .regions
        .get(usize::from(
            *reference
                .submesh_region_indices
                .get(submesh)
                .context("Missing morph region mapping")?,
        ))
        .context("Missing morph region")?;
    let end = (region.start_index as usize)
        .checked_add(region.count as usize)
        .context("Morph region overflow")?;
    ensure!(
        stride > 0 && end <= stream.buffer.len() / stride,
        "Morph region exceeds stream buffer"
    );
    let mut positions = Vec::with_capacity(region.count as usize);
    for vertex in region.start_index as usize..end {
        let mut p = [0.; 3];
        for (axis, value) in p.iter_mut().enumerate() {
            let at = vertex * stride + offset + axis * 4;
            let bytes = stream
                .buffer
                .get(at..at + 4)
                .context("Truncated morph position")?
                .try_into()?;
            *value = if nif.source_is_little_endian {
                f32::from_le_bytes(bytes)
            } else {
                f32::from_be_bytes(bytes)
            };
            ensure!(value.is_finite(), "Nonfinite morph position");
        }
        positions.push(p);
    }
    Ok(positions)
}

/// Original non-normalizing float32 morph path: selection8057beec..bf48,
/// accumulation8057bf88..c434. Caller supplies committed weights (including
/// the base weight for relative targets). No normalization or weight clamping.
pub fn morph_positions(targets: &[Vec<[f32; 3]>], weights: &[f32]) -> Result<Vec<[f32; 3]>> {
    ensure!(
        !targets.is_empty() && targets.len() == weights.len(),
        "Morph target/weight count mismatch"
    );
    let count = targets[0].len();
    ensure!(
        targets.iter().all(|t| t.len() == count),
        "Morph vertex count mismatch"
    );
    ensure!(
        weights.iter().all(|w| w.is_finite()),
        "Nonfinite morph weight"
    );
    let epsilon = f32::from_bits(0x3a83126f);
    let mut output = vec![[0f32; 3]; count];
    for (target, &weight) in targets.iter().zip(weights) {
        if weight < epsilon && weight > -epsilon {
            continue;
        }
        for (out, position) in output.iter_mut().zip(target) {
            for axis in 0..3 {
                ensure!(position[axis].is_finite(), "Nonfinite active morph target");
                out[axis] = weight.mul_add(position[axis], out[axis]);
                ensure!(out[axis].is_finite(), "Morph accumulation overflow");
            }
        }
    }
    Ok(output)
}

/// Expand authored source positions to the display-list vertex layout.
/// Source IDs are absolute stream indices; samples cover one decoded region.
pub fn render_positions(
    template: &[f32],
    sources: &[u32],
    first_source: u32,
    positions: &[[f32; 3]],
    mesh_to_model: [[f32; 4]; 4],
) -> Result<(Vec<f32>, [f32; 3], [f32; 3])> {
    ensure!(
        !sources.is_empty() && template.len() % 12 == 0 && template.len() / 12 == sources.len(),
        "Morph render/source vertex count mismatch"
    );
    ensure!(
        mesh_to_model.iter().flatten().all(|v| v.is_finite())
            && mesh_to_model[3] == [0., 0., 0., 1.],
        "Invalid morph mesh transform"
    );
    let mut output = template.to_vec();
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    for (vertex, &source) in output.chunks_exact_mut(12).zip(sources) {
        let local = source
            .checked_sub(first_source)
            .context("Morph source precedes decoded region")?;
        let p = positions
            .get(local as usize)
            .context("Morph source outside decoded region")?;
        ensure!(
            p.iter().all(|v| v.is_finite()),
            "Nonfinite morph source position"
        );
        for axis in 0..3 {
            vertex[axis] = ((0..3)
                .map(|i| f64::from(mesh_to_model[axis][i]) * f64::from(p[i]))
                .sum::<f64>()
                + f64::from(mesh_to_model[axis][3])) as f32;
            ensure!(vertex[axis].is_finite(), "Morph render position overflow");
            minimum[axis] = minimum[axis].min(vertex[axis]);
            maximum[axis] = maximum[axis].max(vertex[axis]);
        }
    }
    Ok((output, minimum, maximum))
}

/// One sequence's complete position-morph binding. Scheduling/blending and
/// conversion from source vertices to cooked draw vertices remain the caller's.
/// Explicit single-sequence position-only preview. Does not execute a KFM,
/// transform channels, events, blending, or gameplay activation.
pub fn preview_positions(
    model_bytes: &[u8],
    keyframe_bytes: &[u8],
    name: &str,
    time: f32,
) -> Result<std::collections::BTreeMap<u32, (u32, Vec<[f32; 3]>)>> {
    let model = NifFile::load_bytes(model_bytes).map_err(anyhow::Error::msg)?;
    let keyframes = NifFile::load_bytes(keyframe_bytes).map_err(anyhow::Error::msg)?;
    let sequence = Sequence::from_stream(&keyframes, Some(name), 0)?
        .with_context(|| format!("Missing top-level diagnostic sequence {name}"))?;
    let mut output = std::collections::BTreeMap::new();
    for (id, block) in model.blocks.iter().enumerate() {
        if block.type_name() != "NiMorphWeightsController" {
            continue;
        }
        let controller = MorphController::load(&model, id as u32)?;
        let Some(NiObject::NiMesh(mesh)) = model.blocks.get(controller.target as usize) else {
            anyhow::bail!("Missing morph mesh")
        };
        ensure!(
            mesh.submesh_count == 1,
            "Diagnostic morph requires a single submesh; multiple region mapping is unimplemented"
        );
        let streams = mesh
            .stream_refs
            .iter()
            .filter(|r| {
                r.semantics
                    .iter()
                    .any(|s| s.name == "POSITION" && s.index == 0)
            })
            .collect::<Vec<_>>();
        ensure!(streams.len() == 1, "Ambiguous base morph stream");
        let reference = streams[0];
        let Some(NiObject::NiDataStream(stream)) = model.blocks.get(reference.stream_link as usize)
        else {
            anyhow::bail!("Missing base morph stream")
        };
        let region = stream
            .regions
            .get(usize::from(
                *reference
                    .submesh_region_indices
                    .first()
                    .context("Missing morph region")?,
            ))
            .context("Missing morph region")?;
        let positions =
            BoundMorph::bind(&model, id as u32, &keyframes, &sequence, 0)?.sample(time)?;
        ensure!(
            positions.len() == region.count as usize,
            "Morph/source position region size mismatch"
        );
        ensure!(
            output
                .insert(controller.target, (region.start_index, positions))
                .is_none(),
            "Multiple morph controllers target one mesh"
        );
    }
    ensure!(
        !output.is_empty(),
        "Diagnostic model contains no morph controllers"
    );
    Ok(output)
}
#[derive(Debug)]
pub struct BoundMorph {
    targets: std::sync::Arc<Vec<Vec<[f32; 3]>>>,
    source_start: u32,
    channels: Vec<FloatEvaluator>,
    relative: bool,
    duration: f32,
}
impl BoundMorph {
    pub fn bind(
        model: &NifFile,
        controller_block: u32,
        keyframes: &NifFile,
        sequence: &Sequence,
        submesh: usize,
    ) -> Result<Self> {
        let controller = MorphController::load(model, controller_block)?;
        let Some(NiObject::NiMesh(mesh)) = model.blocks.get(controller.target as usize) else {
            anyhow::bail!("Missing morph mesh")
        };
        let name = &mesh.base.base.base.name;
        let mut evaluators = Vec::new();
        for &id in &sequence.evaluators {
            let block = keyframes
                .blocks
                .get(id as usize)
                .context("Missing sequence evaluator")?;
            if !matches!(
                block.type_name(),
                "NiConstFloatEvaluator" | "NiFloatEvaluator" | "NiBSplineCompFloatEvaluator"
            ) {
                continue;
            }
            let evaluator = FloatEvaluator::load(keyframes, id)?;
            if evaluator.binding.object.as_deref() == Some(name)
                && evaluator.binding.controller.as_deref() == Some("NiMorphWeightsController")
            {
                evaluators.push(evaluator);
            }
        }
        Self::bind_channels(
            model,
            &controller,
            evaluators,
            sequence.duration,
            submesh,
            None,
        )
    }
    fn bind_channels(
        model: &NifFile,
        controller: &MorphController,
        evaluators: Vec<FloatEvaluator>,
        duration: f32,
        submesh: usize,
        shared: Option<std::sync::Arc<Vec<Vec<[f32; 3]>>>>,
    ) -> Result<Self> {
        let Some(NiObject::NiMesh(mesh)) = model.blocks.get(controller.target as usize) else {
            anyhow::bail!("Missing morph mesh")
        };
        let positions = mesh
            .stream_refs
            .iter()
            .filter(|r| {
                r.semantics
                    .iter()
                    .any(|s| s.name == "POSITION" && s.index == 0)
            })
            .collect::<Vec<_>>();
        ensure!(
            positions.len() == 1,
            "Ambiguous morph destination position stream"
        );
        let reference = positions[0];
        ensure!(
            reference.is_per_instance == 0,
            "Instanced morph destination requires instance binding"
        );
        let Some(NiObject::NiDataStream(stream)) = model.blocks.get(reference.stream_link as usize)
        else {
            anyhow::bail!("Missing morph destination stream")
        };
        let component = reference
            .semantics
            .iter()
            .position(|s| s.name == "POSITION" && s.index == 0)
            .unwrap();
        ensure!(
            stream.component_formats.get(component) == Some(&0x00030437),
            "Unsupported morph destination position format"
        );
        let region = stream
            .regions
            .get(usize::from(
                *reference
                    .submesh_region_indices
                    .get(submesh)
                    .context("Missing morph destination region mapping")?,
            ))
            .context("Missing morph destination region")?;
        let modifiers = mesh
            .modifiers
            .iter()
            .flatten()
            .filter(|&&id| {
                model
                    .blocks
                    .get(id as usize)
                    .is_some_and(|b| b.type_name() == "NiMorphMeshModifier")
            })
            .copied()
            .collect::<Vec<_>>();
        ensure!(modifiers.len() == 1, "Expected one morph modifier");
        let modifier = MorphModifier::load(model, modifiers[0])?;
        ensure!(
            u32::from(modifier.target_count) == controller.target_count,
            "Morph controller/modifier target count mismatch"
        );
        ensure!(
            modifier.elements.len() == 1
                && modifier.elements[0].semantic == "POSITION"
                && modifier.elements[0].index == 0
                && modifier.elements[0].normalize == 0,
            "Morph binding requires the non-normalizing POSITION0 kernel"
        );
        let mut channels = std::collections::BTreeMap::new();
        for evaluator in evaluators {
            ensure!(
                evaluator.binding.property.is_none() && evaluator.binding.controller_id.is_none(),
                "Qualified morph controller binding requires controller-ID resolution"
            );
            let slot = controller.target_index(
                evaluator
                    .binding
                    .evaluator_id
                    .as_deref()
                    .context("Missing morph evaluator target name")?,
            )?;
            ensure!(
                matches!(evaluator.channel_types, [3, 0, 0, 0] | [67, 0, 0, 0]),
                "Unsupported float evaluator channel state"
            );
            ensure!(
                channels.insert(slot, evaluator).is_none(),
                "Duplicate morph evaluator target"
            );
        }
        let channels = (0..usize::from(modifier.target_count))
            .map(|slot| {
                channels
                    .remove(&slot)
                    .context("Sequence does not supply every morph target")
            })
            .collect::<Result<Vec<_>>>()?;
        let targets = if let Some(shared) = shared {
            shared
        } else {
            std::sync::Arc::new(
                (0..u32::from(modifier.target_count))
                    .map(|index| {
                        position_stream(model, controller.target, "MORPH_POSITION", index, submesh)
                    })
                    .collect::<Result<Vec<_>>>()?,
            )
        };
        ensure!(
            targets
                .iter()
                .all(|target| target.len() == region.count as usize),
            "Morph target/destination region size mismatch"
        );
        Ok(Self {
            targets,
            source_start: region.start_index,
            channels,
            relative: modifier.flags & 1 != 0,
            duration,
        })
    }
    pub fn sample(&self, time: f32) -> Result<Vec<[f32; 3]>> {
        ensure!(
            time.is_finite() && time >= 0. && time <= self.duration,
            "Morph sample outside sequence interval"
        );
        let mut weights = self
            .channels
            .iter()
            .map(|channel| match &channel.value {
                FloatValue::Constant(value) => Ok(*value),
                FloatValue::Keys(Some(track)) => track
                    .sample_scalar(time)?
                    .context("Morph evaluator has no value"),
                FloatValue::Keys(None) => anyhow::bail!("Morph evaluator has no data"),
                FloatValue::Spline(track) => Ok(track.sample::<1>(time)?[0]),
            })
            .collect::<Result<Vec<_>>>()?;
        if self.relative {
            *weights
                .first_mut()
                .context("Relative morph has no base target")? = 1.;
        }
        morph_positions(&self.targets, &weights)
    }
}
impl MorphController {
    pub fn load(nif: &NifFile, block: u32) -> Result<Self> {
        ensure!(
            nif.version == 0x14060500,
            "Unverified morph-controller version"
        );
        let NiObject::Unknown(raw) = nif
            .blocks
            .get(block as usize)
            .context("Missing morph controller")?
        else {
            anyhow::bail!("Expected raw morph controller")
        };
        ensure!(
            raw.type_name == "NiMorphWeightsController",
            "Expected morph weights controller"
        );
        let word = |at: usize| -> Result<u32> {
            let b = raw
                .raw_data
                .get(at..at + 4)
                .context("Truncated morph controller")?
                .try_into()?;
            Ok(if nif.source_is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            })
        };
        let target = word(22)?;
        ensure!(
            matches!(nif.blocks.get(target as usize), Some(NiObject::NiMesh(_))),
            "Morph controller target is not a mesh"
        );
        let target_count = word(26)?;
        let count = word(30)? as usize;
        ensure!(
            count <= raw.raw_data.len().saturating_sub(38) / 4,
            "Morph interpolator count exceeds block"
        );
        let mut interpolators = Vec::with_capacity(count);
        for i in 0..count {
            let id = word(34 + i * 4)?;
            interpolators.push(if id == u32::MAX {
                None
            } else {
                ensure!(
                    nif.blocks
                        .get(id as usize)
                        .context("Missing morph interpolator")?
                        .type_name()
                        .ends_with("Interpolator"),
                    "Invalid morph interpolator type"
                );
                Some(id)
            });
        }
        let names_at = 34 + count * 4;
        let names = word(names_at)? as usize;
        ensure!(
            names <= raw.raw_data.len().saturating_sub(names_at + 4) / 4,
            "Morph name count exceeds block"
        );
        ensure!(
            raw.raw_data.len() == names_at + 4 + names * 4,
            "Trailing morph-controller bytes"
        );
        let target_names = (0..names)
            .map(|i| -> Result<Option<String>> {
                let id = word(names_at + 4 + i * 4)?;
                if id == u32::MAX {
                    Ok(None)
                } else {
                    Ok(Some(
                        nif.string_table
                            .get(id as usize)
                            .context("Missing morph target name")?
                            .clone(),
                    ))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            target,
            target_count,
            interpolators,
            target_names,
        })
    }
    /// Exact authored target lookup, after the model object/controller binding
    /// has selected this controller. Never bind by evaluator array position.
    pub fn target_index(&self, name: &str) -> Result<usize> {
        let matches = self
            .target_names
            .iter()
            .enumerate()
            .filter(|(_, n)| n.as_deref() == Some(name))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "Morph target {name:?} has {} matches",
            matches.len()
        );
        ensure!(
            matches[0] < (self.target_count as usize),
            "Morph target name exceeds target count"
        );
        Ok(matches[0])
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub enum FloatValue {
    Constant(f32),
    Spline(spline::CompactSpline),
    Keys(Option<super::nif_transform::Track>),
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct FloatEvaluator {
    pub binding: Binding,
    pub channel_types: [u8; 4],
    pub value: FloatValue,
}

#[derive(Debug, serde::Serialize)]
pub struct TransformEvaluator {
    pub binding: Binding,
    pub channel_types: [u8; 4],
    /// Retains invalid-channel sentinels; no identity substitution.
    pub transform: super::nif_transform::Interpolator,
    splines: Option<[Option<spline::CompactSpline>; 3]>,
}

/// Default manager palette. EM1 DOL 0x8045353c checks exact-name aliases first,
/// then invokes the scene root's name search and caches successful results.
#[derive(Debug)]
pub struct ObjectPalette {
    pub scene: Option<u32>,
    objects: std::collections::BTreeMap<String, u32>,
}
impl ObjectPalette {
    pub fn new(scene: Option<u32>) -> Self {
        Self {
            scene,
            objects: Default::default(),
        }
    }
    /// DOL 0x80452ed8: a null target removes the alias, enabling fallback again.
    pub fn set(&mut self, name: &str, target: Option<u32>) {
        if let Some(target) = target {
            self.objects.insert(name.into(), target);
        } else {
            self.objects.remove(name);
        }
    }
    pub fn get(
        &mut self,
        hierarchy: &super::scene_bounds::Hierarchy,
        name: &str,
    ) -> Result<Option<u32>> {
        ensure!(
            !name.contains('\0'),
            "Palette lookup name contains an embedded NUL"
        );
        if let Some(&target) = self.objects.get(name) {
            return Ok(Some(target));
        }
        let target = self
            .scene
            .map(|root| hierarchy.find_named_node(root, name))
            .transpose()?
            .flatten();
        if let Some(target) = target {
            self.set(name, Some(target));
        }
        Ok(target)
    }
    /// NiDefaultAVObjectPalette::LoadBinary, DOL 0x804532d0. Palette strings
    /// are inline signed-length C strings, not NIF string-table references.
    pub fn load(nif: &NifFile, id: u32) -> Result<Self> {
        ensure!(
            nif.version == 0x14060500,
            "Unverified palette format version"
        );
        let Some(NiObject::Unknown(raw)) = nif.blocks.get(id as usize) else {
            anyhow::bail!("Missing raw object palette")
        };
        ensure!(
            raw.type_name == "NiDefaultAVObjectPalette",
            "Unsupported object palette class"
        );
        let bytes = &raw.raw_data;
        let word = |cursor: &mut usize| -> Result<u32> {
            let data = bytes
                .get(*cursor..*cursor + 4)
                .context("Truncated object palette")?;
            *cursor += 4;
            Ok(if nif.source_is_little_endian {
                u32::from_le_bytes(data.try_into()?)
            } else {
                u32::from_be_bytes(data.try_into()?)
            })
        };
        let link = |id: u32| -> Result<Option<u32>> {
            if id == u32::MAX {
                return Ok(None);
            }
            ensure!(
                nif.blocks
                    .get(id as usize)
                    .and_then(NiObject::as_av_object)
                    .is_some(),
                "Palette target {id} is not an AVObject"
            );
            Ok(Some(id))
        };
        let mut cursor = 0;
        let mut result = Self::new(link(word(&mut cursor)?)?);
        let count = word(&mut cursor)? as usize;
        ensure!(
            count <= (bytes.len() - cursor) / 8,
            "Truncated palette entries"
        );
        for _ in 0..count {
            let length = word(&mut cursor)? as i32;
            let name = if length > 0 {
                let end = cursor
                    .checked_add(length as usize)
                    .context("Palette string overflow")?;
                let data = bytes.get(cursor..end).context("Truncated palette string")?;
                cursor = end;
                // Native strcmp/hash stop at the first NUL even if the stream
                // supplied additional bytes in the declared string length.
                let data = data.split(|&b| b == 0).next().unwrap_or_default();
                Some(std::str::from_utf8(data).context("Non-UTF8 palette name")?)
            } else {
                None
            };
            let target = link(word(&mut cursor)?)?;
            if let (Some(name), Some(target)) = (name, target) {
                result.set(name, Some(target));
            }
        }
        ensure!(
            cursor == bytes.len(),
            "Unexpected object palette trailing data"
        );
        Ok(result)
    }
}

/// Shared bindings for actor-local transforms and supported position morphs.
#[derive(Debug)]
pub struct ActorBindings {
    pub actor_root: u32,
    pub sequences: std::collections::BTreeMap<u32, BoundTransforms>,
    pub morph_sequences:
        std::collections::BTreeMap<u32, std::collections::BTreeMap<(u32, usize), BoundMorph>>,
    property_sequences: std::collections::BTreeMap<u32, Vec<super::nif_properties::Channel>>,
    /// These channels are not handled by the prepared pose handlers.
    pub other_evaluators: std::collections::BTreeSet<String>,
}
#[derive(Debug)]
pub struct ActorPose {
    pub colors: std::collections::BTreeMap<(u32, usize), [f32; 4]>,
    pub visibility: std::collections::BTreeMap<u32, f32>,
    pub transforms: std::collections::BTreeMap<u32, super::nif_transform::Pose>,
    /// Mesh/submesh -> absolute source vertex start and sampled local positions.
    pub morphs: std::collections::BTreeMap<(u32, usize), (u32, Vec<[f32; 3]>)>,
}
impl ActorBindings {
    /// ChangeNIFRoot (DOL 0x803b9a38) searches the KFM model-root name beneath
    /// the supplied NIF root and constructs a NEW manager. Its Init creates a
    /// fresh default palette rooted there (0x803dfa94..0x803dfb88).
    pub fn prepare(
        resources: &SequenceResources,
        model: &NifFile,
        hierarchy: &super::scene_bounds::Hierarchy,
        nif_root: u32,
    ) -> Result<Self> {
        let name = &resources.manager().model_root;
        let actor_root = hierarchy
            .find_named_node(nif_root, name)?
            .with_context(|| format!("KFM model root {name:?} is missing"))?;
        let mut palette = ObjectPalette::new(Some(actor_root));
        let mut sequences = std::collections::BTreeMap::new();
        let mut morph_sequences = std::collections::BTreeMap::new();
        let mut property_sequences = std::collections::BTreeMap::new();
        let mut geometry = std::collections::BTreeMap::new();
        let mut other_evaluators = std::collections::BTreeSet::new();
        for (id, _) in resources.sequences() {
            let (sequence, keyframes) = resources.get(id).context("Missing prepared sequence")?;
            let mut morph_channels = std::collections::BTreeMap::<u32, Vec<FloatEvaluator>>::new();
            let mut properties = Vec::new();
            for &block in &sequence.evaluators {
                let evaluator = keyframes
                    .blocks
                    .get(block as usize)
                    .context("Missing actor evaluator")?;
                if matches!(evaluator.type_name(), "NiTransformEvaluator" | "NiBSplineCompTransformEvaluator") {
                    continue;
                }
                // Gamebryo also uses property evaluators for non-render
                // component state (for example NiPSEmitParticlesCtlr's
                // EmitterActive/BirthRate channels). A qualified binding is
                // outside the color/visibility handlers below, but must not
                // prevent this sequence's transform evaluators from binding.
                // Keep parsing errors for the supported, unqualified channel
                // formats; only the explicitly unsupported qualification is
                // omitted from this actor pose.
                let property_channel =
                    match super::nif_properties::Channel::load(keyframes, block, |name| {
                        palette.get(hierarchy, name)
                    }) {
                        Ok(channel) => channel,
                        Err(error)
                            if error.to_string()
                                == "Qualified property evaluator requires binding resolution" =>
                        {
                            other_evaluators.insert(format!(
                                "{} (qualified property binding)",
                                evaluator.type_name()
                            ));
                            None
                        }
                        Err(error) => return Err(error),
                    };
                if let Some(channel) = property_channel {
                    ensure!(
                        !properties
                            .iter()
                            .any(
                                |p: &super::nif_properties::Channel| p.target == channel.target
                                    && p.slot == channel.slot
                            ),
                        "Duplicate animated property channel"
                    );
                    properties.push(channel);
                    continue;
                }
                if matches!(
                    evaluator.type_name(),
                    "NiConstFloatEvaluator" | "NiFloatEvaluator" | "NiBSplineCompFloatEvaluator"
                ) {
                    let channel = FloatEvaluator::load(keyframes, block)?;
                    if channel.binding.controller.as_deref() == Some("NiMorphWeightsController") {
                        ensure!(
                            channel.binding.property.is_none()
                                && channel.binding.controller_id.is_none(),
                            "Qualified morph binding requires controller-ID resolution"
                        );
                        let name = channel
                            .binding
                            .object
                            .as_deref()
                            .context("Missing morph object name")?;
                        let target = palette
                            .get(hierarchy, name)?
                            .with_context(|| format!("Missing morph palette target {name:?}"))?;
                        let node = hierarchy
                            .nodes
                            .get(&target)
                            .context("Morph palette target outside model hierarchy")?;
                        let controllers = node
                            .controllers
                            .iter()
                            .filter(|c| c.class_name == "NiMorphWeightsController")
                            .collect::<Vec<_>>();
                        ensure!(
                            controllers.len() == 1,
                            "Morph target requires unambiguous controller resolution"
                        );
                        ensure!(
                            controllers[0].target == Some(target),
                            "Morph controller target differs from palette target"
                        );
                        morph_channels
                            .entry(controllers[0].block)
                            .or_default()
                            .push(channel);
                        continue;
                    }
                }
                other_evaluators.insert(evaluator.type_name().to_owned());
            }
            let mut morphs = std::collections::BTreeMap::new();
            for (controller_id, channels) in morph_channels {
                let controller = MorphController::load(model, controller_id)?;
                let Some(NiObject::NiMesh(mesh)) = model.blocks.get(controller.target as usize)
                else {
                    anyhow::bail!("Missing morph target mesh")
                };
                ensure!(mesh.submesh_count > 0, "Morph mesh has no submeshes");
                for submesh in 0..usize::from(mesh.submesh_count) {
                    let key = (controller.target, submesh);
                    let bound = BoundMorph::bind_channels(
                        model,
                        &controller,
                        channels.clone(),
                        sequence.duration,
                        submesh,
                        geometry.get(&key).cloned(),
                    )?;
                    geometry.entry(key).or_insert_with(|| bound.targets.clone());
                    ensure!(
                        morphs.insert(key, bound).is_none(),
                        "Multiple morph controllers write one mesh submesh"
                    );
                }
            }
            morph_sequences.insert(id, morphs);
            property_sequences.insert(id, properties);
            sequences.insert(
                id,
                BoundTransforms::bind(keyframes, sequence, |name| palette.get(hierarchy, name))
                    .with_context(|| format!("Binding sequence {id} ({:?})", sequence.name))?,
            );
        }
        Ok(Self {
            actor_root,
            sequences,
            morph_sequences,
            property_sequences,
            other_evaluators,
        })
    }
    /// One clip-time snapshot for both handlers. This does not activate clips,
    /// dispatch events, apply cumulative motion or blend concurrent sequences.
    pub fn sample(&self, id: u32, time: f32) -> Result<ActorPose> {
        // Apply every evaluator this runtime understands. An unsupported
        // channel remains visible through `other_evaluators`, but must not
        // cancel the whole authored sequence and strand its Lua coroutine.
        let transforms = self
            .sequences
            .get(&id)
            .context("Missing actor sequence binding")?
            .sample(time)?;
        let morphs = self
            .morph_sequences
            .get(&id)
            .context("Missing actor morph bindings")?
            .iter()
            .map(|(&key, bound)| Ok((key, (bound.source_start, bound.sample(time)?))))
            .collect::<Result<_>>()?;
        let mut colors = std::collections::BTreeMap::new();
        let mut visibility = std::collections::BTreeMap::new();
        for channel in &self.property_sequences[&id] {
            use super::nif_properties::Value;
            match &channel.value {
                Value::Color(track) => {
                    if let Some(color) = track.sample_vector::<4>(time)? {
                        colors.insert((channel.target, channel.slot.unwrap()), color);
                    }
                }
                Value::ConstColor(color) => {
                    colors.insert((channel.target, channel.slot.unwrap()), *color);
                }
                Value::Visibility(value) => {
                    visibility.insert(channel.target, *value);
                }
                Value::VisibilityKeys(keys) => {
                    if let Some(value)=keys.sample(time)? {visibility.insert(channel.target,value);}
                }
            }
        }
        Ok(ActorPose {
            transforms,
            morphs,
            colors,
            visibility,
        })
    }
}

#[derive(Debug)]
pub struct BoundTransforms {
    channels: Vec<(u32, TransformEvaluator)>,
    duration: f32,
}
impl BoundTransforms {
    /// The controller manager supplies its palette lookup (including aliases).
    /// Never flatten the model into a name map: that loses duplicate-name order
    /// and serialized palette overrides. No name searches occur during sample.
    pub fn bind(
        keyframes: &NifFile,
        sequence: &Sequence,
        mut resolve: impl FnMut(&str) -> Result<Option<u32>>,
    ) -> Result<Self> {
        let mut channels = Vec::new();
        let mut owned = std::collections::BTreeMap::<u32, [bool; 3]>::new();
        for &id in &sequence.evaluators {
            let block = keyframes
                .blocks
                .get(id as usize)
                .context("Missing sequence evaluator")?;
            if !matches!(block.type_name(), "NiTransformEvaluator" | "NiBSplineCompTransformEvaluator") {
                continue;
            }
            let evaluator = TransformEvaluator::load(keyframes, id)?;
            ensure!(
                evaluator.binding.controller.as_deref() == Some("NiTransformController"),
                "Transform evaluator {id} targets a different controller"
            );
            ensure!(
                evaluator.channel_types[3] & 1 == 0,
                "Referenced transform evaluator {id} requires a referenced pose handler"
            );
            let name = evaluator
                .binding
                .object
                .as_deref()
                .context("Transform evaluator has no object name")?;
            let target = resolve(name)?.with_context(|| {
                format!(
                    "Transform evaluator {id} target {name:?} is missing from the actor palette"
                )
            })?;
            let mask = owned.entry(target).or_default();
            for index in 0..3 {
                if evaluator.channel_types[index] != 0 {
                    ensure!(!mask[index],"Overlapping transform evaluators for node {target}, channel {index} require pose-binding resolution");
                    mask[index] = true;
                }
            }
            channels.push((target, evaluator));
        }
        Ok(Self {
            channels,
            duration: sequence.duration,
        })
    }
    /// Sample clip time already computed by the sequence clock. Returned local
    /// channels feed Hierarchy::base_world_transforms_with_poses; no root-motion
    /// accumulation, priority blending, LOD or lifecycle is implied here.
    pub fn sample(
        &self,
        time: f32,
    ) -> Result<std::collections::BTreeMap<u32, super::nif_transform::Pose>> {
        ensure!(
            time.is_finite() && time >= 0. && time <= self.duration,
            "Transform sample outside sequence interval"
        );
        let mut poses = std::collections::BTreeMap::new();
        for (target, evaluator) in &self.channels {
            let sampled = evaluator.sample(time)?;
            let pose = poses.entry(*target).or_insert(super::nif_transform::Pose {
                translation: None,
                rotation: None,
                scale: None,
            });
            if sampled.translation.is_some() {
                pose.translation = sampled.translation;
            }
            if sampled.rotation.is_some() {
                pose.rotation = sampled.rotation;
            }
            if sampled.scale.is_some() {
                pose.scale = sampled.scale;
            }
        }
        Ok(poses)
    }
}

impl TransformEvaluator {
    /// Channel dispatch follows NiTransformEvaluator's invalid/posed/keyed
    /// distinction. Interpolation arithmetic retains the sampler's documented
    /// limits; this is not yet the controller manager's pose blend pipeline.
    pub fn sample(&self, time: f32) -> Result<super::nif_transform::Pose> {
        ensure!(
            self.channel_types[3] & 2 != 0,
            "Missing transform evaluator flag"
        );
        let mut active = [false; 3];
        let mut animated = [false; 3];
        for (index, kind) in [4, 5, 3].into_iter().enumerate() {
            let state = self.channel_types[index];
            ensure!(
                state == 0 || state == kind || state == (kind | 0x40),
                "Unsupported transform evaluator channel {index}: {state:#x}"
            );
            active[index] = state != 0;
            animated[index] = state == kind;
        }
        if let Some(splines) = &self.splines {
            let posed = std::array::from_fn(|i| active[i] && !animated[i]);
            let mut pose = self.transform.sample_channels(time, posed, [false; 3])?;
            if let Some(translation) = &splines[0] {
                pose.translation = Some(translation.sample(time)?);
            }
            if let Some(rotation) = &splines[1] {
                pose.rotation = Some(super::nif_quaternion::normalize(rotation.sample(time)?)?);
            }
            if let Some(scale) = &splines[2] {
                pose.scale = Some(scale.sample::<1>(time)?[0]);
            }
            return Ok(pose);
        }
        if animated.iter().any(|v| *v) {
            let channels = self
                .transform
                .channels
                .as_ref()
                .context("Missing animated transform evaluator data")?;
            let keyed = [
                !channels.translation.keys.is_empty(),
                match &channels.rotation {
                    super::nif_transform::Rotation::Absent => false,
                    super::nif_transform::Rotation::Quaternion(track) => !track.keys.is_empty(),
                    super::nif_transform::Rotation::Euler(_) => true,
                },
                !channels.scale.keys.is_empty(),
            ];
            for index in 0..3 {
                ensure!(
                    !animated[index] || keyed[index],
                    "Missing animated transform evaluator keys for channel {index}"
                );
            }
        }
        let pose = self.transform.sample_channels(time, active, animated)?;
        for (index, present) in [
            pose.translation.is_some(),
            pose.rotation.is_some(),
            pose.scale.is_some(),
        ]
        .into_iter()
        .enumerate()
        {
            ensure!(
                !active[index] || present,
                "Missing transform evaluator channel {index} value"
            );
        }
        Ok(pose)
    }
    pub fn load(nif: &NifFile, block: u32) -> Result<Self> {
        ensure!(
            nif.version == 0x14060500,
            "Unverified evaluator format version"
        );
        let NiObject::Unknown(raw) = nif
            .blocks
            .get(block as usize)
            .context("Missing transform evaluator")?
        else {
            anyhow::bail!("Expected raw transform evaluator")
        };
        let compact = raw.type_name == "NiBSplineCompTransformEvaluator";
        ensure!(matches!((raw.type_name.as_str(), raw.raw_data.len()),
            ("NiTransformEvaluator", 60) | ("NiBSplineCompTransformEvaluator", 108)),
            "Unexpected transform evaluator layout");
        let word = |at: usize| {
            let b = raw.raw_data[at..at + 4].try_into().unwrap();
            if nif.source_is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let string = |at: usize| -> Result<Option<String>> {
            let id = word(at);
            if id == u32::MAX {
                Ok(None)
            } else {
                Ok(Some(
                    nif.string_table
                        .get(id as usize)
                        .context("Missing evaluator binding string")?
                        .clone(),
                ))
            }
        };
        let binding = Binding {
            object: string(0)?,
            property: string(4)?,
            controller: string(8)?,
            controller_id: string(12)?,
            evaluator_id: string(16)?,
        };
        let channel_types: [u8; 4] = raw.raw_data[20..24].try_into()?;
        let value = |at| f32::from_bits(word(at));
        let pose_at = if compact { 40 } else { 24 };
        let translation = std::array::from_fn(|i| value(pose_at + 4 * i));
        let rotation = std::array::from_fn(|i| value(pose_at + 12 + 4 * i));
        let scale = value(pose_at + 28);
        ensure!(
            translation
                .iter()
                .chain(rotation.iter())
                .chain(std::iter::once(&scale))
                .all(|v| v.is_finite()),
            "Nonfinite transform evaluator value"
        );
        let splines = if compact {
            let mut tracks = [None, None, None];
            let mut animated = [4, 5, 3].into_iter().enumerate()
                .filter(|&(i, kind)| channel_types[i] == kind).peekable();
            if animated.peek().is_some() {
                let source = spline::Source::load(nif, [value(24), value(28)], word(32), word(36))?;
                for (i, _) in animated {
                    tracks[i] = Some(source.channel(word(72 + i * 4), [3, 4, 1][i], value(84 + i * 8), value(88 + i * 8))?);
                }
            }
            Some(tracks)
        } else { None };
        let link = word(56);
        let data = (!compact && link != u32::MAX).then_some(link);
        let channels = data
            .map(|id| super::nif_transform::Channels::load(nif, id))
            .transpose()?;
        Ok(Self {
            binding,
            channel_types,
            transform: super::nif_transform::Interpolator {
                translation,
                rotation,
                scale,
                data,
                channels,
            },
            splines,
        })
    }
}

impl FloatEvaluator {
    pub fn load(nif: &NifFile, block: u32) -> Result<Self> {
        ensure!(
            nif.version == 0x14060500,
            "Unverified evaluator format version"
        );
        let NiObject::Unknown(raw) = nif
            .blocks
            .get(block as usize)
            .context("Missing float evaluator")?
        else {
            anyhow::bail!("Expected raw float evaluator")
        };
        ensure!(
            matches!(
                raw.type_name.as_str(),
                "NiConstFloatEvaluator" | "NiFloatEvaluator" | "NiBSplineCompFloatEvaluator"
            ),
            "Unsupported float evaluator {}",
            raw.type_name
        );
        ensure!(
            raw.raw_data.len() == if raw.type_name == "NiBSplineCompFloatEvaluator" { 52 } else { 28 },
            "Unexpected float evaluator layout"
        );
        let word = |at: usize| {
            let b = raw.raw_data[at..at + 4].try_into().unwrap();
            if nif.source_is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let string = |at: usize| -> Result<Option<String>> {
            let id = word(at);
            if id == u32::MAX {
                Ok(None)
            } else {
                Ok(Some(
                    nif.string_table
                        .get(id as usize)
                        .context("Missing evaluator binding string")?
                        .clone(),
                ))
            }
        };
        let binding = Binding {
            object: string(0)?,
            property: string(4)?,
            controller: string(8)?,
            controller_id: string(12)?,
            evaluator_id: string(16)?,
        };
        let channel_types = raw.raw_data[20..24].try_into()?;
        let value = if raw.type_name == "NiConstFloatEvaluator" {
            let value = f32::from_bits(word(24));
            ensure!(value.is_finite(), "Nonfinite constant evaluator");
            FloatValue::Constant(value)
        } else if raw.type_name == "NiBSplineCompFloatEvaluator" {
            let value = |at| f32::from_bits(word(at));
            FloatValue::Spline(spline::Source::load(nif, [value(24), value(28)], word(32), word(36))?
                .channel(word(40), 1, value(44), value(48))?)
        } else {
            let link = word(24);
            FloatValue::Keys(if link == u32::MAX {
                None
            } else {
                Some(super::nif_transform::Track::load_float_data(nif, link)?)
            })
        };
        Ok(Self {
            binding,
            channel_types,
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use epic_mickey_lib_rs::nif_file::RawBlockData;
    #[test]
    fn palette_preserves_aliases_fallback_cache_and_stream_rules() -> Result<()> {
        use epic_mickey_lib_rs::nif_file::NiNodeData;
        for little in [false, true] {
            let mut root = NiNodeData::default();
            root.base.base.name = "root".into();
            root.children = vec![Some(2), Some(1)];
            let mut child = NiNodeData::default();
            child.base.base.name = "body".into();
            let mut nif = NifFile {
                version: 0x14060500,
                source_is_little_endian: little,
                roots: vec![Some(0)],
                blocks: vec![
                    NiObject::NiNode(root),
                    NiObject::NiNode(child.clone()),
                    NiObject::NiNode(child),
                ],
                ..Default::default()
            };
            let hierarchy = super::super::scene_bounds::Hierarchy::load(&nif)?;
            let mut palette = ObjectPalette::new(Some(0));
            assert_eq!(palette.get(&hierarchy, "body")?, Some(2));
            palette.scene = None;
            assert_eq!(palette.get(&hierarchy, "body")?, Some(2)); // Cached, independent of scene pointer.
            palette.set("body", Some(1));
            assert_eq!(palette.get(&hierarchy, "body")?, Some(1));
            palette.set("body", None);
            assert_eq!(palette.get(&hierarchy, "body")?, None);
            palette.scene = Some(0);
            assert_eq!(palette.get(&hierarchy, "body")?, Some(2));
            assert_eq!(palette.get(&hierarchy, "Body")?, None);
            let mut bytes = Vec::new();
            let word = |value: u32| {
                if little {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                }
            };
            bytes.extend(word(0));
            bytes.extend(word(5));
            for (name, target) in [
                (Some("body"), 1),
                (Some("body"), 2),
                (Some("body"), u32::MAX),
                (None, 1),
                (Some("alias\0ignored"), 1),
            ] {
                if let Some(name) = name {
                    bytes.extend(word(name.len() as u32));
                    bytes.extend(name.as_bytes());
                } else {
                    bytes.extend(word(u32::MAX));
                } // Native signed nonpositive length => null.
                bytes.extend(word(target));
            }
            nif.blocks.push(NiObject::Unknown(RawBlockData {
                type_name: "NiDefaultAVObjectPalette".into(),
                raw_data: bytes,
            }));
            let mut loaded = ObjectPalette::load(&nif, 3)?;
            loaded.scene = None;
            assert_eq!(loaded.get(&hierarchy, "body")?, Some(2)); // Duplicate replaced; null link skipped, not removed.
            assert_eq!(loaded.get(&hierarchy, "alias")?, Some(1));
            assert_eq!(loaded.get(&hierarchy, "root")?, None);
            if let NiObject::Unknown(raw) = &mut nif.blocks[3] {
                raw.raw_data.pop();
            }
            assert!(ObjectPalette::load(&nif, 3).is_err());
        }
        Ok(())
    }
    #[test]
    fn crossfade_source_retains_unwrapped_time_after_frequency_changes() -> Result<()> {
        let mut source = UnsynchronizedClock::new(0., 2.)?;
        assert_eq!(source.sample(100., 2., 0)?, 0.);
        assert_eq!(source.sample(101.5, 2., 0)?, 1.); // Weighted time=3, visible=1.
        source.frequency = 1.;
        assert_eq!(source.sample(102., 2., 0)?, 1.5); // Weighted time=3.5.
        source.prepare_crossfade_source()?;
        assert_eq!(source.sample(102., 2., 0)?, 1.5);
        assert_eq!(source.sample(102.5, 2., 0)?, 0.); // Retained history reaches4, not2.
        let mut clamped = UnsynchronizedClock::new(0., 1.)?;
        clamped.sample(200., 1., 2)?;
        assert_eq!(clamped.sample(203., 1., 2)?, 1.);
        clamped.prepare_crossfade_source()?;
        // Sampling with a longer duration exposes the unclamped history.
        assert_eq!(clamped.sample(203., 10., 2)?, 3.);
        let mut unsampled = UnsynchronizedClock::new(0.5, 1.)?;
        unsampled.prepare_crossfade_source()?;
        assert_eq!(unsampled.sample(100., 1., 2)?, 0.);
        Ok(())
    }
    #[test]
    fn unsynchronized_activation_starts_on_first_update() -> Result<()> {
        let mut clock = UnsynchronizedClock::new(0.5, 2.)?;
        assert_eq!(clock.sample(100., 10., 2)?, 0.5);
        assert_eq!(clock.sample(100.25, 10., 2)?, 1.);
        clock.frequency = 1.;
        assert_eq!(clock.sample(100.5, 10., 2)?, 1.25);
        let mut other = UnsynchronizedClock::new(0., 1.)?;
        assert_eq!(other.sample(1000., 10., 2)?, 0.);
        assert_eq!(other.sample(1000.5, 10., 2)?, 0.5);
        let mut rounded = UnsynchronizedClock::new(0.25, 1.)?;
        // The original adds the start offset to -global_time first. At this
        // magnitude that addition rounds away the quarter-frame offset.
        assert_eq!(rounded.sample(16_777_216., 1., 2)?, 0.);
        assert!(UnsynchronizedClock::new(0., 0.).is_err());
        Ok(())
    }
    #[test]
    fn sequence_time_preserves_end_frame_frequency_history_and_query_semantics() -> Result<()> {
        let mut clock = SequenceTime::default();
        assert_eq!(clock.compute(2., 1., 2., 0, true)?, 2.);
        assert_eq!(clock.compute(4., 1., 2., 0, true)?, 0.);
        assert_eq!(clock.compute(-0.5, 1., 2., 0, false)?, 1.5);
        assert_eq!(
            clock.compute(-4., 1., 2., 0, false)?.to_bits(),
            (-0.0f32).to_bits()
        );
        assert_eq!(clock.compute(5., 2., 10., 2, true)?, 6.);
        assert_eq!(clock.compute(6., 3., 10., 2, false)?, 9.);
        // A non-storing query must not change the frequency interval.
        assert_eq!(clock.compute(7., 1., 10., 2, true)?, 8.);
        assert_eq!(clock.compute(10., 1., 2., 1, false)?, 2.);
        assert_eq!(clock.compute(11., 1., 0., 0, false)?, 0.);

        let mut fused = SequenceTime::default();
        assert_eq!(fused.compute(-1., 1., 1., 2, true)?, 0.);
        let epsilon = f32::from_bits(0x3400_0000); // 2^-23.
                                                   // (1+epsilon)*(1-epsilon)-1 stays negative with fmadds. Separate
                                                   // multiplication/addition would round to zero and sample frame zero.
        assert_eq!(fused.compute(epsilon, 1. - epsilon, 1., 0, true)?, 1.);
        assert!(fused.compute(f32::NAN, 1., 1., 0, true).is_err());
        Ok(())
    }
    #[test]
    fn fade_envelope_anchors_on_update_and_preserves_ppc_arithmetic() -> Result<()> {
        let mut fade = FadeEnvelope::new(0.1)?;
        assert_eq!(fade.sample(0.1, FadeDirection::In)?, FadeSample::Weight(0.));
        let FadeSample::Weight(incoming) = fade.sample(0.15, FadeDirection::In)? else {
            panic!("early completion")
        };
        let FadeSample::Weight(outgoing) = fade.sample(0.15, FadeDirection::Out)? else {
            panic!("early completion")
        };
        assert_eq!(incoming.to_bits(), 0x3f00_0001);
        assert_eq!(outgoing.to_bits(), 0x3eff_ffff);
        assert_ne!(outgoing.to_bits(), (1. - incoming).to_bits());
        assert_eq!(
            fade.sample(0.2, FadeDirection::Out)?,
            FadeSample::Complete { end_time: 0.2 }
        );
        let mut delayed = FadeEnvelope::new(1.)?;
        assert_eq!(
            delayed.sample(50., FadeDirection::Out)?,
            FadeSample::Weight(1.)
        );
        assert_eq!(
            delayed.sample(50.25, FadeDirection::Out)?,
            FadeSample::Weight(0.75)
        );
        // The native expression does not clamp an update before its anchor.
        assert_eq!(
            delayed.sample(49.5, FadeDirection::In)?,
            FadeSample::Weight(-0.5)
        );
        assert_eq!(
            delayed.sample(60., FadeDirection::Out)?,
            FadeSample::Complete { end_time: 51. }
        );
        assert!(FadeEnvelope::new(0.).is_err());
        assert!(delayed.sample(f32::NAN, FadeDirection::In).is_err());
        Ok(())
    }
    #[test]
    fn transition_defaults_require_an_edge_and_preserve_policy_values() -> Result<()> {
        use epic_mickey_lib_rs::kfm_file::{KfmBlendPair, KfmFile, KfmSequence, KfmTransition};
        let mut manager = KfmFile {
            sync_type: 1,
            nonsync_type: 2,
            sync_duration: 0.125,
            nonsync_duration: 0.75,
            sequences: vec![KfmSequence {
                sequence_id: 10,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(resolve_transition(&manager, 10, 20)?.is_none());
        manager.transitions.push(KfmTransition {
            src_id: 10,
            des_id: 20,
            trans_type: 4,
            duration: 999.,
            ..Default::default()
        });
        let policy = resolve_transition(&manager, 10, 20)?.unwrap();
        assert_eq!((policy.kind, policy.duration), (1, 0.125));
        assert!(policy.blend_pairs.is_empty() && policy.chain.is_empty());
        manager.sync_duration = 0.5;
        assert_eq!(resolve_transition(&manager, 10, 20)?.unwrap().duration, 0.5);
        manager.transitions.push(KfmTransition {
            src_id: 10,
            des_id: 20,
            trans_type: 5,
            ..Default::default()
        });
        let policy = resolve_transition(&manager, 10, 20)?.unwrap();
        assert_eq!((policy.kind, policy.duration), (2, 0.75));
        assert!(resolve_transition(&manager, 99, 20)?.is_none());
        assert!(resolve_transition(&manager, 10, 99)?.is_none());
        manager.transitions.push(KfmTransition {
            src_id: 10,
            des_id: 20,
            trans_type: 0,
            duration: 0.25,
            blend_pairs: vec![KfmBlendPair {
                start_key: "end".into(),
                target_key: "begin".into(),
            }],
            ..Default::default()
        });
        let policy = resolve_transition(&manager, 10, 20)?.unwrap();
        assert_eq!((policy.kind, policy.duration), (0, 0.25));
        assert_eq!(policy.blend_pairs[0].target_key, "begin");
        manager.sequences.push(manager.sequences[0].clone());
        assert!(resolve_transition(&manager, 10, 20).is_err());
        Ok(())
    }
    #[test]
    fn component_clock_preserves_initial_periodic_and_owner_time_rules() {
        let mut clock = ComponentClock::default();
        assert!(clock.use_delta_time);
        assert_eq!(clock.periodic_update_time(500.0), 0.0);
        clock.initialize(true, 16_777_217.0);
        assert_eq!(clock.initial_update_time(), 16_777_216.0);
        clock.advance(true, 1.0);
        clock.advance(true, 1.0);
        // Per-step f32 rounding, not a double accumulator rounded at sampling.
        assert_eq!(clock.periodic_update_time(500.0), 16_777_216.0);

        clock.initialize(false, 900.0);
        clock.advance(true, 0.25);
        clock.advance(false, 10.0);
        assert_eq!(clock.periodic_update_time(500.0), 0.25);
        // Skipping a pose sample must not discard an otherwise eligible tick.
        clock.advance(true, 0.25);
        clock.advance(true, 0.25);
        assert_eq!(clock.periodic_update_time(500.0), 0.75);

        clock.use_delta_time = false;
        assert_eq!(clock.initial_update_time(), 0.0);
        assert_eq!(clock.periodic_update_time(500.0), 500.0);
        clock.advance(true, -0.25);
        clock.use_delta_time = true;
        assert_eq!(clock.periodic_update_time(500.0), 0.5);
    }
    #[test]
    fn kf_paths_use_wii_asset_namespace() -> Result<()> {
        assert_eq!(
            kf_asset_path("Objects/Sub/model.kfm_wii", r".\motion.kf_wii")?,
            "Objects/Sub/motion.kf_wii"
        );
        assert_eq!(
            kf_asset_path("Objects/Sub/model.kfm", r"..\motion.kf")?,
            "Objects/motion.kf"
        );
        assert_eq!(
            kf_asset_path("Objects/model.kfm", r"\Shared\motion.kf")?,
            "Shared/motion.kf"
        );
        assert_eq!(
            kf_asset_path("Objects/model.kfm", "C:/motion.kf")?,
            "Objects/C:/motion.kf"
        );
        assert!(kf_asset_path("model.kfm", "../motion.kf").is_err());
        assert!(kf_asset_path("model.kfm", "").is_err());
        Ok(())
    }
    #[test]
    fn kfm_load_order_matches_resource_map_not_file_or_numeric_order() {
        use epic_mickey_lib_rs::kfm_file::{KfmFile, KfmSequence};
        let mut manager = KfmFile::default();
        for (id, name) in [
            (1, "first"),
            (38, "collision"),
            (0, "zero"),
            (75, "newest"),
            (38, "replacement"),
        ] {
            manager.sequences.push(KfmSequence {
                sequence_id: id,
                sequence_name: name.into(),
                ..Default::default()
            });
        }
        let ordered = kfm_load_order(&manager);
        assert_eq!(
            ordered.iter().map(|e| e.sequence_id).collect::<Vec<_>>(),
            vec![0, 75, 38, 1]
        );
        assert_eq!(ordered[2].sequence_name, "replacement");
    }
    #[test]
    fn registry_preserves_original_bucket_and_chain_order() {
        let sequence = |name: &str| {
            std::sync::Arc::new(Sequence {
                name: Some(name.into()),
                evaluators: vec![],
                text_keys: None,
                duration: 1.,
                cycle_type: 2,
                frequency: 1.,
                accumulation_root: None,
                flags: 0,
            })
        };
        let mut registry = SequenceRegistry::new(std::num::NonZeroU32::new(3).unwrap());
        registry.set(1, sequence("Rest"));
        registry.set(4, sequence("Rest"));
        assert_eq!(registry.find_name("Rest"), 4); // newest colliding entry first
        registry.set(3, sequence("Rest"));
        assert_eq!(registry.find_name("Rest"), 3); // earlier bucket wins
        registry.remove(3).unwrap();
        registry.set(1, sequence("Rest")); // updating an existing key keeps its position
        assert_eq!(registry.find_name("Rest"), 4);
        registry.remove(1).unwrap();
        registry.set(1, sequence("Rest"));
        assert_eq!(registry.find_name("Rest"), 1); // unload then re-add moves to head
        assert_eq!(registry.find_name("rest"), SequenceRegistry::INVALID_ID);
        assert!(registry.get(4).is_some());
        assert!(registry.get(99).is_none());
        assert!(registry.remove(99).is_none());
    }
    #[test]
    fn stream_selection_uses_root_order_and_unfiltered_indices() -> Result<()> {
        let sequence = |duration: f32| {
            NiObject::Unknown(RawBlockData {
                type_name: "NiSequenceData".into(),
                raw_data: [
                    0,
                    0,
                    u32::MAX,
                    duration.to_bits(),
                    2,
                    1f32.to_bits(),
                    u32::MAX,
                    0,
                ]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect(),
            })
        };
        let mut nif = NifFile {
            version: 0x14060500,
            source_is_little_endian: false,
            string_table: vec!["Rest".into()],
            blocks: vec![
                sequence(1.),
                sequence(2.),
                NiObject::NiMesh(Default::default()),
            ],
            roots: vec![None, Some(2), Some(1), Some(0)],
            ..Default::default()
        };
        assert_eq!(
            Sequence::from_stream(&nif, Some("Rest"), 0)?
                .unwrap()
                .duration,
            2.
        );
        assert!(Sequence::from_stream(&nif, Some("rest"), 0)?.is_none());
        for i in [-1, 0, 1, 4] {
            assert!(Sequence::from_stream(&nif, None, i)?.is_none());
        }
        assert_eq!(Sequence::from_stream(&nif, None, 3)?.unwrap().duration, 1.);
        nif.roots = vec![Some(2)];
        assert!(Sequence::from_stream(&nif, Some("Rest"), 0)?.is_none());
        nif.roots = vec![Some(99)];
        assert!(Sequence::from_stream(&nif, Some("Rest"), 0).is_err());
        Ok(())
    }
    #[test]
    fn render_morph_preserves_seams_attributes_and_region_offsets() -> Result<()> {
        let template = (0..36).map(|v| v as f32).collect::<Vec<_>>();
        let transform = [
            [1., 0., 0., 10.],
            [0., 2., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ];
        let (out, lo, hi) = render_positions(
            &template,
            &[8, 7, 8],
            7,
            &[[1., 2., 3.], [4., 5., 6.]],
            transform,
        )?;
        assert_eq!(&out[..3], &[14., 10., 6.]);
        assert_eq!(&out[24..27], &out[..3]);
        assert_eq!((lo, hi), ([11., 4., 3.], [14., 10., 6.]));
        for (a, b) in out.chunks_exact(12).zip(template.chunks_exact(12)) {
            assert_eq!(&a[3..], &b[3..]);
        }
        assert!(render_positions(&template, &[6, 7, 8], 7, &[[0.; 3]; 2], transform).is_err());
        assert!(render_positions(&template, &[7, 8, 9], 7, &[[0.; 3]; 2], transform).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "audits all original packfiles in BLOTWORX_TEST_DATA"]
    fn production_kfm_corpus_preserves_authored_data() -> Result<()> {
        use epic_mickey_lib_rs::kfm_file::KfmFile;
        use std::collections::{BTreeMap, HashSet};
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let mut archives = std::fs::read_dir(root.join("packfiles"))?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        archives.retain(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
        });
        archives.sort();
        ensure!(!archives.is_empty(), "No packfiles found");
        let mut seen = HashSet::new();
        let mut cases = Vec::new();
        let mut errors = Vec::new();
        let mut occurrences = 0usize;
        let mut versions = BTreeMap::<String, usize>::new();
        for path in &archives {
            let pak = crate::pack_io::read(std::fs::read(path)?)
                .with_context(|| format!("Archive {}", path.display()))?
                .pak;
            for file in &pak.files {
                let name = file.path.to_ascii_lowercase();
                if !name.ends_with(".kfm") && !name.ends_with(".kfm_wii") {
                    continue;
                }
                occurrences += 1;
                let hash = ring::digest::digest(&ring::digest::SHA256, &file.data);
                if !seen.insert(hash.as_ref().to_vec()) {
                    continue;
                }
                let identity = serde_json::json!({"archive":path.file_name().unwrap().to_string_lossy(),"asset":file.path,
                    "sha256":hash.as_ref().iter().map(|b|format!("{b:02x}")).collect::<String>()});
                let checked = (|| -> Result<serde_json::Value> {
                    let manager = KfmFile::load_bytes(&file.data).map_err(anyhow::Error::msg)?;
                    *versions.entry(manager.version.clone()).or_default() += 1;
                    let encoded = manager.to_bytes().map_err(anyhow::Error::msg)?;
                    let reloaded = KfmFile::load_bytes(&encoded).map_err(anyhow::Error::msg)?;
                    ensure!(
                        serde_json::to_value(&manager)? == serde_json::to_value(&reloaded)?,
                        "KFM semantic roundtrip differs"
                    );
                    ensure!(
                        encoded == file.data,
                        "KFM byte roundtrip differs (original {}, rewritten {})",
                        file.data.len(),
                        encoded.len()
                    );
                    let mut resolved_kinds = BTreeMap::<u32, usize>::new();
                    for edge in &manager.transitions {
                        let policy = resolve_transition(&manager, edge.src_id, edge.des_id)?
                            .context("Missing authored transition")?;
                        *resolved_kinds.entry(policy.kind).or_default() += 1;
                    }
                    Ok(
                        serde_json::json!({"identity":identity,"version":manager.version,
                        "sync_policy":{"kind":manager.sync_type,"duration":manager.sync_duration},
                        "nonsync_policy":{"kind":manager.nonsync_type,"duration":manager.nonsync_duration},
                        "resolved_kinds":resolved_kinds,
                        "sequences":manager.sequences.len(),"transitions":manager.transitions.len(),
                        "explicit_transitions":manager.transitions.iter().filter(|t|t.trans_type!=4 && t.trans_type!=5).count(),
                        "blend_pairs":manager.transitions.iter().map(|t|t.blend_pairs.len()).sum::<usize>(),
                        "chain_entries":manager.transitions.iter().map(|t|t.chain.len()).sum::<usize>(),
                        "groups":manager.groups.len(),"group_members":manager.groups.iter().map(|g|g.sequences.len()).sum::<usize>()}),
                    )
                })();
                match checked {
                    Ok(case) => cases.push(case),
                    Err(error) => errors.push(
                        serde_json::json!({"identity":identity,"error":format!("{error:#}")}),
                    ),
                }
            }
        }
        let report = serde_json::json!({"archives":archives.len(),"occurrences":occurrences,"unique_payloads":seen.len(),
            "versions":versions,"passed":cases.len(),"cases":cases,"errors":errors,"runtime_verified":false});
        std::fs::create_dir_all("target")?;
        std::fs::write(
            "target/kfm-corpus-audit.json",
            serde_json::to_vec_pretty(&report)?,
        )?;
        ensure!(occurrences > 0, "No KFM assets found");
        ensure!(
            errors.is_empty(),
            "{} KFM failures; see target/kfm-corpus-audit.json",
            errors.len()
        );
        eprintln!(
            "KFM corpus: {} archives, {occurrences} occurrences, {} unique payloads",
            archives.len(),
            seen.len()
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_spawner_sequence_deforms_authored_vertices() -> Result<()> {
        let root = std::path::PathBuf::from(
            std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?,
        );
        let pak = crate::pack_io::read(std::fs::read(
            root.join("packfiles/gremlinvillage_zonea.pak"),
        )?)?
        .pak;
        let load = |suffix: &str| -> Result<NifFile> {
            let file = pak
                .files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().ends_with(suffix))
                .context("Missing spawner asset")?;
            NifFile::load_bytes(&file.data).map_err(anyhow::Error::msg)
        };
        let bytes = |suffix: &str| -> Result<&[u8]> {
            Ok(&pak
                .files
                .iter()
                .find(|f| f.path.to_ascii_lowercase().ends_with(suffix))
                .context("Missing preview asset")?
                .data)
        };
        let model_bytes = bytes("pipe_straight_2m_spawner_01.nif_wii")?;
        let keyframe_bytes = bytes("pipe_straight_2m_spawner_01.kf_wii")?;
        let preview = preview_positions(model_bytes, keyframe_bytes, "Rest", 0.)?;
        assert_eq!(preview.len(), 2);
        assert!(preview_positions(model_bytes, keyframe_bytes, "missing", 0.).is_err());
        assert!(preview_positions(model_bytes, keyframe_bytes, "Spawn", f32::NAN).is_err());
        assert!(preview_positions(model_bytes, keyframe_bytes, "Spawn", 10.).is_err());
        let model = load("pipe_straight_2m_spawner_01.nif_wii")?;
        let kf = load("pipe_straight_2m_spawner_01.kf_wii")?;
        let sequences = kf
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.type_name() == "NiSequenceData")
            .map(|(i, _)| Sequence::load(&kf, i as u32))
            .collect::<Result<Vec<_>>>()?;
        let manager_bytes = bytes("pipe_straight_2m_spawner_01.kfm_wii")?;
        let manager = epic_mickey_lib_rs::kfm_file::KfmFile::load_bytes(manager_bytes)
            .map_err(anyhow::Error::msg)?;
        let mut resolves = 0;
        let prepared = SequenceResources::load(&manager, |filename| {
            assert!(filename
                .to_ascii_lowercase()
                .ends_with("pipe_straight_2m_spawner_01.kf_wii"));
            resolves += 1;
            Ok(std::sync::Arc::new(
                NifFile::load_bytes(keyframe_bytes).map_err(anyhow::Error::msg)?,
            ))
        })?;
        assert_eq!(resolves, 1, "Shared KF must be decoded once");
        assert_eq!(prepared.find_name("Rest"), 0);
        assert_eq!(prepared.find_name("Spawn"), 1);
        assert_eq!(
            serde_json::to_value(prepared.manager())?,
            serde_json::to_value(&manager)?
        );
        assert_eq!(prepared.manager().transitions.len(), 2);
        let loaded = SequenceResources::load_from_assets(
            "GameObjects/Spawners/Pipe_Straight_2m_Spawner_01.kfm",
            &pak.files,
        )?;
        assert_eq!(loaded.find_name("Rest"), 0);
        assert_eq!(loaded.find_name("Spawn"), 1);
        assert!(std::ptr::eq(
            loaded.get(0).unwrap().1,
            loaded.get(1).unwrap().1
        ));
        assert!(std::ptr::eq(
            prepared.get(0).unwrap().1,
            prepared.get(1).unwrap().1
        ));
        assert!(prepared.get(99).is_none());
        let mut missing = manager.clone();
        missing.sequences[0].sequence_name = "Missing".into();
        assert!(
            SequenceResources::load(&missing, |_| Ok(std::sync::Arc::new(
                NifFile::load_bytes(keyframe_bytes).unwrap()
            )))
            .is_err()
        );
        assert!(SequenceResources::load(&manager, |_| anyhow::bail!("missing file")).is_err());
        let mut registry = SequenceRegistry::default();
        assert_eq!(manager.sequences.len(), 2);
        assert_ne!(
            manager.sequences[0].sequence_id % 37,
            manager.sequences[1].sequence_id % 37
        );
        for entry in kfm_load_order(&manager) {
            let name = (!entry.sequence_name.is_empty()).then_some(entry.sequence_name.as_str());
            let bound = Sequence::from_stream(&kf, name, entry.anim_index)?
                .context("KFM sequence is missing")?;
            registry.set(entry.sequence_id, std::sync::Arc::new(bound));
        }
        assert_eq!(registry.find_name("Rest"), 0);
        assert_eq!(registry.find_name("Spawn"), 1);
        assert_eq!(registry.get(1).unwrap().duration, 1.233333349f32);
        let rest = sequences
            .iter()
            .find(|s| s.name.as_deref() == Some("Rest"))
            .context("Missing Rest")?;
        let spawn = sequences
            .iter()
            .find(|s| s.name.as_deref() == Some("Spawn"))
            .context("Missing Spawn")?;
        let hierarchy = super::super::scene_bounds::Hierarchy::load(&model)?;
        let palettes = model
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| block.type_name() == "NiDefaultAVObjectPalette")
            .map(|(id, _)| id as u32)
            .collect::<Vec<_>>();
        ensure!(
            palettes.is_empty(),
            "Original spawner fixture unexpectedly contains a serialized palette"
        );
        let root = hierarchy
            .roots
            .iter()
            .flatten()
            .copied()
            .next()
            .context("Missing spawner root")?;
        let mut palette = ObjectPalette::new(Some(root));
        let actor = ActorBindings::prepare(&prepared, &model, &hierarchy, root)?;
        ensure!(
            actor.other_evaluators.is_empty(),
            "Spawner has unhandled pose channels"
        );
        ensure!(
            actor.morph_sequences[&0].len() == 2 && actor.morph_sequences[&1].len() == 2,
            "Spawner morph channels missing"
        );
        for (key, idle) in &actor.morph_sequences[&0] {
            ensure!(
                std::sync::Arc::ptr_eq(&idle.targets, &actor.morph_sequences[&1][key].targets),
                "Sequence bindings duplicated morph geometry"
            );
        }
        let combined = actor.sample(1, 0.25)?;
        ensure!(
            combined.transforms.len() == 3 && combined.morphs.len() == 2,
            "Incomplete actor pose snapshot"
        );
        for (&(mesh, submesh), (_, positions)) in &combined.morphs {
            ensure!(submesh == 0, "Unexpected spawner submesh");
            let controller = model
                .blocks
                .iter()
                .enumerate()
                .find_map(|(id, block)| {
                    (block.type_name() == "NiMorphWeightsController")
                        .then(|| MorphController::load(&model, id as u32).map(|c| (id, c)))
                        .transpose()
                        .ok()
                        .flatten()
                        .filter(|(_, c)| c.target == mesh)
                        .map(|(id, _)| id as u32)
                })
                .context("Missing spawner controller")?;
            ensure!(
                *positions
                    == BoundMorph::bind(&model, controller, &kf, spawn, submesh)?.sample(0.25)?,
                "Combined pose differs from original morph binding"
            );
        }
        for sequence in [rest, spawn] {
            let bound = BoundTransforms::bind(&kf, sequence, |name| palette.get(&hierarchy, name))?;
            for time in [0., sequence.duration * 0.5, sequence.duration] {
                let poses = bound.sample(time)?;
                ensure!(
                    poses.len() == 3,
                    "Expected all three spawner transform targets"
                );
                let world = hierarchy.base_world_transforms_with_poses(None, &poses)?;
                ensure!(
                    world.len() == hierarchy.nodes.len(),
                    "Animated spawner lost scene nodes"
                );
                for (id, pose) in &poses {
                    let local = pose.apply_to(&hierarchy.nodes[id].local)?;
                    ensure!(local.scale.is_finite(), "Nonfinite spawner scale");
                }
            }
            let mut transforms = 0;
            for &id in &sequence.evaluators {
                if kf.blocks[id as usize].type_name() != "NiTransformEvaluator" {
                    continue;
                }
                let evaluator = TransformEvaluator::load(&kf, id)?;
                for time in [0., sequence.duration * 0.5, sequence.duration] {
                    let pose = evaluator.sample(time)?;
                    let owned = [
                        pose.translation.is_some(),
                        pose.rotation.is_some(),
                        pose.scale.is_some(),
                    ];
                    for (index, present) in owned.into_iter().enumerate() {
                        ensure!(
                            present == (evaluator.channel_types[index] != 0),
                            "Spawner transform channel ownership changed"
                        );
                    }
                }
                transforms += 1;
            }
            ensure!(
                transforms == 3,
                "Expected three authored spawner transform evaluators"
            );
        }
        let mut reports = Vec::new();
        for (id, block) in model
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.type_name() == "NiMorphWeightsController")
        {
            let _ = block;
            let controller = MorphController::load(&model, id as u32)?;
            let base = position_stream(&model, controller.target, "POSITION", 0, 0)?;
            assert_eq!(preview[&controller.target].0, 0);
            assert_eq!(preview[&controller.target].1, base);
            let Some(NiObject::NiMesh(mesh)) = model.blocks.get(controller.target as usize) else {
                unreachable!()
            };
            let decoded = crate::wii_mesh::extract(&model, mesh)?;
            let template = vec![0.; decoded.source_indices.len() * 12];
            let (prepared_rest, prepared_kf) = prepared.get(0).context("Prepared Rest")?;
            let idle = BoundMorph::bind(&model, id as u32, prepared_kf, prepared_rest, 0)?;
            ensure!(
                idle.sample(0.)? == base && idle.sample(rest.duration)? == base,
                "Rest changed authored base vertices"
            );
            let active = BoundMorph::bind(&model, id as u32, &kf, spawn, 0)?;
            let mut playback = UnsynchronizedClock::new(0., spawn.frequency)?;
            let first = playback.sample(100., spawn.duration, spawn.cycle_type)?;
            assert_eq!(active.sample(first)?, active.sample(0.)?);
            let next = playback.sample(100.25, spawn.duration, spawn.cycle_type)?;
            assert_eq!(active.sample(next)?, active.sample(0.25 * spawn.frequency)?);
            ensure!(
                active.sample(next)?.iter().zip(&base).any(|(a, b)| a != b),
                "Activated spawner did not deform"
            );
            playback.prepare_crossfade_source()?;
            let resumed = playback.sample(100.25, spawn.duration, spawn.cycle_type)?;
            assert_eq!(active.sample(resumed)?, active.sample(next)?);
            let continued = playback.sample(100.5, spawn.duration, spawn.cycle_type)?;
            assert_eq!(
                active.sample(continued)?,
                active.sample(0.5 * spawn.frequency)?
            );
            let mut samples = Vec::new();
            for fraction in [0., 0.25, 0.5, 0.75, 1.] {
                let positions = active.sample(spawn.duration * fraction)?;
                let (rendered, _, _) = render_positions(
                    &template,
                    &decoded.source_indices,
                    0,
                    &positions,
                    [
                        [1., 0., 0., 0.],
                        [0., 1., 0., 0.],
                        [0., 0., 1., 0.],
                        [0., 0., 0., 1.],
                    ],
                )?;
                for (vertex, &source) in rendered.chunks_exact(12).zip(&decoded.source_indices) {
                    ensure!(
                        vertex[..3] == positions[source as usize],
                        "Cooked vertex lost morph mapping"
                    );
                }
                let changed = positions.iter().zip(&base).filter(|(a, b)| a != b).count();
                let center = [
                    mesh.bound_center.x,
                    mesh.bound_center.y,
                    mesh.bound_center.z,
                ];
                let max_distance = positions
                    .iter()
                    .map(|p| {
                        (0..3)
                            .map(|i| {
                                let d = f64::from(p[i]) - f64::from(center[i]);
                                d * d
                            })
                            .sum::<f64>()
                            .sqrt()
                    })
                    .fold(0f64, f64::max);
                samples.push(
                    serde_json::json!({"time":spawn.duration*fraction,"changed_vertices":changed,
                    "max_distance_from_authored_center":max_distance,
                    "outside_authored_sphere":max_distance>f64::from(mesh.bound_radius)}),
                );
            }
            ensure!(
                samples
                    .iter()
                    .any(|s| s["changed_vertices"].as_u64().unwrap() > 0),
                "Spawn did not deform any vertex"
            );
            ensure!(
                active.sample(spawn.duration)? == base,
                "Spawn endpoint did not restore base vertices"
            );
            reports.push(serde_json::json!({"mesh":controller.target,"vertices":base.len(),"samples":samples,
                "authored_center":[mesh.bound_center.x,mesh.bound_center.y,mesh.bound_center.z],"authored_radius":mesh.bound_radius,
                "bound_runtime_policy_verified":false}));
        }
        ensure!(reports.len() == 2, "Expected both spawner meshes");
        std::fs::write(
            "target/spawner-morph-samples.json",
            serde_json::to_vec_pretty(&reports)?,
        )?;
        println!(
            "{} meshes: Rest preserves base, Spawn deforms and restores it",
            reports.len()
        );
        Ok(())
    }
    #[test]
    fn morph_kernel_keeps_signed_threshold_and_fused_accumulation() -> Result<()> {
        let epsilon = f32::from_bits(0x3a83126f);
        let below = f32::from_bits(epsilon.to_bits() - 1);
        let target = vec![[1000., -1000., 2.]];
        assert_eq!(morph_positions(&[target.clone()], &[below])?, vec![[0.; 3]]);
        assert_eq!(
            morph_positions(&[target.clone()], &[-below])?,
            vec![[0.; 3]]
        );
        for weight in [epsilon, -epsilon, 1.25, -0.1] {
            assert_eq!(
                morph_positions(&[target.clone()], &[weight])?[0],
                target[0].map(|v| weight.mul_add(v, 0.))
            );
        }
        let a = f32::from_bits(0x3f800001);
        let b = f32::from_bits(0x3f7fffff);
        let result = morph_positions(&[vec![[-1.; 3]], vec![[b; 3]]], &[1., a])?;
        assert_eq!(result[0], [a.mul_add(b, -1.); 3]);
        assert_ne!(result[0][0], a * b - 1.);
        assert!(morph_positions(&[target.clone()], &[f32::NAN]).is_err());
        assert!(morph_positions(&[target, vec![]], &[1., 1.]).is_err());
        Ok(())
    }
    #[test]
    fn morph_modifier_preserves_sync_points_and_element_policy() -> Result<()> {
        for little in [false, true] {
            let mut bytes = Vec::new();
            for (value, width) in [
                (2u32, 4),
                (0x8030, 2),
                (0x8040, 2),
                (0, 4),
                (3, 1),
                (4, 2),
                (1, 4),
                (0, 4),
                (2, 4),
                (1, 4),
            ] {
                let word = if little {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                bytes.extend_from_slice(if little {
                    &word[..width]
                } else {
                    &word[4 - width..]
                });
            }
            let mut nif = NifFile {
                version: 0x14060500,
                source_is_little_endian: little,
                string_table: vec!["NORMAL".into()],
                blocks: vec![NiObject::Unknown(RawBlockData {
                    type_name: "NiMorphMeshModifier".into(),
                    raw_data: bytes,
                })],
                ..Default::default()
            };
            let modifier = MorphModifier::load(&nif, 0)?;
            assert_eq!(modifier.submit_points, vec![0x8030, 0x8040]);
            assert!(modifier.complete_points.is_empty());
            assert_eq!((modifier.flags, modifier.target_count), (3, 4));
            assert_eq!(modifier.elements[0].semantic, "NORMAL");
            assert_eq!(
                (modifier.elements[0].index, modifier.elements[0].normalize),
                (2, 1)
            );
            if let NiObject::Unknown(raw) = &mut nif.blocks[0] {
                raw.raw_data.push(0);
            }
            assert!(MorphModifier::load(&nif, 0).is_err());
        }
        Ok(())
    }
    #[test]
    fn morph_targets_follow_names_not_evaluator_order() -> Result<()> {
        for little in [false, true] {
            let words = |v: &[u32]| {
                v.iter()
                    .flat_map(|v| {
                        if little {
                            v.to_le_bytes()
                        } else {
                            v.to_be_bytes()
                        }
                    })
                    .collect::<Vec<_>>()
            };
            let mut bytes = vec![0; 22];
            bytes.extend(words(&[1, 2, 2, u32::MAX, u32::MAX, 2, 1, 0]));
            let mut nif = NifFile {
                version: 0x14060500,
                source_is_little_endian: little,
                string_table: vec!["Smile".into(), "Rest".into()],
                blocks: vec![
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiMorphWeightsController".into(),
                        raw_data: bytes,
                    }),
                    NiObject::NiMesh(Default::default()),
                ],
                ..Default::default()
            };
            let mut controller = MorphController::load(&nif, 0)?;
            assert_eq!(controller.interpolators, vec![None, None]);
            assert_eq!(controller.target_index("Smile")?, 1);
            assert_eq!(controller.target_index("Rest")?, 0);
            assert!(controller.target_index("smile").is_err());
            controller.target_names[0] = Some("Smile".into());
            assert!(controller.target_index("Smile").is_err());
            if let NiObject::Unknown(raw) = &mut nif.blocks[0] {
                raw.raw_data.pop();
            }
            assert!(MorphController::load(&nif, 0).is_err());
        }
        Ok(())
    }
    #[test]
    fn transform_evaluator_retains_partial_pose() -> Result<()> {
        for little in [false, true] {
            let words = |v: &[u32]| {
                v.iter()
                    .flat_map(|v| {
                        if little {
                            v.to_le_bytes()
                        } else {
                            v.to_be_bytes()
                        }
                    })
                    .collect::<Vec<_>>()
            };
            let mut bytes = words(&[0, u32::MAX, 1, u32::MAX, u32::MAX]);
            bytes.extend([0x44, 0, 0, 2]);
            bytes.extend(words(&[
                2f32.to_bits(),
                3f32.to_bits(),
                4f32.to_bits(),
                (-f32::MAX).to_bits(),
                (-f32::MAX).to_bits(),
                (-f32::MAX).to_bits(),
                (-f32::MAX).to_bits(),
                (-f32::MAX).to_bits(),
                u32::MAX,
            ]));
            let mut nif = NifFile {
                version: 0x14060500,
                source_is_little_endian: little,
                string_table: vec!["Node".into(), "NiTransformController".into()],
                blocks: vec![NiObject::Unknown(RawBlockData {
                    type_name: "NiTransformEvaluator".into(),
                    raw_data: bytes,
                })],
                ..Default::default()
            };
            let mut evaluator = TransformEvaluator::load(&nif, 0)?;
            assert_eq!(evaluator.channel_types, [0x44, 0, 0, 2]);
            let mut sequence = Sequence {
                name: Some("test".into()),
                evaluators: vec![0],
                text_keys: None,
                duration: 1.,
                cycle_type: 2,
                frequency: 1.,
                accumulation_root: None,
                flags: 0,
            };
            let mut lookups = 0;
            let bound = BoundTransforms::bind(&nif, &sequence, |name| {
                assert_eq!(name, "Node");
                lookups += 1;
                Ok(Some(7))
            })?;
            assert_eq!(lookups, 1);
            assert_eq!(bound.sample(0.)?[&7].translation, Some([2., 3., 4.]));
            assert_eq!(bound.sample(1.)?[&7].translation, Some([2., 3., 4.]));
            assert!(bound.sample(2.).is_err());
            assert!(BoundTransforms::bind(&nif, &sequence, |_| Ok(None)).is_err());
            sequence.evaluators.push(0);
            assert!(BoundTransforms::bind(&nif, &sequence, |_| Ok(Some(7))).is_err());
            let pose = evaluator.sample(0.)?;
            assert_eq!(pose.translation, Some([2., 3., 4.]));
            assert!(pose.rotation.is_none() && pose.scale.is_none());
            // A cached value is not ownership of an inactive channel.
            evaluator.transform.scale = 7.;
            assert!(evaluator.sample(0.)?.scale.is_none());
            evaluator.channel_types[2] = 0x43;
            assert_eq!(evaluator.sample(0.)?.scale, Some(7.));
            evaluator.channel_types[2] = 3;
            assert!(evaluator.sample(0.).is_err()); // No authored scale keys.
            evaluator.channel_types[2] = 0;
            use super::super::nif_transform::{Channels, Key, Rotation, Track};
            evaluator.transform.channels = Some(Channels {
                translation: Track {
                    kind: 1,
                    keys: vec![Key {
                        time: 0.,
                        value: vec![9., 8., 7.],
                        tangents: None,
                        tcb: None,
                    }],
                },
                rotation: Rotation::Quaternion(Track {
                    kind: 99,
                    keys: vec![],
                }),
                scale: Track {
                    kind: 99,
                    keys: vec![],
                },
            });
            // Posed translation ignores keys; inactive unsupported rotation
            // and scale must not prevent evaluation of the owned channel.
            assert_eq!(evaluator.sample(0.)?.translation, Some([2., 3., 4.]));
            evaluator.channel_types[0] = 4;
            assert_eq!(evaluator.sample(0.)?.translation, Some([9., 8., 7.]));
            evaluator.channel_types[0] = 3;
            assert!(evaluator.sample(0.).is_err());
            if let NiObject::Unknown(raw) = &mut nif.blocks[0] {
                raw.raw_data.pop();
            }
            assert!(TransformEvaluator::load(&nif, 0).is_err());
        }
        Ok(())
    }
    #[test]
    fn float_evaluator_preserves_binding_and_key_data() -> Result<()> {
        for little in [false, true] {
            let words = |values: &[u32]| {
                values
                    .iter()
                    .flat_map(|v| {
                        if little {
                            v.to_le_bytes()
                        } else {
                            v.to_be_bytes()
                        }
                    })
                    .collect::<Vec<_>>()
            };
            let mut bytes = words(&[0, u32::MAX, 1, u32::MAX, 2]);
            bytes.extend([3, 0, 0, 0]);
            bytes.extend(words(&[1]));
            let mut nif = NifFile {
                version: 0x14060500,
                source_is_little_endian: little,
                string_table: vec![
                    "Mesh:0".into(),
                    "NiMorphWeightsController".into(),
                    "Target".into(),
                ],
                blocks: vec![
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiFloatEvaluator".into(),
                        raw_data: bytes,
                    }),
                    NiObject::Unknown(RawBlockData {
                        type_name: "NiFloatData".into(),
                        raw_data: words(&[
                            2,
                            1,
                            0f32.to_bits(),
                            0f32.to_bits(),
                            1f32.to_bits(),
                            1f32.to_bits(),
                        ]),
                    }),
                ],
                ..Default::default()
            };
            let evaluator = FloatEvaluator::load(&nif, 0)?;
            assert_eq!(evaluator.binding.object.as_deref(), Some("Mesh:0"));
            assert_eq!(evaluator.binding.evaluator_id.as_deref(), Some("Target"));
            assert!(evaluator.binding.property.is_none());
            assert_eq!(evaluator.channel_types, [3, 0, 0, 0]);
            let FloatValue::Keys(Some(track)) = evaluator.value else {
                panic!("Missing keys")
            };
            assert_eq!(track.sample_scalar(0.25)?, Some(0.25));
            if let NiObject::Unknown(raw) = &mut nif.blocks[1] {
                raw.raw_data.pop();
            }
            assert!(FloatEvaluator::load(&nif, 0).is_err());
            if let NiObject::Unknown(raw) = &mut nif.blocks[0] {
                raw.type_name = "NiConstFloatEvaluator".into();
                raw.raw_data[24..28].copy_from_slice(&words(&[0.75f32.to_bits()]));
            }
            assert!(matches!(
                FloatEvaluator::load(&nif, 0)?.value,
                FloatValue::Constant(0.75)
            ));
        }
        Ok(())
    }
    #[test]
    fn sequence_links_timing_and_malformed_blocks() -> Result<()> {
        for little in [false, true] {
            let raw = |name: &str, words: &[u32]| {
                NiObject::Unknown(RawBlockData {
                    type_name: name.into(),
                    raw_data: words
                        .iter()
                        .flat_map(|v| {
                            if little {
                                v.to_le_bytes()
                            } else {
                                v.to_be_bytes()
                            }
                        })
                        .collect(),
                })
            };
            let mut nif = NifFile {
                version: 0x14060500,
                source_is_little_endian: little,
                string_table: vec!["Spawn".into(), "Root".into()],
                blocks: vec![
                    raw(
                        "NiSequenceData",
                        &[
                            0,
                            2,
                            2,
                            1,
                            3,
                            1.25f32.to_bits(),
                            2,
                            0.5f32.to_bits(),
                            1,
                            0x40,
                        ],
                    ),
                    raw("NiFloatEvaluator", &[]),
                    raw("NiTransformEvaluator", &[]),
                    raw("NiTextKeyExtraData", &[]),
                ],
                ..Default::default()
            };
            let sequence = Sequence::load(&nif, 0)?;
            assert_eq!(sequence.evaluators, vec![2, 1]);
            assert_eq!(sequence.name.as_deref(), Some("Spawn"));
            assert_eq!(sequence.accumulation_root.as_deref(), Some("Root"));
            assert_eq!(
                (
                    sequence.duration,
                    sequence.frequency,
                    sequence.cycle_type,
                    sequence.flags
                ),
                (1.25, 0.5, 2, 0x40)
            );
            assert_eq!(sequence.text_keys, Some(3));
            let valid = nif.blocks[0].clone();
            if let NiObject::Unknown(b) = &mut nif.blocks[0] {
                b.raw_data.pop();
            }
            assert!(Sequence::load(&nif, 0).is_err());
            nif.blocks[0] = valid;
            nif.blocks[2] = raw("NiNode", &[]);
            assert!(Sequence::load(&nif, 0).is_err());
        }
        Ok(())
    }
}

impl Sequence {
    /// Original stream selection: DOL804164f8 (name), 80416620 (index).
    /// Name lookup takes the first matching top-level sequence, never a nested
    /// block. Legacy indices count every top-level slot, including nulls.
    pub fn from_stream(nif: &NifFile, name: Option<&str>, index: i32) -> Result<Option<Self>> {
        let decode = |root: Option<u32>| -> Result<Option<Self>> {
            let Some(id) = root else { return Ok(None) };
            let block = nif
                .blocks
                .get(id as usize)
                .context("Invalid top-level sequence link")?;
            if block.type_name() != "NiSequenceData" {
                return Ok(None);
            }
            Ok(Some(Self::load(nif, id)?))
        };
        if let Some(name) = name {
            for &root in &nif.roots {
                if let Some(sequence) = decode(root)? {
                    if sequence.name.as_deref() == Some(name) {
                        return Ok(Some(sequence));
                    }
                }
            }
            Ok(None)
        } else {
            // The original compares the signed legacy value as unsigned.
            match nif.roots.get(index as u32 as usize) {
                Some(&root) => decode(root),
                None => Ok(None),
            }
        }
    }

    pub fn load(nif: &NifFile, block: u32) -> Result<Self> {
        ensure!(
            nif.version == 0x14060500,
            "Unverified sequence format version {:08x}",
            nif.version
        );
        let NiObject::Unknown(raw) = nif
            .blocks
            .get(block as usize)
            .context("Missing sequence block")?
        else {
            anyhow::bail!("Expected raw NiSequenceData");
        };
        ensure!(
            raw.type_name == "NiSequenceData",
            "Expected NiSequenceData, got {}",
            raw.type_name
        );
        let bytes = &raw.raw_data;
        let mut at = 0usize;
        let mut word = || -> Result<u32> {
            let b = bytes
                .get(at..at + 4)
                .context("Truncated NiSequenceData")?
                .try_into()?;
            at += 4;
            Ok(if nif.source_is_little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            })
        };
        let string = |index: u32| -> Result<Option<String>> {
            if index == u32::MAX {
                return Ok(None);
            }
            Ok(Some(
                nif.string_table
                    .get(index as usize)
                    .context("Missing sequence string")?
                    .clone(),
            ))
        };
        let name = string(word()?)?;
        let count = word()? as usize;
        ensure!(
            count <= bytes.len().saturating_sub(32) / 4,
            "Sequence evaluator count exceeds block size"
        );
        ensure!(
            bytes.len() == 32 + count * 4,
            "Unexpected NiSequenceData layout"
        );
        let mut evaluators = Vec::with_capacity(count);
        for _ in 0..count {
            let link = word()?;
            let evaluator = nif
                .blocks
                .get(link as usize)
                .context("Missing sequence evaluator")?;
            ensure!(
                evaluator.type_name().ends_with("Evaluator"),
                "Sequence link {link} is not an evaluator"
            );
            evaluators.push(link);
        }
        let text = word()?;
        let text_keys = if text == u32::MAX {
            None
        } else {
            ensure!(
                nif.blocks
                    .get(text as usize)
                    .context("Missing text-key block")?
                    .type_name()
                    == "NiTextKeyExtraData",
                "Invalid sequence text-key type"
            );
            Some(text)
        };
        let duration = f32::from_bits(word()?);
        let cycle_type = word()?;
        let frequency = f32::from_bits(word()?);
        let accumulation_root = string(word()?)?;
        let flags = word()?;
        ensure!(
            duration.is_finite() && frequency.is_finite(),
            "Nonfinite sequence timing"
        );
        Ok(Self {
            name,
            evaluators,
            text_keys,
            duration,
            cycle_type,
            frequency,
            accumulation_root,
            flags,
        })
    }
}
