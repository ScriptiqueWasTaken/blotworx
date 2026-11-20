//! InventoryComponent command dispatch, REL3436d8..343864 /3435a8.
use anyhow::Result;

/// Registry2c108 +20,+24,+28,+2c, confirmed by registration3427b8..3429f8.
pub const POWERUP_NAMES: [&str; 4] = ["BigBall", "SuperSaturated", "ThinnerShockwave", "LongRange"];
/// Registry2c17c binds literal None (registration341fa8..341fd0).
pub const NO_POWERUP: &str = "None";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SketchArray {
    Names,
    Counts,
    Maxima,
}
pub trait NewSketch {
    fn count(&mut self) -> Result<u32>;
    /// Native +268 resize, with the constructor/notification flag false.
    fn resize(&mut self, array: SketchArray, count: u32) -> Result<()>;
    /// Original variable SketchDefaultMax, default string 2.0, bounds +/-FLT_MAX.
    fn default_maximum(&mut self) -> Result<f32>;
    fn set_name(&mut self, index: u32, item: &str) -> Result<()>;
    fn set_count(&mut self, index: u32, count: i32) -> Result<()>;
    fn set_maximum(&mut self, index: u32, maximum: f32) -> Result<()>;
}
#[derive(Debug, PartialEq)]
pub struct SketchAdded {
    pub index: u32,
    pub count: i32,
    pub maximum: f32,
}
/// REL343a80..343c14, mutation branch after lookup reports no existing entry.
/// This does not include the later inventory events/message/UI phase.
pub fn add_new_sketch(
    item: &str,
    amount: f32,
    backend: &mut impl NewSketch,
) -> Result<SketchAdded> {
    let index = backend.count()?;
    let count = index.wrapping_add(1);
    backend.resize(SketchArray::Names, count)?;
    backend.resize(SketchArray::Counts, count)?;
    backend.resize(SketchArray::Maxima, count)?;
    let maximum = backend.default_maximum()?;
    let count = checked_fctiwz(amount)?.min(checked_fctiwz(maximum)?);
    backend.set_name(index, item)?;
    backend.set_count(index, count)?;
    backend.set_maximum(index, maximum)?;
    Ok(SketchAdded {
        index,
        count,
        maximum,
    })
}
pub(super) fn checked_fctiwz(value: f32) -> Result<i32> {
    anyhow::ensure!(
        value.is_finite() && (-2147483648.0..2147483648.0).contains(&(value as f64)),
        "Unverified Wii inventory integer conversion for {value}"
    );
    Ok(value as i32)
}
pub trait ExistingSketch {
    fn count(&mut self, index: u32) -> Result<i32>;
    fn maximum(&mut self, index: u32) -> Result<f32>;
    fn set_count(&mut self, index: u32, count: i32) -> Result<()>;
}
/// REL343c1c..343d68. Returns previous/next only after a successful setter.
/// Acceptance uses current<maximum before considering the requested amount.
pub fn add_existing_sketch(
    index: u32,
    amount: f32,
    backend: &mut impl ExistingSketch,
) -> Result<Option<(f32, f32)>> {
    let previous = backend.count(index)? as f32;
    let maximum = backend.maximum(index)?;
    if !(previous < maximum) {
        return Ok(None);
    }
    // Native converts the maximum first, then the single-precision sum.
    let maximum_count = checked_fctiwz(maximum)?;
    let next = checked_fctiwz(previous + amount)?.min(maximum_count);
    backend.set_count(index, next)?;
    Ok(Some((previous, next as f32)))
}

/// Original property access must preserve count, validity and interned identity.
pub trait SketchNames {
    fn count(&mut self) -> Result<u32>;
    /// Getter+288 returns a low-byte validity flag and an interned string handle.
    fn name(&mut self, index: u32) -> Result<(u32, usize)>;
}
/// REL343868..343984: first valid identical handle; never deduplicate arrays.
pub fn sketch_index(requested: usize, names: &mut impl SketchNames) -> Result<Option<u32>> {
    let count = names.count()?;
    for index in 0..count {
        let (valid, handle) = names.name(index)?;
        if valid as u8 != 0 && handle == requested {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

pub trait Properties: SketchNames {
    /// Original locale-aware prefix comparison, DOL80503890 ->80011838.
    fn is_sketch_family(&mut self, item: &str) -> Result<bool>;
    fn sketch_maximum(&mut self, index: u32) -> Result<f32>;
    fn set_sketch_count(&mut self, index: u32, value: i32) -> Result<()>;
    /// Appends literal Max and uses getter+270's low-byte validity.
    fn maximum(&mut self, item: &str) -> Result<Option<f32>>;
    fn is_eticket(&mut self, handle: usize) -> Result<bool>;
    /// Native ETicket path reads the previous value before the setter.
    fn read_previous_eticket(&mut self, item: &str) -> Result<()>;
    fn set_float(&mut self, item: &str, value: f32) -> Result<()>;
}
/// REL344868..344ad0. Notifications belong to property setters, not this helper.
pub fn set_property(
    item: &str,
    handle: usize,
    requested: f32,
    backend: &mut impl Properties,
) -> Result<()> {
    if backend.is_sketch_family(item)? {
        if let Some(index) = sketch_index(handle, backend)? {
            let maximum = backend.sketch_maximum(index)?;
            let value = if requested > maximum {
                maximum
            } else {
                requested
            };
            // Preserve fctiwz for validated finite range; unsupported conversion
            // must not silently adopt Rust's saturating float-to-int conversion.
            backend.set_sketch_count(index, checked_fctiwz(value)?)?;
        }
    } else {
        let maximum = backend.maximum(item)?;
        let value = if maximum.is_some_and(|m| requested > m) {
            maximum.unwrap()
        } else if requested < 0. {
            0.
        } else {
            requested
        };
        if backend.is_eticket(handle)? {
            backend.read_previous_eticket(item)?;
        }
        backend.set_float(item, value)?;
    }
    Ok(())
}

pub trait AddPreview: SketchNames {
    fn is_sketch_family(&mut self, item: &str) -> Result<bool>;
    fn sketch_count(&mut self, index: u32) -> Result<i32>;
    /// Native getter destination starts at zero; validity is ignored here.
    fn sketch_maximum(&mut self, index: u32) -> Result<f32>;
    fn is_registered_powerup(&mut self, handle: usize) -> Result<bool>;
    fn powerup_values(&mut self) -> Result<[f32; 4]>;
    /// Original locale-aware strcmp, including literal None selected name.
    fn matches_selected(&mut self, item: &str, selected: &str) -> Result<bool>;
    fn scalar_value(&mut self, item: &str) -> Result<f32>;
    /// Getter destination starts at zero; validity is ignored by Add.
    fn scalar_maximum(&mut self, item: &str) -> Result<f32>;
}
/// REL3439c8 with preview flag=1. No mutations or messages in this path.
pub fn preview_add(
    item: &str,
    handle: usize,
    amount: f32,
    backend: &mut impl AddPreview,
) -> Result<bool> {
    if backend.is_sketch_family(item)? {
        let Some(index) = sketch_index(handle, backend)? else {
            return Ok(true);
        };
        let current = backend.sketch_count(index)? as f32;
        let maximum = backend.sketch_maximum(index)?;
        return Ok(current < maximum);
    }
    if backend.is_registered_powerup(handle)? {
        let values = backend.powerup_values()?;
        let selected = selected_powerup(values).map_or(NO_POWERUP, |i| POWERUP_NAMES[i]);
        if backend.matches_selected(item, selected)? {
            return Ok(false);
        }
        // Native preview also performs a second selected-powerup query and
        // compares with literal None before skipping mutation-only branches.
        let values = backend.powerup_values()?;
        backend.matches_selected(
            NO_POWERUP,
            selected_powerup(values).map_or(NO_POWERUP, |i| POWERUP_NAMES[i]),
        )?;
    }
    let current = backend.scalar_value(item)?;
    let maximum = backend.scalar_maximum(item)?;
    let next = preview_scalar_balance(current, amount, maximum);
    Ok(next != current)
}
/// REL343f6c..343f98: no lower clamp; maximum applies only when positive.
pub fn preview_scalar_balance(current: f32, amount: f32, maximum: f32) -> f32 {
    let next = current + amount;
    if maximum > 0. && next > maximum {
        maximum
    } else {
        next
    }
}
pub trait AddEffects {
    /// Event0xac: item at+10, previous at+14, next at+18, owning recipient.
    fn item_changed(&mut self, item: &str, previous: f32, next: f32) -> Result<()>;
    /// REL31c5fc, owner, zero optional arguments, PickedUpSFX only when nonempty.
    fn picked_up_sfx(&mut self, name: &str) -> Result<()>;
    /// REL344120..34431c: Paint/Thinner maxed glow cooldown or ETicket event.
    /// This phase needs original name matching, global timers and UI dispatch.
    fn rejected_positive(&mut self, item: &str, next: f32) -> Result<()>;
    fn powerup_values(&mut self) -> Result<[f32; 4]>;
    fn update_powerup_ui(&mut self, selected: &str) -> Result<()>;
}
/// Common Add UI/event tail REL344034..344348; early powerup rejection bypasses it.
/// Called after storage/paint-event work, with the branch's original float fields.
pub fn finish_add(
    item: &str,
    amount: f32,
    previous: f32,
    next: f32,
    accepted: bool,
    preview: bool,
    message: &str,
    backend: &mut impl AddEffects,
) -> Result<bool> {
    if preview {
        return Ok(accepted);
    }
    if accepted {
        backend.item_changed(item, previous, next)?;
        let message = message.split('\0').next().unwrap_or("");
        if !message.is_empty() {
            backend.picked_up_sfx(message)?;
        }
    } else if amount > 0. {
        backend.rejected_positive(item, next)?;
    }
    let values = backend.powerup_values()?;
    backend.update_powerup_ui(selected_powerup(values).map_or(NO_POWERUP, |i| POWERUP_NAMES[i]))?;
    Ok(accepted)
}

pub trait Backend {
    /// DOL8001b528 followed by frsp. Native numeric parsing is required; do not
    /// substitute strict Rust parsing, which rejects accepted numeric prefixes.
    fn amount(&mut self, token: &str) -> Result<f32>;
    fn add(&mut self, item: &str, amount: f32, preview: bool, message: &mut String)
        -> Result<bool>;
    /// Original Remove branch does not forward the preview flag.
    fn remove(&mut self, item: &str, amount: f32) -> Result<()>;
}
/// Native removal hooks, REL344468..344650. Event/UI hooks are
/// required: a balance write alone must not pretend this operation completed.
pub trait RemovalBackend {
    fn value(&mut self, item: &str) -> Result<f32>;
    fn set_value(&mut self, item: &str, value: f32) -> Result<()>;
    fn is_paint(&mut self, item: &str) -> Result<bool>;
    /// Global 0x8011 event: next, previous, thinner=false.
    fn paint_changed(&mut self, previous: f32, next: f32) -> Result<()>;
    /// Global 0xac event: owner, item, previous, next; owner comes from backend.
    fn item_changed(&mut self, item: &str, previous: f32, next: f32) -> Result<()>;
    /// REL344d34 reads all four registered powerup properties before choosing.
    fn powerup_values(&mut self) -> Result<[f32; 4]>;
    /// REL20b938: UpdateVariable / POWERUP, selected registered name or None (UI movie invocation).
    fn update_powerup_ui(&mut self, selected: &str) -> Result<()>;
}
pub fn remove(item: &str, amount: f32, backend: &mut impl RemovalBackend) -> Result<()> {
    let previous = backend.value(item)?;
    let difference = previous - amount;
    // Preserve native ordered comparison, including signed zero and NaN:
    // NaN difference selects +0, unlike Rust's general min/max conveniences.
    let next = if 0. <= difference { difference } else { 0. };
    backend.set_value(item, next)?;
    if next != previous {
        if backend.is_paint(item)? {
            backend.paint_changed(previous, next)?;
        }
        backend.item_changed(item, previous, next)?;
    }
    let values = backend.powerup_values()?;
    backend.update_powerup_ui(
        selected_powerup(values).map_or(NO_POWERUP, |index| POWERUP_NAMES[index]),
    )
}
/// REL344d34..344e74: first registered value >= 1; NaN does not qualify.
pub fn selected_powerup(values: [f32; 4]) -> Option<usize> {
    values.into_iter().position(|value| value >= 1.)
}
pub fn dispatch(
    command: Option<&str>,
    preview: bool,
    message: &mut String,
    backend: &mut impl Backend,
) -> Result<bool> {
    let Some(command) = command else {
        return Ok(false);
    };
    let mut words = command
        .split('\0')
        .next()
        .unwrap_or("")
        .split(' ')
        .filter(|w| !w.is_empty());
    let Some(verb) = words.next() else {
        return Ok(false);
    };
    if verb != "Add" && verb != "Remove" {
        return Ok(false);
    }
    let (Some(item), Some(amount)) = (words.next(), words.next()) else {
        return Ok(false);
    };
    let amount = backend.amount(amount)?;
    if verb == "Add" {
        backend.add(item, amount, preview, message)
    } else {
        backend.remove(item, amount)?;
        Ok(true)
    }
}
/// Inventory event0x34 preview scans all commands and ORs results. Its caller
/// must resolve the payload pickup and InventoryComponent before this step.
pub fn preview(commands: &[String], backend: &mut impl Backend) -> Result<bool> {
    let mut accepted = false;
    for command in commands {
        let mut message = String::new();
        accepted |= dispatch(Some(command), true, &mut message, backend)?;
    }
    Ok(accepted)
}
/// Inventory event0x34, REL343400..343568, after payload/component resolution.
/// Executes all commands, each with a fresh copy of PickedUpSFX; then calls
/// native pickup clear38a150 once only if any command returned true.
pub fn collect(
    commands: &[String],
    picked_up_sfx: &str,
    backend: &mut impl Backend,
    mut clear: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut accepted = false;
    for command in commands {
        let mut sfx = picked_up_sfx.to_owned();
        accepted |= dispatch(Some(command), false, &mut sfx, backend)?;
    }
    if accepted {
        clear()?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn add_tail_preserves_notifications_preview_and_unconditional_ui_update() -> Result<()> {
        struct Effects(Vec<&'static str>);
        impl AddEffects for Effects {
            fn item_changed(&mut self, _: &str, old: f32, new: f32) -> Result<()> {
                assert_eq!((old, new), (1., 2.));
                self.0.push("event");
                Ok(())
            }
            fn picked_up_sfx(&mut self, text: &str) -> Result<()> {
                assert_eq!(text, "notice");
                self.0.push("message");
                Ok(())
            }
            fn rejected_positive(&mut self, _: &str, _: f32) -> Result<()> {
                self.0.push("rejected");
                Ok(())
            }
            fn powerup_values(&mut self) -> Result<[f32; 4]> {
                self.0.push("reads");
                Ok([0.; 4])
            }
            fn update_powerup_ui(&mut self, name: &str) -> Result<()> {
                assert_eq!(name, "None");
                self.0.push("persist");
                Ok(())
            }
        }
        let mut e = Effects(vec![]);
        assert!(finish_add(
            "Paint", 1., 1., 2., true, true, "notice", &mut e
        )?);
        assert!(e.0.is_empty());
        assert!(finish_add(
            "Paint",
            1.,
            1.,
            2.,
            true,
            false,
            "notice\0tail",
            &mut e
        )?);
        assert_eq!(e.0, vec!["event", "message", "reads", "persist"]);
        e.0.clear();
        assert!(!finish_add(
            "Paint", 1., 1., 2., false, false, "notice", &mut e
        )?);
        assert_eq!(e.0, vec!["rejected", "reads", "persist"]);
        e.0.clear();
        finish_add("Paint", -1., 1., 2., false, false, "", &mut e)?;
        assert_eq!(e.0, vec!["reads", "persist"]);
        Ok(())
    }
    #[test]
    fn existing_sketch_checks_capacity_before_amount_and_writes_even_unchanged() -> Result<()> {
        struct Store {
            count: i32,
            max: f32,
            writes: Vec<i32>,
        }
        impl ExistingSketch for Store {
            fn count(&mut self, _: u32) -> Result<i32> {
                Ok(self.count)
            }
            fn maximum(&mut self, _: u32) -> Result<f32> {
                Ok(self.max)
            }
            fn set_count(&mut self, _: u32, n: i32) -> Result<()> {
                self.count = n;
                self.writes.push(n);
                Ok(())
            }
        }
        let mut s = Store {
            count: 1,
            max: 3.9,
            writes: vec![],
        };
        assert_eq!(add_existing_sketch(0, 0.1, &mut s)?, Some((1., 1.)));
        assert_eq!(s.writes, vec![1]);
        assert_eq!(add_existing_sketch(0, 9., &mut s)?, Some((1., 3.)));
        s.max = 3.;
        assert_eq!(add_existing_sketch(0, -2., &mut s)?, None);
        assert_eq!(s.writes, vec![1, 3]);
        s.count = 1;
        s.max = 3.;
        assert_eq!(add_existing_sketch(0, -4.9, &mut s)?, Some((1., -3.)));
        s.max = f32::NAN;
        assert_eq!(add_existing_sketch(0, 1., &mut s)?, None);
        s.max = 3.;
        assert!(add_existing_sketch(0, f32::INFINITY, &mut s).is_err());
        Ok(())
    }
    #[test]
    fn new_sketch_resizes_then_initializes_in_original_order() -> Result<()> {
        struct Store {
            calls: Vec<String>,
        }
        impl NewSketch for Store {
            fn count(&mut self) -> Result<u32> {
                self.calls.push("count".into());
                Ok(2)
            }
            fn resize(&mut self, a: SketchArray, n: u32) -> Result<()> {
                self.calls.push(format!("resize {a:?} {n}"));
                Ok(())
            }
            fn default_maximum(&mut self) -> Result<f32> {
                self.calls.push("maximum".into());
                Ok(2.9)
            }
            fn set_name(&mut self, i: u32, item: &str) -> Result<()> {
                self.calls.push(format!("name {i} {item}"));
                Ok(())
            }
            fn set_count(&mut self, i: u32, n: i32) -> Result<()> {
                self.calls.push(format!("count {i} {n}"));
                Ok(())
            }
            fn set_maximum(&mut self, i: u32, n: f32) -> Result<()> {
                self.calls.push(format!("max {i} {n}"));
                Ok(())
            }
        }
        let mut store = Store { calls: vec![] };
        assert_eq!(
            add_new_sketch("SketchAnvil", 9.8, &mut store)?,
            SketchAdded {
                index: 2,
                count: 2,
                maximum: 2.9
            }
        );
        assert_eq!(
            store.calls,
            vec![
                "count",
                "resize Names 3",
                "resize Counts 3",
                "resize Maxima 3",
                "maximum",
                "name 2 SketchAnvil",
                "count 2 2",
                "max 2 2.9"
            ]
        );
        assert_eq!(add_new_sketch("SketchAnvil", -1.9, &mut store)?.count, -1);
        store.calls.clear();
        assert!(add_new_sketch("SketchAnvil", f32::NAN, &mut store).is_err());
        assert_eq!(
            store.calls,
            vec![
                "count",
                "resize Names 3",
                "resize Counts 3",
                "resize Maxima 3",
                "maximum"
            ]
        );
        Ok(())
    }
    #[test]
    fn add_preview_preserves_capacity_and_selected_powerup_rules() -> Result<()> {
        struct P {
            sketch: bool,
            registered: bool,
            current: f32,
            max: f32,
            selected: [f32; 4],
            reads: u32,
        }
        impl SketchNames for P {
            fn count(&mut self) -> Result<u32> {
                Ok(1)
            }
            fn name(&mut self, _: u32) -> Result<(u32, usize)> {
                Ok((1, 7))
            }
        }
        impl AddPreview for P {
            fn is_sketch_family(&mut self, _: &str) -> Result<bool> {
                Ok(self.sketch)
            }
            fn sketch_count(&mut self, _: u32) -> Result<i32> {
                Ok(self.current as i32)
            }
            fn sketch_maximum(&mut self, _: u32) -> Result<f32> {
                Ok(self.max)
            }
            fn is_registered_powerup(&mut self, _: usize) -> Result<bool> {
                Ok(self.registered)
            }
            fn powerup_values(&mut self) -> Result<[f32; 4]> {
                self.reads += 1;
                Ok(self.selected)
            }
            fn matches_selected(&mut self, item: &str, selected: &str) -> Result<bool> {
                Ok(item == selected)
            }
            fn scalar_value(&mut self, _: &str) -> Result<f32> {
                Ok(self.current)
            }
            fn scalar_maximum(&mut self, _: &str) -> Result<f32> {
                Ok(self.max)
            }
        }
        let mut p = P {
            sketch: true,
            registered: false,
            current: 1.,
            max: 3.,
            selected: [0.; 4],
            reads: 0,
        };
        assert!(preview_add("SketchAnvil", 8, 0., &mut p)?); // New sketch, no amount guard.
        assert!(preview_add("SketchAnvil", 7, -2., &mut p)?);
        p.current = 3.;
        assert!(!preview_add("SketchAnvil", 7, 1., &mut p)?);
        p.sketch = false;
        p.current = 5.;
        p.max = 5.;
        assert!(!preview_add("Paint", 7, 1., &mut p)?);
        assert!(preview_add("Paint", 7, -1., &mut p)?);
        p.max = 0.;
        assert!(preview_add("Paint", 7, 1., &mut p)?);
        p.registered = true;
        p.selected = [1., 0., 0., 0.];
        assert!(!preview_add("BigBall", 7, 1., &mut p)?);
        assert_eq!(p.reads, 1);
        assert!(preview_add("LongRange", 7, 1., &mut p)?);
        assert_eq!(p.reads, 3);
        assert!(preview_scalar_balance(2., f32::NAN, 5.).is_nan());
        assert_eq!(preview_scalar_balance(2., -5., 4.), -3.);
        Ok(())
    }
    #[test]
    fn property_setter_preserves_distinct_sketch_and_scalar_paths() -> Result<()> {
        struct P {
            sketch: bool,
            max: Option<f32>,
            count: Option<i32>,
            value: Option<f32>,
            calls: Vec<&'static str>,
        }
        impl SketchNames for P {
            fn count(&mut self) -> Result<u32> {
                Ok(1)
            }
            fn name(&mut self, _: u32) -> Result<(u32, usize)> {
                Ok((1, 7))
            }
        }
        impl Properties for P {
            fn is_sketch_family(&mut self, _: &str) -> Result<bool> {
                Ok(self.sketch)
            }
            fn sketch_maximum(&mut self, _: u32) -> Result<f32> {
                Ok(3.)
            }
            fn set_sketch_count(&mut self, _: u32, value: i32) -> Result<()> {
                self.count = Some(value);
                Ok(())
            }
            fn maximum(&mut self, _: &str) -> Result<Option<f32>> {
                Ok(self.max)
            }
            fn is_eticket(&mut self, _: usize) -> Result<bool> {
                Ok(true)
            }
            fn read_previous_eticket(&mut self, _: &str) -> Result<()> {
                self.calls.push("previous");
                Ok(())
            }
            fn set_float(&mut self, _: &str, value: f32) -> Result<()> {
                self.calls.push("set");
                self.value = Some(value);
                Ok(())
            }
        }
        let mut p = P {
            sketch: true,
            max: None,
            count: None,
            value: None,
            calls: vec![],
        };
        set_property("SketchAnvil", 7, 8., &mut p)?;
        assert_eq!(p.count, Some(3));
        set_property("SketchAnvil", 7, -2.9, &mut p)?;
        assert_eq!(p.count, Some(-2));
        p.count = None;
        set_property("SketchAbsent", 8, 1., &mut p)?;
        assert_eq!(p.count, None);
        assert!(set_property("SketchAnvil", 7, f32::NAN, &mut p).is_err());
        p.sketch = false;
        p.max = Some(-1.);
        set_property("ETicket", 7, 5., &mut p)?;
        assert_eq!(p.value, Some(-1.));
        assert_eq!(p.calls, vec!["previous", "set"]);
        set_property("ETicket", 7, f32::NAN, &mut p)?;
        assert!(p.value.unwrap().is_nan());
        p.max = None;
        set_property("ETicket", 7, -2., &mut p)?;
        assert_eq!(p.value, Some(0.));
        Ok(())
    }
    #[test]
    fn removal_setter_events_and_ui_follow_original_order() -> Result<()> {
        struct State {
            value: f32,
            calls: Vec<&'static str>,
        }
        impl RemovalBackend for State {
            fn value(&mut self, _: &str) -> Result<f32> {
                self.calls.push("get");
                Ok(self.value)
            }
            fn set_value(&mut self, _: &str, v: f32) -> Result<()> {
                self.calls.push("set");
                self.value = v;
                Ok(())
            }
            fn is_paint(&mut self, _: &str) -> Result<bool> {
                self.calls.push("paint?");
                Ok(true)
            }
            fn paint_changed(&mut self, _: f32, _: f32) -> Result<()> {
                self.calls.push("paint event");
                Ok(())
            }
            fn item_changed(&mut self, _: &str, _: f32, _: f32) -> Result<()> {
                self.calls.push("item event");
                Ok(())
            }
            fn powerup_values(&mut self) -> Result<[f32; 4]> {
                self.calls.push("powerup reads");
                Ok([0., 1., 2., 0.])
            }
            fn update_powerup_ui(&mut self, selected: &str) -> Result<()> {
                assert_eq!(selected, "SuperSaturated");
                self.calls.push("persist");
                Ok(())
            }
        }
        let mut s = State {
            value: 3.,
            calls: vec![],
        };
        remove("Paint", 5., &mut s)?;
        assert_eq!(s.value, 0.);
        assert_eq!(
            s.calls,
            vec![
                "get",
                "set",
                "paint?",
                "paint event",
                "item event",
                "powerup reads",
                "persist"
            ]
        );
        s.calls.clear();
        remove("Paint", 0., &mut s)?;
        assert_eq!(s.calls, vec!["get", "set", "powerup reads", "persist"]);
        s.calls.clear();
        remove("Paint", -2., &mut s)?;
        assert_eq!(s.value, 2.);
        s.value = -0.;
        remove("Paint", 0., &mut s)?;
        assert_eq!(s.value.to_bits(), (-0.0f32).to_bits());
        s.value = 1.;
        remove("Paint", f32::NAN, &mut s)?;
        assert_eq!(s.value.to_bits(), 0.0f32.to_bits());
        Ok(())
    }
    #[test]
    fn powerup_selection_keeps_priority_and_ordered_threshold() {
        assert_eq!(selected_powerup([1., 5., 8., 9.]), Some(0));
        assert_eq!(selected_powerup([f32::NAN, 0.999, 1., 2.]), Some(2));
        assert_eq!(selected_powerup([0., -1., f32::NAN, 0.999]), None);
        assert_eq!(selected_powerup([0., 0., 0., f32::INFINITY]), Some(3));
    }
    #[test]
    fn sketch_lookup_keeps_invalid_slots_duplicates_and_first_match() -> Result<()> {
        struct Names {
            calls: Vec<u32>,
        }
        impl SketchNames for Names {
            fn count(&mut self) -> Result<u32> {
                Ok(4)
            }
            fn name(&mut self, index: u32) -> Result<(u32, usize)> {
                self.calls.push(index);
                Ok([(0x100, 7), (1, 9), (0x101, 7), (1, 7)][index as usize])
            }
        }
        let mut names = Names { calls: vec![] };
        assert_eq!(sketch_index(7, &mut names)?, Some(2));
        assert_eq!(names.calls, vec![0, 1, 2]);
        names.calls.clear();
        assert_eq!(sketch_index(8, &mut names)?, None);
        assert_eq!(names.calls, vec![0, 1, 2, 3]);
        Ok(())
    }
    #[derive(Default)]
    struct Fixture {
        calls: Vec<String>,
    }
    #[test]
    fn collection_scans_all_commands_and_clears_once_only_after_success() -> Result<()> {
        let mut b = Fixture::default();
        let mut cleared = 0;
        collect(&[], "", &mut b, || {
            cleared += 1;
            Ok(())
        })?;
        assert_eq!(cleared, 0);
        collect(&["Give Brush All 1".into()], "", &mut b, || {
            cleared += 1;
            Ok(())
        })?;
        assert_eq!(cleared, 0);
        collect(
            &["Add Paint 2tail".into(), "Remove Ticket 3".into()],
            "",
            &mut b,
            || {
                cleared += 1;
                Ok(())
            },
        )?;
        assert_eq!(cleared, 1);
        assert_eq!(
            b.calls,
            vec![
                "number 2tail",
                "add Paint 2 false",
                "number 3",
                "remove Ticket 3"
            ]
        );
        Ok(())
    }
    impl Backend for Fixture {
        fn amount(&mut self, t: &str) -> Result<f32> {
            self.calls.push(format!("number {t}"));
            match t {
                "2tail" => Ok(2.),
                "3" => Ok(3.),
                _ => anyhow::bail!("Native numeric parsing unavailable"),
            }
        }
        fn add(&mut self, item: &str, n: f32, p: bool, message: &mut String) -> Result<bool> {
            assert!(message.is_empty());
            self.calls.push(format!("add {item} {n} {p}"));
            Ok(true)
        }
        fn remove(&mut self, item: &str, n: f32) -> Result<()> {
            self.calls.push(format!("remove {item} {n}"));
            Ok(())
        }
    }
    #[test]
    fn preview_runs_every_command_and_remove_keeps_native_side_effect() -> Result<()> {
        let mut b = Fixture::default();
        assert!(preview(
            &[
                "Add Paint 2tail extra".into(),
                "Give Brush All 1".into(),
                "Remove Ticket 3".into()
            ],
            &mut b
        )?);
        assert_eq!(
            b.calls,
            vec![
                "number 2tail",
                "add Paint 2 true",
                "number 3",
                "remove Ticket 3"
            ]
        );
        assert!(!preview(&[], &mut b)?);
        Ok(())
    }
    #[test]
    fn token_guards_do_not_call_unimplemented_operations() -> Result<()> {
        let mut b = Fixture::default();
        let mut message = String::new();
        for command in [
            None,
            Some(""),
            Some("add Paint 3"),
            Some("Add Paint"),
            Some("Add\tPaint 3"),
            Some("Add Paint\0 3"),
        ] {
            assert!(!dispatch(command, true, &mut message, &mut b)?);
        }
        assert!(b.calls.is_empty());
        assert!(dispatch(Some("Add Paint unknown"), true, &mut message, &mut b).is_err());
        Ok(())
    }
}
