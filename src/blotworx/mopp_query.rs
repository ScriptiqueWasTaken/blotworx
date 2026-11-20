//! Original MOPP query preparation. Bytecode traversal is separate.
use anyhow::{ensure, Context, Result};

/// Borrowed source code: importing does not duplicate the HKX byte array.
pub struct Code<'a> {
    pub info: [f32; 4],
    pub bytes: &'a [u8],
}
#[derive(Debug)]
pub struct OwnedCode {
    pub info: [f32; 4],
    pub bytes: Box<[u8]>,
}
impl OwnedCode {
    pub fn query(&self, bounds: [[f32; 4]; 2]) -> Result<Vec<u32>> {
        query(&self.bytes, bounds, self.info)
    }
}
/// Scoped to one HKX import. Repeated source identities share one retained
/// allocation; the source file can be released after its shapes are imported.
pub struct CodeCache<'a> {
    file: &'a epic_mickey_lib_rs::hkx_file::HkxFile,
    entries: std::collections::HashMap<(usize, usize), std::sync::Arc<OwnedCode>>,
}
impl<'a> CodeCache<'a> {
    pub fn new(file: &'a epic_mickey_lib_rs::hkx_file::HkxFile) -> Self {
        Self {
            file,
            entries: std::collections::HashMap::new(),
        }
    }
    pub fn get(&mut self, section: usize, offset: usize) -> Result<std::sync::Arc<OwnedCode>> {
        if let Some(code) = self.entries.get(&(section, offset)) {
            return Ok(code.clone());
        }
        let imported = Code::import(self.file, section, offset)?;
        let owned = std::sync::Arc::new(OwnedCode {
            info: imported.info,
            bytes: imported.bytes.into(),
        });
        self.entries.insert((section, offset), owned.clone());
        Ok(owned)
    }
}
impl<'a> Code<'a> {
    pub fn import(
        file: &'a epic_mickey_lib_rs::hkx_file::HkxFile,
        section: usize,
        offset: usize,
    ) -> Result<Self> {
        ensure!(
            file.header.pointer_size == 4
                && ((file.header.endianness == 0 && file.header.contents_version.starts_with("Havok-7.0"))
                    || (file.header.endianness == 1 && file.header.contents_version == "hk_2012.1.0-r1")),
            "Unsupported MOPP HKX layout"
        );
        let data = file
            .section_data
            .get(section)
            .context("Missing MOPP section")?;
        let end = offset.checked_add(40).context("MOPP header overflow")?;
        let header = data
            .get(offset..end)
            .context("Truncated MOPP code header")?;
        // The header follows the packfile ABI. MOPP instruction operands
        // retain their bytecode-defined big-endian order on every platform.
        let word = |at| {
            let raw = header[at..at + 4].try_into().unwrap();
            if file.header.endianness == 1 {u32::from_le_bytes(raw)} else {u32::from_be_bytes(raw)}
        };
        let info = std::array::from_fn(|i| f32::from_bits(word(16 + 4 * i)));
        ensure!(
            info.iter().all(|v| v.is_finite()) && info[3] > 0.,
            "Invalid MOPP code info"
        );
        let pointer = offset + 32;
        let mut targets = Vec::new();
        if let Some(fixups) = file.local_fixups.get(section) {
            targets.extend(
                fixups
                    .iter()
                    .filter(|f| f.src_offset as usize == pointer)
                    .map(|f| (section, f.dst_offset as usize)),
            );
        }
        if let Some(fixups) = file.global_fixups.get(section) {
            targets.extend(
                fixups
                    .iter()
                    .filter(|f| f.src_offset as usize == pointer)
                    .map(|f| (f.dst_section as usize, f.dst_offset as usize)),
            );
        }
        ensure!(
            targets.len() == 1,
            "Missing or ambiguous MOPP bytecode pointer"
        );
        let (section, start) = targets[0];
        let size = word(36) as usize;
        let end = start
            .checked_add(size)
            .context("MOPP bytecode range overflow")?;
        let bytes = file
            .section_data
            .get(section)
            .and_then(|d| d.get(start..end))
            .context("Truncated MOPP bytecode")?;
        ensure!(!bytes.is_empty(), "Empty MOPP bytecode");
        Ok(Self { info, bytes })
    }
    pub fn query(&self, bounds: [[f32; 4]; 2]) -> Result<Vec<u32>> {
        query(self.bytes, bounds, self.info)
    }
}

/// 60..6b encode four property slots with 8/16/32-bit unsigned values.
/// The AABB terminal routine emits only shape keys; these values do not
/// participate in its spatial comparisons or primitive-key calculation.
pub fn property(code: &[u8], pc: usize) -> Result<Option<(usize, usize, u32)>> {
    let op = *code
        .get(pc)
        .ok_or_else(|| anyhow::anyhow!("MOPP property outside code"))?;
    let (base, width) = match op {
        0x60..=0x63 => (0x60, 1),
        0x64..=0x67 => (0x64, 2),
        0x68..=0x6b => (0x68, 4),
        _ => return Ok(None),
    };
    let start = pc
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("MOPP property overflow"))?;
    let end = start
        .checked_add(width)
        .ok_or_else(|| anyhow::anyhow!("MOPP property overflow"))?;
    let bytes = code
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated MOPP property"))?;
    let value = bytes.iter().fold(0u32, |v, &b| (v << 8) | b as u32);
    Ok(Some((end, (op - base) as usize, value)))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rebase {
    pub origin: [i32; 3],
    pub shift: u32,
}

/// 802f0b3c..0c0c: regenerate coarse bounds from the ORIGINAL fine bounds,
/// not the previous coarse interval. Returns new frame state atomically.
pub fn rebase(
    code: &[u8],
    pc: usize,
    bounds: &IntegerBounds,
    state: Rebase,
) -> Result<Option<(usize, IntegerBounds, Rebase)>> {
    let op = *code
        .get(pc)
        .ok_or_else(|| anyhow::anyhow!("MOPP rebase outside code"))?;
    if !(1..=4).contains(&op) {
        return Ok(None);
    }
    let end = pc
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("MOPP rebase overflow"))?;
    let bytes = code
        .get(pc..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated MOPP rebase"))?;
    let origin = std::array::from_fn(|i| {
        state.origin[i]
            .wrapping_add(bytes[i + 1] as i32)
            .wrapping_shl(op as u32)
    });
    let shift = state.shift.wrapping_add(op as u32);
    let count = 16u32.wrapping_sub(shift) & 63;
    // PPC sraw uses six count bits; >=32 produces the sign word.
    let sraw = |value: i32| {
        if count >= 32 {
            value >> 31
        } else {
            value >> count
        }
    };
    let coarse = std::array::from_fn(|side| {
        std::array::from_fn(|axis| {
            sraw(bounds.fine[side][axis])
                .wrapping_sub(origin[axis])
                .wrapping_add(side as i32)
        })
    });
    Ok(Some((
        end,
        IntegerBounds {
            fine: bounds.fine,
            coarse,
        },
        Rebase { origin, shift },
    )))
}

/// Primitive offsets are traversal-local: native 09/0a add, but 0b
/// REPLACES the offset. Caller copies state for a recursive child.
/// Returns the next PC, or None for a different instruction family.
pub fn primitive_offset(code: &[u8], pc: usize, current: &mut u32) -> Result<Option<usize>> {
    let op = *code
        .get(pc)
        .ok_or_else(|| anyhow::anyhow!("MOPP offset outside code"))?;
    let width = match op {
        9 => 1,
        10 => 2,
        11 => 4,
        _ => return Ok(None),
    };
    let start = pc
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("MOPP offset overflow"))?;
    let end = start
        .checked_add(width)
        .ok_or_else(|| anyhow::anyhow!("MOPP offset overflow"))?;
    let bytes = code
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated MOPP primitive offset"))?;
    let value = bytes.iter().fold(0u32, |v, &b| (v << 8) | b as u32);
    *current = if op == 11 {
        value
    } else {
        current.wrapping_add(value)
    };
    Ok(Some(end))
}

/// Non-spatial VM instructions recovered from 802f0a94..0b38 and
/// 802f0df4..0ec0. None means a spatial/state opcode, not a skipped opcode.
#[derive(Debug, PartialEq)]
pub enum Control {
    Return,
    Jump { target: usize, chunk: Option<u32> },
    Terminal(u32),
}
pub fn control(
    code: &[u8],
    pc: usize,
    primitive_offset: u32,
    chunk: u32,
    chunked: bool,
) -> Result<Option<Control>> {
    let opcode = *code
        .get(pc)
        .ok_or_else(|| anyhow::anyhow!("MOPP program counter outside code"))?;
    let operand = |width: usize| -> Result<u32> {
        let start = pc
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("MOPP operand overflow"))?;
        let end = start
            .checked_add(width)
            .ok_or_else(|| anyhow::anyhow!("MOPP operand overflow"))?;
        let bytes = code
            .get(start..end)
            .ok_or_else(|| anyhow::anyhow!("Truncated MOPP operand"))?;
        Ok(bytes.iter().fold(0u32, |v, &b| (v << 8) | b as u32))
    };
    let jump = |target: usize, chunk| -> Result<Option<Control>> {
        ensure!(target < code.len(), "MOPP jump outside code");
        Ok(Some(Control::Jump { target, chunk }))
    };
    match opcode {
        0 => Ok(Some(Control::Return)),
        5..=7 => {
            let width = (opcode - 4) as usize;
            let offset = operand(width)? as usize;
            let target = pc
                .checked_add(1 + width)
                .and_then(|v| v.checked_add(offset))
                .ok_or_else(|| anyhow::anyhow!("Invalid MOPP relative jump"))?;
            jump(target, None)
        }
        0x0c | 0x70 => {
            let offset = if opcode == 0x0c {
                operand(2)? << 9
            } else {
                operand(4)?
            };
            jump(offset as usize, Some(offset))
        }
        0x0d => {
            operand(4)?;
            jump(pc + 5, None)
        }
        0x30..=0x53 => {
            let id = if opcode < 0x50 {
                (opcode - 0x30) as u32
            } else {
                operand((opcode - 0x4f) as usize)?
            };
            let prefix = if chunked {
                ((chunk as i32 >> 9) as u32) << 8
            } else {
                0
            };
            Ok(Some(Control::Terminal(
                id.wrapping_add(primitive_offset) | prefix,
            )))
        }
        _ => Ok(None),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntegerBounds {
    /// Full precision VM bounds, minimum then maximum.
    pub fine: [[i32; 3]; 2],
    /// Initial traversal bounds after arithmetic shift and upper padding.
    pub coarse: [[i32; 3]; 2],
}

/// Conservative union of terminal keys in the executable MOPP graph. Visit
/// both spatial branches, independent of their bounds. This proves absence,
/// not presence in any particular query, and does not dereference mesh data.
pub fn terminal_keys(code: &[u8]) -> Result<std::collections::HashSet<u32>> {
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    struct Frame {
        pc: usize,
        offset: u32,
        chunk: u32,
    }
    enum Work {
        Enter(Frame),
        Exit(Frame),
    }
    ensure!(!code.is_empty(), "Empty MOPP program");
    let mut pending = vec![Work::Enter(Frame {
        pc: 0,
        offset: 0,
        chunk: 0,
    })];
    let mut active = std::collections::HashSet::new();
    let mut visited = std::collections::HashSet::new();
    let mut keys = std::collections::HashSet::new();
    while let Some(work) = pending.pop() {
        let mut frame = match work {
            Work::Exit(f) => {
                active.remove(&f);
                visited.insert(f);
                continue;
            }
            Work::Enter(f) => f,
        };
        ensure!(
            !active.contains(&frame),
            "Cyclic MOPP terminal graph at {:#x}",
            frame.pc
        );
        if visited.contains(&frame) {
            continue;
        }
        active.insert(frame);
        pending.push(Work::Exit(frame));
        if let Some(c) = control(code, frame.pc, frame.offset, frame.chunk, code[0] == 0x0d)? {
            match c {
                Control::Return => continue,
                Control::Terminal(key) => {
                    keys.insert(key);
                    continue;
                }
                Control::Jump { target, chunk } => {
                    frame.pc = target;
                    if let Some(c) = chunk {
                        frame.chunk = c;
                    }
                }
            }
        } else if let Some(next) = primitive_offset(code, frame.pc, &mut frame.offset)? {
            frame.pc = next;
        } else if let Some((next, _, _)) = property(code, frame.pc)? {
            frame.pc = next;
        } else {
            let pc = frame.pc;
            let op = code[pc];
            let size = match op {
                1..=4 => 4,
                0x10..=0x1f => 4,
                0x20..=0x22 | 0x26..=0x28 => 3,
                0x23..=0x25 | 0x29..=0x2b => 7,
                _ => anyhow::bail!("Unsupported MOPP opcode {op:#x} at {pc:#x}"),
            };
            let row = code
                .get(pc..pc + size)
                .context("Truncated MOPP terminal graph")?;
            let end = pc + size;
            let offsets = match op {
                0x10..=0x1f => vec![0, row[3] as usize],
                0x20..=0x22 => vec![0, row[2] as usize],
                0x23..=0x25 => vec![
                    u16::from_be_bytes([row[3], row[4]]) as usize,
                    u16::from_be_bytes([row[5], row[6]]) as usize,
                ],
                _ => vec![0],
            };
            for offset in offsets.into_iter().rev() {
                let pc = end
                    .checked_add(offset)
                    .context("MOPP terminal graph overflow")?;
                ensure!(pc < code.len(), "MOPP terminal graph target outside code");
                pending.push(Work::Enter(Frame { pc, ..frame }));
            }
            continue;
        }
        pending.push(Work::Enter(frame));
    }
    Ok(keys)
}

/// Execute recovered instruction families without host recursion or result
/// deduplication. Property operands are retained but do not affect AABB keys.
pub fn query(code: &[u8], bounds: [[f32; 4]; 2], info: [f32; 4]) -> Result<Vec<u32>> {
    #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    struct Frame {
        pc: usize,
        bounds: IntegerBounds,
        rebase: Rebase,
        offset: u32,
        chunk: u32,
        properties: [u32; 4],
    }
    enum Work {
        Enter(Frame),
        Exit(Frame),
    }
    ensure!(!code.is_empty(), "Empty MOPP program");
    let bounds = integer_bounds(restrict_bounds(bounds, info)?, info)?;
    let mut pending = vec![Work::Enter(Frame {
        pc: 0,
        bounds,
        rebase: Rebase::default(),
        offset: 0,
        chunk: 0,
        properties: [0; 4],
    })];
    let mut active = std::collections::HashSet::new();
    let mut result = Vec::new();
    while let Some(work) = pending.pop() {
        let mut frame = match work {
            Work::Exit(f) => {
                active.remove(&f);
                continue;
            }
            Work::Enter(f) => f,
        };
        ensure!(
            active.insert(frame),
            "Cyclic MOPP query at byte {:#x}",
            frame.pc
        );
        pending.push(Work::Exit(frame));
        if let Some(control) = control(code, frame.pc, frame.offset, frame.chunk, code[0] == 0x0d)?
        {
            match control {
                Control::Return => continue,
                Control::Terminal(key) => {
                    result.push(key);
                    continue;
                }
                Control::Jump { target, chunk } => {
                    frame.pc = target;
                    if let Some(c) = chunk {
                        frame.chunk = c;
                    }
                }
            }
        } else if let Some(next) = primitive_offset(code, frame.pc, &mut frame.offset)? {
            frame.pc = next;
        } else if let Some((next, slot, value)) = property(code, frame.pc)? {
            frame.pc = next;
            frame.properties[slot] = value;
        } else if let Some((next, bounds, state)) =
            rebase(code, frame.pc, &frame.bounds, frame.rebase)?
        {
            frame.pc = next;
            frame.bounds = bounds;
            frame.rebase = state;
        } else {
            let children = match axis_branch(code, frame.pc, &frame.bounds)? {
                Some(v) => Some(v),
                None => diagonal_branch(code, frame.pc, &frame.bounds)?,
            };
            if let Some(children) = children {
                for pc in children.into_iter().rev() {
                    pending.push(Work::Enter(Frame { pc, ..frame }));
                }
                continue;
            }
            anyhow::bail!(
                "Unsupported MOPP opcode {:#x} at byte {:#x}",
                code[frame.pc],
                frame.pc
            );
        }
        pending.push(Work::Enter(frame));
    }
    Ok(result)
}

/// 802f0648..802f0888, integer diagonal projections. Negative axes select
/// the opposite endpoint. Arithmetic wraps like the original add/subf.
fn diagonal_projection(op: u8, bounds: &IntegerBounds) -> Option<(i32, i32, i32, i32)> {
    let signs = match op {
        0x13 => [0, 1, 1],
        0x14 => [0, 1, -1],
        0x15 => [1, 0, 1],
        0x16 => [1, 0, -1],
        0x17 => [1, 1, 0],
        0x18 => [1, -1, 0],
        0x19 => [1, 1, 1],
        0x1a => [1, 1, -1],
        0x1b => [1, -1, 1],
        0x1c => [1, -1, -1],
        _ => return None,
    };
    let (mut lo, mut hi) = (0i32, 0i32);
    let (mut count, mut bias) = (0, 0);
    for (axis, sign) in signs.into_iter().enumerate() {
        if sign == 1 {
            lo = lo.wrapping_add(bounds.coarse[0][axis]);
            hi = hi.wrapping_add(bounds.coarse[1][axis]);
            count += 1;
        } else if sign == -1 {
            lo = lo.wrapping_sub(bounds.coarse[1][axis]);
            hi = hi.wrapping_sub(bounds.coarse[0][axis]);
            count += 1;
            bias -= 255;
        }
    }
    Some((lo, hi, count, bias))
}

/// Diagonal split children in native left-first order (802f088c..08d0).
pub fn diagonal_branch(
    code: &[u8],
    pc: usize,
    bounds: &IntegerBounds,
) -> Result<Option<Vec<usize>>> {
    let op = *code
        .get(pc)
        .ok_or_else(|| anyhow::anyhow!("MOPP branch outside code"))?;
    let Some((lo, hi, factor, bias)) = diagonal_projection(op, bounds) else {
        return Ok(None);
    };
    let end = pc
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("MOPP branch overflow"))?;
    let bytes = code
        .get(pc..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated diagonal MOPP branch"))?;
    let left_limit = bytes[1] as i32 * factor + bias;
    let right_limit = bytes[2] as i32 * factor + bias;
    let mut children = Vec::with_capacity(2);
    if lo < left_limit {
        children.push(end);
    }
    if hi > right_limit {
        children.push(
            end.checked_add(bytes[3] as usize)
                .ok_or_else(|| anyhow::anyhow!("MOPP branch overflow"))?,
        );
    }
    // If upper <= right threshold and lower >= left threshold, return.
    for &target in &children {
        ensure!(target < code.len(), "MOPP diagonal target outside code");
    }
    Ok(Some(children))
}

/// Axis branch destinations in execution order. Empty means return; None
/// means another instruction family. Bounds remain min/max in this API.
/// Recovered from 802f08d4..802f0a90; do not make split comparisons uniform.
pub fn axis_branch(code: &[u8], pc: usize, bounds: &IntegerBounds) -> Result<Option<Vec<usize>>> {
    let op = *code
        .get(pc)
        .ok_or_else(|| anyhow::anyhow!("MOPP branch outside code"))?;
    let (axis, len) = match op {
        0x10..=0x12 => ((op - 0x10) as usize, 4),
        0x20..=0x22 => ((op - 0x20) as usize, 3),
        0x23..=0x25 => ((op - 0x23) as usize, 7),
        0x26..=0x28 => ((op - 0x26) as usize, 3),
        0x29..=0x2b => ((op - 0x29) as usize, 7),
        _ => return Ok(None),
    };
    let end = pc
        .checked_add(len)
        .ok_or_else(|| anyhow::anyhow!("MOPP branch overflow"))?;
    let bytes = code
        .get(pc..end)
        .ok_or_else(|| anyhow::anyhow!("Truncated MOPP branch"))?;
    let lo = bounds.coarse[0][axis];
    let hi = bounds.coarse[1][axis];
    let mut offsets = Vec::with_capacity(2);
    match op {
        0x10..=0x12 | 0x23..=0x25 => {
            let (left, right) = if len == 4 {
                (0, bytes[3] as usize)
            } else {
                (
                    u16::from_be_bytes([bytes[3], bytes[4]]) as usize,
                    u16::from_be_bytes([bytes[5], bytes[6]]) as usize,
                )
            };
            if hi <= bytes[2] as i32 {
                if lo < (bytes[1] as i32) {
                    offsets.push(left);
                }
            } else {
                if lo < (bytes[1] as i32) {
                    offsets.push(left);
                }
                offsets.push(right);
            }
        }
        0x20..=0x22 => {
            let split = bytes[1] as i32;
            if hi <= split {
                offsets.push(0);
            } else {
                if lo <= split {
                    offsets.push(0);
                }
                offsets.push(bytes[2] as usize);
            }
        }
        0x26..=0x28 => {
            if hi >= bytes[1] as i32 && lo < (bytes[2] as i32) {
                offsets.push(0);
            }
        }
        _ => {
            let a = ((bytes[1] as i32) << 16) | ((bytes[2] as i32) << 8) | bytes[3] as i32;
            let b = ((bytes[4] as i32) << 16) | ((bytes[5] as i32) << 8) | bytes[6] as i32;
            if bounds.fine[1][axis] >= a && bounds.fine[0][axis] <= b {
                offsets.push(0);
            }
        }
    }
    for offset in &mut offsets {
        *offset = end
            .checked_add(*offset)
            .ok_or_else(|| anyhow::anyhow!("MOPP branch target overflow"))?;
        ensure!(*offset < code.len(), "MOPP branch target outside code");
    }
    Ok(Some(offsets))
}

/// 802f1384..802f14b0. Subtraction and multiplication round separately;
/// fctiwz truncates toward zero, then the integer endpoints expand by one.
/// Call after restrict_bounds, using the MOPP code object's own info.
pub fn integer_bounds(bounds: [[f32; 4]; 2], info: [f32; 4]) -> Result<IntegerBounds> {
    ensure!(
        bounds.iter().flatten().chain(&info).all(|v| v.is_finite()) && info[3] > 0.,
        "Invalid MOPP integer query input"
    );
    let mut fine = [[0i32; 3]; 2];
    let mut coarse = [[0i32; 3]; 2];
    for side in 0..2 {
        for axis in 0..3 {
            let difference = bounds[side][axis] - info[axis];
            let scaled = info[3] * difference;
            // Invalid-conversion FPSCR behavior is not yet captured. Diagnose
            // instead of relying on Rust's saturating cast for such inputs.
            ensure!(
                scaled.is_finite() && scaled >= -2147483648. && scaled < 2147483648.,
                "MOPP query coordinate exceeds verified integer conversion range"
            );
            let integer = scaled as i32;
            fine[side][axis] = if side == 0 {
                integer.wrapping_sub(1)
            } else {
                integer.wrapping_add(1)
            };
            coarse[side][axis] = (fine[side][axis] >> 16) + side as i32;
        }
    }
    Ok(IntegerBounds { fine, coarse })
}

/// MOPP BV-tree virtual +38 (802763d4): restrict the incoming AABB to
/// the code's representable domain before integer conversion. Info contains
/// XYZ offsets and W scale. Preserve all paired lanes and native tie choices.
pub fn restrict_bounds(bounds: [[f32; 4]; 2], info: [f32; 4]) -> Result<[[f32; 4]; 2]> {
    ensure!(
        bounds.iter().flatten().chain(&info).all(|v| v.is_finite()) && info[3] > 0.,
        "Invalid MOPP query bounds/scale"
    );
    let extent = f32::from_bits(0x4b800000) / info[3];
    ensure!(extent.is_finite(), "MOPP query domain overflow");
    let upper = info.map(|v| v + extent);
    ensure!(
        upper.iter().all(|v| v.is_finite()),
        "MOPP query domain overflow"
    );
    Ok([
        std::array::from_fn(|i| {
            if info[i] - bounds[0][i] >= 0. {
                info[i]
            } else {
                bounds[0][i]
            }
        }),
        std::array::from_fn(|i| {
            if bounds[1][i] - upper[i] >= 0. {
                upper[i]
            } else {
                bounds[1][i]
            }
        }),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_graph_visits_both_branches_with_independent_offsets() -> Result<()> {
        // Left branch changes its primitive offset; the right must retain 0.
        let keys = terminal_keys(&[0x10, 0, 255, 3, 0x09, 4, 0x31, 0x32])?;
        assert_eq!(keys, std::collections::HashSet::from([5, 2]));
        let keys = terminal_keys(&[0x23, 0, 255, 0, 0, 0, 3, 0x09, 4, 0x31, 0x32])?;
        assert_eq!(keys, std::collections::HashSet::from([5, 2]));
        for code in [&[0x70, 0, 0, 0, 0][..], &[0x23, 0][..], &[0xff][..]] {
            assert!(terminal_keys(code).is_err());
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_zone_mesh_welding_audit() -> Result<()> {
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            std::path::Path::new(&root).join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let identity = super::super::native_hk_transform::from_entity(
            [1., 0., 0., 0., 1., 0., 0., 0., 1.],
            [0.; 3],
        );
        let mut report = Vec::new();
        let mut compared_total = 0;
        let mut mismatch_total = 0;
        for asset in &pak.files {
            let name = asset.path.to_ascii_lowercase();
            if !name.ends_with(".hkx_wii") || !name.contains("mesh") {
                continue;
            }
            let audit = (|| -> Result<serde_json::Value> {
                let file = epic_mickey_lib_rs::hkx_file::HkxFile::load_bytes(&asset.data)
                    .map_err(anyhow::Error::msg)?;
                let parts = super::super::collision::body_local_meshes(&file)?;
                let mut compared = 0;
                let mut mismatches = 0;
                let mut generated_count = 0;
                let mut details = Vec::new();
                let mut cross_body_mismatches = 0;
                let mut cross_body_details = Vec::new();
                let mut excluded = Vec::new();
                let bodies = super::super::dynamic_body::layout(&parts, &identity)?;
                let collections = bodies
                    .iter()
                    .filter(|b| parts[b.parts[0]].mopp.is_some())
                    .map(|b| (b, b.initial_transform))
                    .collect::<Vec<_>>();
                for body in &bodies {
                    let cross_generated = body
                        .generate_welding(&parts, &body.initial_transform, &collections, true)?
                        .into_iter()
                        .collect::<std::collections::BTreeMap<_, _>>();
                    let generated = body
                        .generate_local_welding(&parts)?
                        .into_iter()
                        .collect::<std::collections::BTreeMap<_, _>>();
                    generated_count += generated.len();
                    for &index in &body.parts {
                        let part = &parts[index];
                        if let Some(w) = &part.welding {
                            if w.mode == 5 {
                                if let Some(words) =
                                    w.subpart(part.welding_offset, part.indices.len() / 3)?
                                {
                                    for (i, &word) in words.iter().enumerate() {
                                        let key = part.shape_keys.key(Some(i))?;
                                        let extract =
                                            |p: &super::super::collision::StaticMesh, n: usize| {
                                                std::array::from_fn::<_, 3, _>(|j| {
                                                    let v =
                                                        p.vertices[p.indices[n * 3 + j] as usize];
                                                    [v[0], v[1], v[2], 0.]
                                                })
                                            };
                                        let source = extract(part, i);
                                        if super::super::shape_welding::degenerate_triangle(source)?
                                        {
                                            ensure!(
                                                !generated.contains_key(&key)
                                                    && !cross_generated.contains_key(&key),
                                                "Degenerate source generated a write"
                                            );
                                            excluded.push(serde_json::json!({"body":body.body_offset,"key":key,"stored":word}));
                                            continue;
                                        }
                                        let rebuilt = *generated
                                            .get(&key)
                                            .context("Missing eligible welding write")?;
                                        compared += 1;
                                        let cross_body = *cross_generated
                                            .get(&key)
                                            .context("Missing cross-collection welding write")?;
                                        if cross_body != word {
                                            cross_body_mismatches += 1;
                                            cross_body_details.push(serde_json::json!({"body":body.body_offset,"key":key,"stored":word,"generated":cross_body,
                                                "excluded_by_mesh_iterator":super::super::shape_welding::degenerate_triangle(source)?}));
                                        }
                                        if rebuilt != word {
                                            mismatches += 1;
                                            let exhaustive =
                                                super::super::shape_welding::generate_triangle(
                                                    true,
                                                    |edge, intervals| {
                                                        let frame=super::super::shape_welding::CandidateFrame::in_collection(source,&identity,&identity,edge)?;
                                                        for &candidate in &body.parts {
                                                            let p = &parts[candidate];
                                                            for n in 0..p.indices.len() / 3 {
                                                                frame.accumulate(
                                                                    extract(p, n),
                                                                    intervals,
                                                                )?;
                                                            }
                                                        }
                                                        Ok(())
                                                    },
                                                )?;
                                            details.push(serde_json::json!({"body":body.body_offset,"key":key,"stored":word,"generated":rebuilt,
                                            "cross_body":cross_body,
                                            "exhaustive":exhaustive,
                                            "vertices":part.indices[i*3..i*3+3].iter().map(|&v|part.vertices[v as usize]).collect::<Vec<_>>()}));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                compared_total += compared;
                mismatch_total += mismatches;
                Ok(
                    serde_json::json!({"asset":asset.path,"generated":generated_count,"compared":compared,"mismatches":mismatches,"details":details,
                    "cross_body_mismatches":cross_body_mismatches,"cross_body_details":cross_body_details,"excluded":excluded}),
                )
            })();
            report.push(match audit {
                Ok(v) => v,
                Err(e) => serde_json::json!({"asset":asset.path,"error":format!("{e:#}")}),
            });
        }
        std::fs::write(
            "target/zone-mesh-welding-audit.json",
            serde_json::to_vec_pretty(&report)?,
        )?;
        println!("{} mesh-named resources, {compared_total} eligible words compared, {mismatch_total} single-body scope differences",report.len());
        ensure!(
            report.iter().all(|v| v.get("error").is_none()),
            "Mesh audit failed to import/generate a resource"
        );
        // Authored seams span bodies: compare the complete collection scope,
        // retaining single-body differences as evidence of that distinction.
        ensure!(
            report.len() == 7 && compared_total == 4909,
            "Original zone fixture coverage changed"
        );
        ensure!(
            report
                .iter()
                .map(|v| v["excluded"].as_array().map_or(0, Vec::len))
                .sum::<usize>()
                == 1,
            "Original zone degenerate-key coverage changed"
        );
        ensure!(
            report.iter().all(|v| v["cross_body_mismatches"] == 0),
            "Cross-collection welding mismatch; see zone audit report"
        );
        Ok(())
    }
    #[test]
    #[ignore = "requires original game files in BLOTWORX_TEST_DATA"]
    fn production_platform_mopp_query() -> Result<()> {
        use anyhow::Context;
        let root = std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;
        let pak = epic_mickey_lib_rs::packfile::Packfile::from_binary(std::fs::read(
            std::path::Path::new(&root).join("packfiles/gremlinvillage_zonea.pak"),
        )?);
        let asset = pak
            .files
            .iter()
            .find(|f| {
                f.path
                    .to_ascii_lowercase()
                    .contains("gv_zonea_platform01_dynamic_mesh.hkx")
            })
            .context("Missing platform HKX")?;
        let file = epic_mickey_lib_rs::hkx_file::HkxFile::load_bytes(&asset.data)
            .map_err(anyhow::Error::msg)?;
        let mut cache = CodeCache::new(&file);
        let mut count = 0;
        for (section, fixups) in file.virtual_fixups.iter().enumerate() {
            for fixup in fixups {
                let names = &file.section_data[fixup.section_index as usize]
                    [fixup.class_name_offset as usize..];
                if !names.starts_with(b"hkpMoppCode\0") {
                    continue;
                }
                let resource = Code::import(&file, section, fixup.src_offset as usize)?;
                let retained = cache.get(section, fixup.src_offset as usize)?;
                let shared = cache.get(section, fixup.src_offset as usize)?;
                ensure!(
                    std::sync::Arc::ptr_eq(&retained, &shared),
                    "Duplicate MOPP allocation"
                );
                ensure!(
                    retained.bytes.as_ref() == resource.bytes && retained.info == resource.info,
                    "Changed retained MOPP data"
                );
                let info = resource.info;
                let code = resource.bytes;
                let size = code.len();
                let extent = f32::from_bits(0x4b800000) / info[3];
                let keys = query(code, [info, info.map(|v| v + extent)], info)?;
                ensure!(
                    retained.query([info, info.map(|v| v + extent)])? == keys,
                    "Retained MOPP changed query order"
                );
                ensure!(!keys.is_empty(), "Platform MOPP returned no keys");
                let geometry = super::super::collision::body_local_meshes(&file)?;
                let identity = super::super::native_hk_transform::from_entity(
                    [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                    [0.; 3],
                );
                let bodies = super::super::dynamic_body::layout(&geometry, &identity)?;
                ensure!(bodies.len() == 1, "Platform body count changed");
                let resolved =
                    bodies[0].query_local(&geometry, [info, info.map(|v| v + extent)])?;
                let resolved_keys = resolved
                    .iter()
                    .map(|&(part, triangle)| geometry[part].shape_keys.key(triangle))
                    .collect::<Result<Vec<_>>>()?;
                ensure!(
                    resolved_keys == keys,
                    "Body query changed ordered shape keys"
                );
                let expected = geometry
                    .iter()
                    .flat_map(|part| {
                        (0..part.indices.len() / 3)
                            .map(|triangle| part.shape_keys.key(Some(triangle)))
                    })
                    .collect::<Result<std::collections::BTreeSet<_>>>()?;
                let actual = keys
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>();
                ensure!(
                    actual == expected,
                    "Platform MOPP keys differ from decoded triangle keys"
                );
                let mut probes = 0;
                let mut smallest = usize::MAX;
                let mut largest = 0;
                for part in &geometry {
                    for (triangle, indices) in part.indices.chunks_exact(3).enumerate() {
                        let vertices =
                            std::array::from_fn::<_, 3, _>(|i| part.vertices[indices[i] as usize]);
                        let center = std::array::from_fn::<_, 4, _>(|axis| {
                            if axis == 3 {
                                0.
                            } else {
                                ((vertices[0][axis] as f64
                                    + vertices[1][axis] as f64
                                    + vertices[2][axis] as f64)
                                    / 3.) as f32
                            }
                        });
                        let probe = [center.map(|v| v - 0.0001), center.map(|v| v + 0.0001)];
                        let candidates = query(code, probe, info)?;
                        ensure!(
                            candidates.contains(&part.shape_keys.key(Some(triangle))?),
                            "Triangle {triangle} absent from its centroid query"
                        );
                        ensure!(
                            candidates.iter().all(|key| expected.contains(key)),
                            "Unknown spatial-query key"
                        );
                        smallest = smallest.min(candidates.len());
                        largest = largest.max(candidates.len());
                        probes += 1;
                    }
                }
                ensure!(
                    smallest < keys.len(),
                    "Spatial probes never narrowed full-domain query"
                );
                let mut triangles = std::collections::BTreeMap::new();
                for part in &geometry {
                    for (i, indices) in part.indices.chunks_exact(3).enumerate() {
                        let vertices = std::array::from_fn::<_, 3, _>(|v| {
                            let p = part.vertices[indices[v] as usize];
                            [p[0], p[1], p[2], 0.]
                        });
                        triangles.insert(part.shape_keys.key(Some(i))?, vertices);
                    }
                }
                use super::super::shape_welding::{generate_triangle, CandidateFrame};
                let identity = super::super::native_hk_transform::from_entity(
                    [1., 0., 0., 0., 1., 0., 0., 0., 1.],
                    [0.; 3],
                );
                let mut accepted = 0;
                let mut words = Vec::new();
                for &vertices in triangles.values() {
                    let word = generate_triangle(true, |edge, intervals| {
                        let frame =
                            CandidateFrame::in_collection(vertices, &identity, &identity, edge)?;
                        accepted += frame.accumulate_mopp(
                            code,
                            info,
                            |key| Ok(Some(*triangles.get(&key).context("Missing MOPP triangle")?)),
                            intervals,
                        )?;
                        Ok(())
                    })?;
                    words.push(word);
                }
                ensure!(
                    accepted > 0,
                    "Platform welding accepted no neighbor intervals"
                );
                let generated = triangles
                    .keys()
                    .copied()
                    .zip(words.iter().copied())
                    .collect::<std::collections::BTreeMap<_, _>>();
                let body_generated = bodies[0]
                    .generate_local_welding(&geometry)?
                    .into_iter()
                    .collect::<std::collections::BTreeMap<_, _>>();
                ensure!(
                    body_generated == generated,
                    "Body welding path differs from raw query generation"
                );
                use super::super::shape_welding::{Prepared, Welding};
                let Prepared::Authored(retained) = bodies[0]
                    .prepare_welding(&geometry)?
                    .context("Missing prepared platform welding")?
                else {
                    anyhow::bail!("Authored platform welding regenerated unnecessarily");
                };
                ensure!(
                    std::sync::Arc::ptr_eq(&retained, geometry[0].welding.as_ref().unwrap()),
                    "Copied authored welding"
                );
                // Alter only newly decoded in-memory fixtures, never original files.
                for (mode, initial_words) in [(4, retained.words.clone()), (5, Vec::new())] {
                    let mut fixture = super::super::collision::body_local_meshes(&file)?;
                    let initial = std::sync::Arc::new(Welding {
                        mode,
                        words: initial_words,
                    });
                    for part in &mut fixture {
                        part.welding = Some(initial.clone());
                    }
                    let Prepared::TwoSided(storage) = bodies[0]
                        .prepare_welding(&fixture)?
                        .context("Missing regenerated welding")?
                    else {
                        anyhow::bail!("Required welding regeneration skipped");
                    };
                    for part in &fixture {
                        for triangle in 0..part.indices.len() / 3 {
                            ensure!(
                                storage.word(part.subpart_index, triangle)?
                                    == generated[&part.shape_keys.key(Some(triangle))?],
                                "Prepared welding differs from generator"
                            );
                        }
                    }
                    ensure!(initial.mode == mode, "Preparation mutated source mode");
                }
                let mut comparison = Vec::new();
                let mut compared = 0;
                let mut differences = 0;
                for part in &geometry {
                    let welding = part
                        .welding
                        .as_ref()
                        .context("Platform missing welding metadata")?;
                    let stored = welding.subpart(part.welding_offset, part.indices.len() / 3)?;
                    if let Some(stored) = stored {
                        for (i, &word) in stored.iter().enumerate() {
                            let key = part.shape_keys.key(Some(i))?;
                            let rebuilt = generated[&key];
                            compared += 1;
                            if word != rebuilt {
                                differences += 1;
                            }
                            comparison.push(serde_json::json!({"key":key,"mode":welding.mode,
                                "stored":word,"generated":rebuilt}));
                        }
                    }
                    println!(
                        "Stored welding mode {}, {} words",
                        welding.mode,
                        stored.map_or(0, |v| v.len())
                    );
                }
                std::fs::write(
                    "target/platform-welding-comparison.json",
                    serde_json::to_vec_pretty(&comparison)?,
                )?;
                println!(
                    "Authored comparison: {compared} words, {differences} full-word differences"
                );
                ensure!(
                    compared == triangles.len() && differences == 0,
                    "Generated platform edge fields differ from authored welding"
                );
                println!(
                    "{} welding words; {accepted} accepted intervals",
                    words.len()
                );
                println!("{probes} centroid probes: {smallest}..{largest} candidates");
                println!(
                    "{}: {} code bytes, {} full-domain keys",
                    asset.path,
                    size,
                    keys.len()
                );
                count += 1;
            }
        }
        ensure!(count > 0, "No platform MOPP code found");
        Ok(())
    }
    #[test]
    fn properties_decode_all_slots_and_do_not_change_aabb_keys() {
        for slot in 0..4u8 {
            assert_eq!(
                property(&[0x60 + slot, 0xab], 0).unwrap(),
                Some((2, slot as usize, 0xab))
            );
            assert_eq!(
                property(&[0x64 + slot, 0xab, 0xcd], 0).unwrap(),
                Some((3, slot as usize, 0xabcd))
            );
            assert_eq!(
                property(&[0x68 + slot, 0x89, 0xab, 0xcd, 0xef], 0).unwrap(),
                Some((5, slot as usize, 0x89abcdef))
            );
        }
        let bounds = [[0.; 4], [1.; 4]];
        let info = [0., 0., 0., 65536.];
        assert_eq!(
            query(
                &[0x60, 255, 0x67, 1, 2, 0x68, 0xff, 0xff, 0xff, 0xff, 0x31],
                bounds,
                info
            )
            .unwrap(),
            vec![1]
        );
        assert!(query(&[0x6b, 1], bounds, info).is_err());
    }
    #[test]
    fn executor_keeps_order_duplicates_and_child_offsets_isolated() {
        let bounds = [[0.; 4], [1.; 4]];
        let info = [0., 0., 0., 65536.];
        // Left increments its offset; right must retain the parent's zero.
        assert_eq!(
            query(&[0x20, 0, 3, 9, 5, 0x31, 0x32], bounds, info).unwrap(),
            vec![6, 2]
        );
        assert_eq!(
            query(&[0x20, 0, 1, 0x31, 0x31], bounds, info).unwrap(),
            vec![1, 1]
        );
        assert!(query(&[0x70, 0, 0, 0, 0], bounds, info)
            .unwrap_err()
            .to_string()
            .contains("Cyclic"));
        assert!(query(&[0x08], bounds, info)
            .unwrap_err()
            .to_string()
            .contains("Unsupported"));
    }
    #[test]
    fn rebase_recomputes_from_fine_bounds_and_preserves_native_shift_rules() {
        let b = IntegerBounds {
            fine: [[-65537, 65535, 131071], [-1, 131073, 262145]],
            coarse: [[999; 3]; 2],
        };
        let (_, out, state) = rebase(&[1, 1, 2, 3], 0, &b, Rebase::default())
            .unwrap()
            .unwrap();
        assert_eq!(
            state,
            Rebase {
                origin: [2, 4, 6],
                shift: 1
            }
        );
        assert_eq!(out.coarse, [[-5, -3, -3], [-2, 1, 3]]);
        assert_eq!(out.fine, b.fine);
        let (_, out, state) = rebase(&[1, 0, 0, 0], 0, &out, state).unwrap().unwrap();
        assert_eq!(state.origin, [4, 8, 12]);
        assert_eq!(out.coarse, [[-9, -5, -5], [-4, 1, 5]]);
        let (_, out, _) = rebase(
            &[1, 0, 0, 0],
            0,
            &b,
            Rebase {
                origin: [0; 3],
                shift: 16,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(out.coarse, [[-1, 0, 0], [0, 1, 1]]);
        assert!(rebase(&[4, 1], 0, &b, Rebase::default()).is_err());
    }
    #[test]
    fn primitive_offsets_add_small_operands_but_replace_full_words() {
        let mut offset = 0xffff_fffe;
        assert_eq!(primitive_offset(&[9, 3], 0, &mut offset).unwrap(), Some(2));
        assert_eq!(offset, 1);
        primitive_offset(&[10, 1, 2], 0, &mut offset).unwrap();
        assert_eq!(offset, 0x103);
        primitive_offset(&[11, 0x12, 0x34, 0x56, 0x78], 0, &mut offset).unwrap();
        assert_eq!(offset, 0x12345678);
        let parent = offset;
        let mut child = parent;
        primitive_offset(&[9, 1], 0, &mut child).unwrap();
        assert_eq!(
            control(&[0x31], 0, child, 0, false).unwrap(),
            Some(Control::Terminal(0x1234567a))
        );
        assert_eq!(offset, parent);
        assert!(primitive_offset(&[11, 1], 0, &mut offset).is_err());
        assert_eq!(offset, parent);
    }
    #[test]
    fn diagonal_projection_uses_opposite_corners_for_negative_axes() {
        let b = IntegerBounds {
            fine: [[0; 3]; 2],
            coarse: [[10, 20, 30], [40, 50, 60]],
        };
        let expected = [
            (50, 110),
            (-40, 20),
            (40, 100),
            (-50, 10),
            (30, 90),
            (-40, 20),
            (60, 150),
            (-30, 60),
            (-10, 80),
            (-100, -10),
        ];
        for (i, (lo, hi)) in expected.into_iter().enumerate() {
            let p = diagonal_projection(0x13 + i as u8, &b).unwrap();
            assert_eq!((p.0, p.1), (lo, hi));
        }
        assert_eq!(
            diagonal_branch(&[0x13, 30, 50, 1, 0, 0], 0, &b).unwrap(),
            Some(vec![4, 5])
        );
        assert_eq!(
            diagonal_branch(&[0x13, 25, 55, 1, 0, 0], 0, &b).unwrap(),
            Some(vec![])
        );
        assert_eq!(
            diagonal_branch(&[0x14, 110, 130, 1, 0, 0], 0, &b).unwrap(),
            Some(vec![4, 5])
        );
    }
    #[test]
    fn axis_splits_keep_native_boundary_rules_and_left_first_order() {
        let mut bounds = IntegerBounds {
            fine: [[10; 3], [20; 3]],
            coarse: [[10; 3], [20; 3]],
        };
        assert_eq!(
            axis_branch(&[0x10, 11, 19, 1, 0, 0], 0, &bounds).unwrap(),
            Some(vec![4, 5])
        );
        assert_eq!(
            axis_branch(&[0x10, 10, 20, 1, 0, 0], 0, &bounds).unwrap(),
            Some(vec![])
        );
        assert_eq!(
            axis_branch(&[0x20, 10, 1, 0, 0], 0, &bounds).unwrap(),
            Some(vec![3, 4])
        );
        assert_eq!(
            axis_branch(&[0x23, 11, 19, 0, 1, 0, 0, 0, 0], 0, &bounds).unwrap(),
            Some(vec![8, 7])
        );
        assert_eq!(
            axis_branch(&[0x26, 20, 10, 0], 0, &bounds).unwrap(),
            Some(vec![])
        );
        assert_eq!(
            axis_branch(&[0x29, 0, 0, 20, 0, 0, 10, 0], 0, &bounds).unwrap(),
            Some(vec![7])
        );
        bounds.coarse = [[0; 3]; 2];
        assert_eq!(
            axis_branch(&[0x21, 0, 1, 0, 0], 0, &bounds).unwrap(),
            Some(vec![3])
        );
        assert!(axis_branch(&[0x23], 0, &bounds).is_err());
    }
    #[test]
    fn control_flow_and_terminal_keys_match_native_encodings() {
        assert_eq!(
            control(&[5, 1, 0, 0x31], 0, 0, 0, false).unwrap(),
            Some(Control::Jump {
                target: 3,
                chunk: None
            })
        );
        assert_eq!(
            control(&[0x51, 0x12, 0x34], 0, 5, 0, false).unwrap(),
            Some(Control::Terminal(0x1239))
        );
        assert_eq!(
            control(&[0x30], 0, 0xff, 0x400, true).unwrap(),
            Some(Control::Terminal(0x2ff))
        );
        assert_eq!(
            control(&[0x53, 0xff, 0xff, 0xff, 0xff], 0, 1, 0, false).unwrap(),
            Some(Control::Terminal(0))
        );
        assert_eq!(
            control(&[0], 0, 0, 0, false).unwrap(),
            Some(Control::Return)
        );
        assert_eq!(control(&[0x10], 0, 0, 0, false).unwrap(), None);
        assert!(control(&[0x52, 1], 0, 0, 0, false).is_err());
        assert!(control(&[5, 255], 0, 0, 0, false).is_err());
    }
    #[test]
    fn integer_query_uses_truncation_padding_and_arithmetic_shift() {
        let query = integer_bounds(
            [[-0.75, 65536., -65536., 0.], [0.75, 65535., -65537., 0.]],
            [0., 0., 0., 1.],
        )
        .unwrap();
        assert_eq!(query.fine, [[-1, 65535, -65537], [1, 65536, -65536]]);
        assert_eq!(query.coarse, [[-1, 0, -2], [1, 2, 0]]);
        let info = [10., 20., 30., 2.];
        assert_eq!(
            integer_bounds([[10.75, 20.75, 30.75, 0.]; 2], info)
                .unwrap()
                .fine,
            [[0; 3], [2; 3]]
        );
        assert!(integer_bounds([[f32::MAX; 4]; 2], info).is_err());
    }
    #[test]
    fn query_domain_clipping_preserves_disjoint_bounds_and_signed_ties() {
        let scale = f32::from_bits(0x4b800000);
        let info = [-0., 10., -10., scale];
        let result = restrict_bounds([[-1., 9., -11., 0.], [2., 12., -8., scale]], info).unwrap();
        assert_eq!(result, [[0., 10., -10., scale], [1., 11., -9., scale]]);
        assert_eq!(result[0][0].to_bits(), (-0f32).to_bits());
        let outside = restrict_bounds([[4.; 4], [5.; 4]], [0., 0., 0., scale]).unwrap();
        assert_eq!(outside[0][0], 4.);
        assert_eq!(outside[1][0], 1.); // No fabricated empty point or reordered bounds.
        let tied = restrict_bounds([[0.; 4], [0.; 4]], info).unwrap();
        assert_eq!(tied[0][0].to_bits(), (-0f32).to_bits());
        assert!(restrict_bounds([[0.; 4]; 2], [0.; 4]).is_err());
    }
}
