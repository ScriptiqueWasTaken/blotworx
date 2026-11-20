//! Native Wwise transition math: DOL80376770/803765f0/803773a0.
//! Lookup values captured read-only from the original USA game, not host libm.
use anyhow::{ensure, Context, Result};
const DB_RANGE: f32 = 96.3;
#[derive(Clone, Debug)]
struct Lookup {
    values: Vec<f32>,
    tail: f32,
    minimum: f32,
    maximum: f32,
    scale: f32,
}
impl Lookup {
    fn sample(&self, value: f32, clamped: bool) -> Result<f32> {
        if clamped && value >= self.maximum {
            return Ok(*self.values.last().unwrap());
        }
        if clamped && value <= self.minimum {
            return Ok(self.values[0]);
        }
        let index = (value - self.minimum) * self.scale;
        ensure!(
            index.is_finite() && index >= 0.,
            "Lookup input outside native table domain"
        );
        let whole = index.trunc() as usize;
        let a = *self
            .values
            .get(whole)
            .context("Lookup exceeds captured native table")?;
        let b = self.values.get(whole + 1).copied().unwrap_or(self.tail);
        let fraction = index - whole as f32;
        Ok(a + fraction * (b - a))
    }
}
pub struct Tables {
    slots: Vec<Lookup>,
}
impl Tables {
    pub fn retail() -> Result<Self> {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_lookup_tables.json"
        ))?;
        ensure!(
            data["game_id"] == "SEME4Q",
            "Unsupported native audio table identity"
        );
        let word = |value: &serde_json::Value| -> Result<u32> {
            Ok(u32::from_str_radix(
                value
                    .as_str()
                    .context("Missing captured table word")?
                    .trim_start_matches("0x"),
                16,
            )?)
        };
        let mut slots = Vec::new();
        for table in data["tables"].as_array().context("Missing audio tables")? {
            ensure!(
                table["slot"].as_u64() == Some(slots.len() as u64),
                "Audio table slots out of order"
            );
            let header = table["header"].as_array().context("Missing table header")?;
            let values = table["values_bits"]
                .as_array()
                .context("Missing table samples")?
                .iter()
                .map(|value| Ok(f32::from_bits(word(value)?)))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                values.len() >= 2 && values.len() == word(&header[5])? as usize,
                "Native table count mismatch"
            );
            let lookup = Lookup {
                values,
                tail: f32::from_bits(word(&table["tail_bits"])?),
                minimum: f32::from_bits(word(&header[4])?),
                maximum: f32::from_bits(word(&header[3])?),
                scale: f32::from_bits(word(&header[6])?),
            };
            ensure!(
                lookup.values.iter().all(|v| v.is_finite())
                    && lookup.tail.is_finite()
                    && lookup.minimum.is_finite()
                    && lookup.maximum > lookup.minimum
                    && lookup.scale > 0.,
                "Invalid captured audio table"
            );
            slots.push(lookup);
        }
        ensure!(slots.len() == 7, "Incomplete native audio math tables");
        Ok(Self { slots })
    }
    pub fn db_to_linear(&self, value: f32) -> Result<f32> {
        ensure!(value.is_finite(), "Non-finite audio gain");
        self.slots[0].sample(value, true)
    }
    pub fn linear_to_db(&self, mut value: f32) -> Result<f32> {
        ensure!(value.is_finite(), "Non-finite audio level");
        if value <= 0. {
            return Ok(-DB_RANGE);
        }
        let mut offset = 0.;
        while value > 10. {
            value /= 10.;
            offset += 20.;
        }
        while value < 1. {
            value *= 10.;
            offset -= 20.;
        }
        Ok(offset + self.slots[1].sample(value, false)?)
    }
    /// Native generic interpolation80375d00. Conversion curves do not apply
    /// the transition system's separate descending-curve orientation rule.
    pub fn interpolate(&self, phase: f32, start: f32, end: f32, curve: u32) -> Result<f32> {
        let lookup = |slot: usize, p: f32| self.slots[slot].sample(p, false);
        let shape = match curve {
            4 => phase,
            6 => lookup(2, phase)?,
            7 => lookup(3, phase)?,
            8 => lookup(4, phase)?,
            0 => 1. - lookup(4, 1. - phase)?,
            1 => 1. - lookup(3, 1. - phase)?,
            2 => 1. - lookup(2, 1. - phase)?,
            5 => {
                if phase < 0.5 {
                    lookup(3, 2. * phase)? * 0.5
                } else {
                    0.5 + (1. - lookup(3, 1. - 2. * (phase - 0.5))?) * 0.5
                }
            }
            3 => {
                if phase < 0.5 {
                    (1. - lookup(3, 1. - 2. * phase)?) * 0.5
                } else {
                    0.5 + lookup(3, 2. * (phase - 0.5))? * 0.5
                }
            }
            _ => 0., // Native jump-table default, not a linear fallback.
        };
        Ok(start + shape * (end - start))
    }
    pub fn scale_conversion(&self, value: f32, scaling: u8) -> Result<f32> {
        match scaling {
            1 => self.slots[5].sample(value, false),
            2 => self.slots[6].sample(value, true),
            _ => Ok(value), // Native scaling switch leaves other values unchanged.
        }
    }
}
#[derive(Clone, Debug)]
pub struct Transition {
    start: f32,
    end: f32,
    current_db: f32,
    start_tick: f32,
    duration: f32,
    curve: u32,
    sampled: bool,
    linear:bool,
}
fn orient(curve: u8, start: f32, end: f32) -> u32 {
    if start <= end || matches!(curve, 3 | 5) {
        curve as u32
    } else {
        8u32.wrapping_sub(curve as u32)
    }
}
impl Transition {
    pub fn new(
        tables: &Tables,
        start_db: f32,
        end_db: f32,
        curve: u8,
        duration: i32,
        tick: u32,
    ) -> Result<Self> {
        let start = tables.db_to_linear(start_db)?;
        let end = tables.db_to_linear(end_db)?;
        Ok(Self {
            start,
            end,
            current_db: start,
            start_tick: tick as f32,
            duration: ((duration as u32).wrapping_add(2) / 3) as f32,
            curve: orient(curve, start, end),
            sampled: false,
            linear:false,
        })
    }
    pub fn new_linear(start:f32,end:f32,curve:u8,duration:i32,tick:u32)->Self {
        Self{start,end,current_db:start,start_tick:tick as f32,duration:((duration as u32).wrapping_add(2)/3) as f32,curve:orient(curve,start,end),sampled:false,linear:true}
    }
    pub fn sample(&mut self, tables: &Tables, tick: u32) -> Result<(f32, bool)> {
        let phase = if self.duration == 0. {
            1.
        } else {
            ((tick as f32 - self.start_tick) / self.duration).max(0.)
        };
        let complete = phase >= 1.;
        let value = if complete {
            self.end
        } else {
            tables.interpolate(phase, self.start, self.end, self.curve)?
        };
        let value = if self.linear {value}else {tables.linear_to_db(value)?};
        self.current_db = value;
        self.sampled = true;
        Ok((value, complete))
    }
    pub fn retarget(
        &mut self,
        tables: &Tables,
        end_db: f32,
        curve: u8,
        duration: i32,
        tick: u32,
    ) -> Result<()> {
        self.retarget_relative(tables,end_db,curve,duration,tick,false)
    }
    pub fn retarget_relative(&mut self,tables:&Tables,end:f32,curve:u8,duration:i32,tick:u32,relative:bool)->Result<()> {
        // Mute transitions reuse the last emitted float, not its byte-quantized
        // SIS copy. Unvisited transitions retain their initial converted start.
        if self.sampled {
            self.start = if self.linear {self.current_db}else {tables.db_to_linear(self.current_db)?};
        }
        let value=if self.linear {end}else {tables.db_to_linear(end)?};
        self.end=if relative {if self.linear {self.end+value}else {self.end*value}}else {value};
        self.curve = orient(curve, self.start, self.end);
        self.duration = ((duration as u32).wrapping_add(2) / 3) as f32;
        self.start_tick = tick as f32;
        Ok(())
    }
}
#[derive(Debug)]
pub struct Mute {
    pub byte: u8,
    /// Native callback retains the untruncated converted value for live voices;
    /// newly allocated voices instead read the stored byte.
    pub callback_value: f32,
    pub transition: Option<Transition>,
}
impl Default for Mute {
    fn default() -> Self {
        Self {
            byte: 255,
            callback_value: 255.,
            transition: None,
        }
    }
}
impl Mute {
    pub fn db(&self) -> f32 {
        (self.byte as i32 - 255) as f32 * DB_RANGE / 255.
    }
    pub fn apply_db(&mut self, value: f32) -> Result<()> {
        ensure!(value.is_finite(), "Non-finite mute callback");
        let converted = (value - (-DB_RANGE)) * 255. / DB_RANGE;
        ensure!(
            converted >= i32::MIN as f32 && converted < (i32::MAX as f64 + 1.) as f32,
            "Mute conversion outside fctiwz range"
        );
        self.byte = converted.trunc() as i32 as u8;
        self.callback_value = converted;
        Ok(())
    }
    pub fn request(
        &mut self,
        tables: &Tables,
        muting: bool,
        curve: u8,
        duration: i32,
        tick: u32,
    ) -> Result<()> {
        let end = if muting { -DB_RANGE } else { 0. };
        if let Some(transition) = &mut self.transition {
            transition.retarget(tables, end, curve, duration, tick)?;
        } else if duration == 0 {
            self.apply_db(end)?;
        } else {
            self.transition = Some(Transition::new(
                tables,
                self.db(),
                end,
                curve,
                duration,
                tick,
            )?);
        }
        Ok(())
    }
    pub fn update(&mut self, tables: &Tables, tick: u32) -> Result<()> {
        if let Some(transition) = &mut self.transition {
            let (value, complete) = transition.sample(tables, tick)?;
            self.apply_db(value)?;
            if complete {
                self.transition = None;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gain_matches_original_lookup_instructions() -> Result<()> {
        let tables = Tables::retail()?;
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/audio_gain.json"
        ))?;
        for case in reference["cases"].as_array().unwrap() {
            let input = f32::from_bits(case["input_bits"].as_u64().unwrap() as u32);
            assert_eq!(
                tables.db_to_linear(input)?.to_bits(),
                case["output_bits"].as_u64().unwrap() as u32,
                "gain {input}"
            );
        }
        assert!(tables.db_to_linear(f32::NAN).is_err());
        Ok(())
    }
    #[test]
    fn captured_native_tables_drive_curves_quantization_and_retargeting() -> Result<()> {
        let tables = Tables::retail()?;
        let mut mute = Mute::default();
        assert_eq!(mute.db(), 0.);
        mute.request(&tables, true, 4, 500, 100)?;
        mute.update(&tables, 100)?;
        assert_eq!(mute.byte, 254);
        mute.update(&tables, 180)?;
        assert!(mute.byte > 0 && mute.byte < 255);
        let previous = mute.byte;
        mute.request(&tables, false, 7, 500, 180)?;
        mute.update(&tables, 180)?;
        assert_eq!(mute.byte, previous);
        mute.update(&tables, 347)?;
        assert_eq!(mute.byte, 254);
        assert!(mute.transition.is_none());
        mute.request(&tables, true, 4, 0, 400)?;
        assert_eq!(mute.byte, 0);
        mute.request(&tables, false, 4, 0, 400)?;
        assert_eq!(mute.byte, 254);
        for curve in 0..9 {
            let mut transition = Transition::new(&tables, -DB_RANGE, 0., curve, 500, 0)?;
            for tick in 0..=167 {
                let (db, complete) = transition.sample(&tables, tick)?;
                assert!(db.is_finite());
                assert_eq!(complete, tick == 167);
            }
        }
        Ok(())
    }
}
