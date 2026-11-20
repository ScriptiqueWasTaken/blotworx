//! Typed payloads sharing the native deferred entity-event queue.
//! Concrete status receivers and reference ownership are still required.
use super::action_event_queue::{ActionEvent, QueuedEvent};
use anyhow::Result;
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct CounterStatus {
    pub source: Option<usize>,
    pub header: Option<usize>,
    pub previous: i32,
    pub current: i32,
    /// Native word18 participates in equality, even when its constructor
    /// initializes it to zero. Keep it distinct from the reference at1c.
    pub auxiliary: u32,
    pub reference: Option<usize>,
}
impl CounterStatus {
    /// REL12a250: referenced-object virtual10 is evaluated BEFORE the base
    /// source/header/status comparisons. Pointer equality cannot replace it.
    fn equivalent_with(
        &self,
        other: &Self,
        compare: &mut dyn FnMut(usize, usize) -> Result<bool>,
    ) -> Result<bool> {
        let reference = match (self.reference, other.reference) {
            (None, None) => true,
            (Some(left), Some(right)) => compare(left, right)?,
            _ => false,
        };
        Ok(self.source == other.source
            && self.header == other.header
            && self.previous == other.previous
            && self.current == other.current
            && self.auxiliary == other.auxiliary
            && reference)
    }
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "payload")]
pub enum Event {
    Action(ActionEvent),
    CounterStatus(CounterStatus),
    Damage(Damage),
    ZeroHealth(ZeroHealth),
    Basic(Basic),
    Animation(Animation),
    Variable(Variable),
    BehaviorFinished(BehaviorFinished),
}
/// REL1990d4/2b8040: kind82 carries an interned behavior name. Equality
/// compares source/header and name identity (equal interned text), not time.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BehaviorFinished {
    pub source: Option<usize>,
    pub header: Option<usize>,
    pub name: String,
}
/// Native kind0b, vtable4f57f0. Type0 is a byte at18, type1 an int32,
/// type2 a floating word. Its332d00 equality uses numeric float comparison.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Variable {
    pub source: Option<usize>,
    pub header: Option<usize>,
    pub value_type: u32,
    pub index: i32,
    pub bits: u32,
}
impl Variable {
    fn equivalent(&self, other: &Self) -> bool {
        if self.source != other.source
            || self.header != other.header
            || self.value_type != other.value_type
            || self.index != other.index
        {
            return false;
        }
        match self.value_type {
            0 => self.bits >> 24 == other.bits >> 24,
            1 => self.bits == other.bits,
            2 => f32::from_bits(self.bits) == f32::from_bits(other.bits),
            _ => false,
        }
    }
}
impl From<Variable> for Event {
    fn from(value: Variable) -> Self {
        Self::Variable(value)
    }
}
/// Ordinary controller export2f1320. Byte14 distinguishes exported feedback
/// from explicit injected events; equality332c20 ignores that byte.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Animation {
    pub source: Option<usize>,
    pub header: Option<usize>,
    pub id: u32,
    pub flag: u8,
}
impl From<Animation> for Event {
    fn from(value: Animation) -> Self {
        Self::Animation(value)
    }
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Basic {
    pub event_kind: u16,
    pub source: Option<usize>,
    pub header: Option<usize>,
}
impl From<Basic> for Event {
    fn from(value: Basic) -> Self {
        Self::Basic(value)
    }
}
/// Event10 emitted by counter3641b8. Equality333a48 compares the base fields
/// before virtual10 on its required reference; magnitude/displacement are not
/// equality fields. Keep float lanes as raw bits for reproducible evidence.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Damage {
    pub source: Option<usize>,
    pub header: Option<usize>,
    pub amount_bits: u32,
    pub displacement_bits: u32,
    pub reference: usize,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ZeroHealth {
    pub source: Option<usize>,
    pub header: Option<usize>,
}
impl From<ActionEvent> for Event {
    fn from(value: ActionEvent) -> Self {
        Self::Action(value)
    }
}
impl From<CounterStatus> for Event {
    fn from(value: CounterStatus) -> Self {
        Self::CounterStatus(value)
    }
}
impl From<Damage> for Event {
    fn from(value: Damage) -> Self {
        Self::Damage(value)
    }
}
impl From<ZeroHealth> for Event {
    fn from(value: ZeroHealth) -> Self {
        Self::ZeroHealth(value)
    }
}
impl Event {
    pub fn action(&self) -> Option<&ActionEvent> {
        match self {
            Self::Action(event) => Some(event),
            _ => None,
        }
    }
}
impl QueuedEvent for Event {
    fn kind(&self) -> u16 {
        match self {
            Self::Action(_) => 0x0c,
            Self::CounterStatus(_) => 0x75,
            Self::Damage(_) => 0x10,
            Self::ZeroHealth(_) => 0x35,
            Self::Basic(event) => event.event_kind,
            Self::Animation(_) => 9,
            Self::Variable(_) => 0xb,
            Self::BehaviorFinished(_) => 0x82,
        }
    }
    fn equivalent_with(
        &self,
        other: &Self,
        compare: &mut dyn FnMut(usize, usize) -> Result<bool>,
    ) -> Result<bool> {
        match (self, other) {
            (Self::Action(left), Self::Action(right)) => left.equivalent_with(right, compare),
            (Self::CounterStatus(left), Self::CounterStatus(right)) => {
                left.equivalent_with(right, compare)
            }
            (Self::Damage(left), Self::Damage(right)) => {
                if left.source != right.source || left.header != right.header {
                    return Ok(false);
                }
                compare(left.reference, right.reference)
            }
            (Self::ZeroHealth(left), Self::ZeroHealth(right)) => Ok(left == right),
            (Self::Basic(left), Self::Basic(right)) => Ok(left == right),
            (Self::Animation(left), Self::Animation(right)) => Ok(left.id == right.id
                && left.source == right.source
                && left.header == right.header),
            (Self::Variable(left), Self::Variable(right)) => Ok(left.equivalent(right)),
            (Self::BehaviorFinished(left), Self::BehaviorFinished(right)) => Ok(left == right),
            _ => Ok(false),
        }
    }
}
pub type Queue = super::action_event_queue::Queue<Event>;

#[cfg(test)]
mod tests {
    use super::super::action_event_queue::{Admission, Entry, Host};
    use super::*;
    #[test]
    fn behavior_finished_event_matches_original_equality_and_retains_queued_name()->Result<()> {
        let fixture:serde_json::Value=serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/behavior_finished.json"))?;
        let cases=fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(),160);
        for case in cases {
            let left=Event::BehaviorFinished(serde_json::from_value(case["left"].clone())?);
            let right=Event::BehaviorFinished(serde_json::from_value(case["right"].clone())?);
            assert_eq!(left.kind(),0x82);
            assert_eq!(left.equivalent_with(&right,&mut |_,_|anyhow::bail!("No referenced-object comparison"))?,
                case["equal"].as_bool().unwrap(),"{case}");
        }
        let mut event=Event::BehaviorFinished(BehaviorFinished{source:Some(0),header:None,name:"ScriptedIdle".into()});
        let mut queue=Queue::default();queue.enqueue(&event,None)?;
        if let Event::BehaviorFinished(value)=&mut event {value.name.clear();}
        let Event::BehaviorFinished(retained)=&queue.entries()[0].event else {panic!("Wrong event payload")};
        assert_eq!(retained.name,"ScriptedIdle");
        assert_eq!(super::super::action_event_dispatch::DispatchEvent::source(&queue.entries()[0].event),Some(0));
        Ok(())
    }
    #[test]
    fn variable_feedback_equality_uses_native_type_and_float_comparison() -> Result<()> {
        let mut left = Variable {
            source: Some(3),
            header: None,
            value_type: 2,
            index: 24,
            bits: 0,
        };
        let mut right = left.clone();
        right.bits = (-0f32).to_bits();
        let compare = |a: &Variable, b: &Variable| {
            Event::from(a.clone()).equivalent_with(&b.clone().into(), &mut |_, _| {
                anyhow::bail!("No references")
            })
        };
        assert!(compare(&left, &right)?);
        right.bits = f32::NAN.to_bits();
        assert!(!compare(&right, &right)?);
        left.value_type = 1;
        right.value_type = 1;
        left.bits = 0x80000000;
        right.bits = 0;
        assert!(!compare(&left, &right)?);
        right.bits = left.bits;
        assert!(compare(&left, &right)?);
        right.index = -1;
        assert!(!compare(&left, &right)?);
        right.index = 24;
        right.source = Some(4);
        assert!(!compare(&left, &right)?);
        left.value_type = 0;
        right = left.clone();
        left.bits = 0x01000000;
        right.bits = 0x01ffffff;
        assert!(compare(&left, &right)?);
        left.value_type = 3;
        right = left.clone();
        assert!(!compare(&left, &right)?);
        assert_eq!(Event::from(left).kind(), 0xb);
        Ok(())
    }
    #[test]
    fn animation_equality_ignores_export_flag_and_preserves_identity() -> Result<()> {
        let a = Animation {
            source: Some(3),
            header: None,
            id: 85,
            flag: 1,
        };
        let mut b = a.clone();
        b.flag = 0;
        assert!(Event::from(a.clone())
            .equivalent_with(&b.clone().into(), &mut |_, _| anyhow::bail!(
                "No references"
            ))?);
        b.id = 25;
        assert!(!Event::from(a.clone()).equivalent_with(&b.clone().into(), &mut |_, _| Ok(true))?);
        b.id = 85;
        b.header = Some(4);
        assert!(!Event::from(a).equivalent_with(&b.into(), &mut |_, _| Ok(true))?);
        Ok(())
    }
    #[test]
    fn damage_checks_base_before_reference_and_ignores_magnitude_lanes() -> Result<()> {
        let a = Damage {
            source: Some(3),
            header: Some(4),
            amount_bits: 1f32.to_bits(),
            displacement_bits: 2f32.to_bits(),
            reference: 50,
        };
        let mut b = a.clone();
        b.source = Some(5);
        assert!(!Event::from(a.clone())
            .equivalent_with(&b.clone().into(), &mut |_, _| anyhow::bail!(
                "Base mismatch must skip reference"
            ))?);
        b.source = a.source;
        b.amount_bits = f32::NAN.to_bits();
        b.displacement_bits = 99f32.to_bits();
        b.reference = 51;
        let mut compared = vec![];
        assert!(
            Event::from(a).equivalent_with(&b.into(), &mut |left, right| {
                compared.push((left, right));
                Ok(true)
            })?
        );
        assert_eq!(compared, [(50, 51)]);
        Ok(())
    }
    fn action(value: u32) -> Event {
        ActionEvent {
            source: Some(3),
            header: None,
            action: value,
            vector_bits: [0; 4],
            reference: None,
        }
        .into()
    }
    fn status(current: i32) -> Event {
        CounterStatus {
            source: Some(3),
            header: Some(5),
            previous: 0,
            current,
            auxiliary: 0,
            reference: None,
        }
        .into()
    }
    #[test]
    fn mixed_queue_preserves_order_and_same_pass_appends_across_kinds() -> Result<()> {
        struct Receiver {
            seen: Vec<u16>,
            queries: Vec<(usize, u16)>,
        }
        impl Host<Event> for Receiver {
            fn listener_registered(&mut self, key: usize, kind: u16) -> Result<bool> {
                self.queries.push((key, kind));
                Ok(true)
            }
            fn deliver(&mut self, queue: &mut Queue, entry: &Entry<Event>) -> Result<()> {
                self.seen.push(entry.event.kind());
                if entry.event == status(1) {
                    assert_eq!(
                        queue.enqueue(&status(1), Some(900))?,
                        Admission::DuplicateWhileProcessing
                    );
                    assert_eq!(queue.enqueue(&status(2), Some(901))?, Admission::Queued);
                }
                Ok(())
            }
        }
        let mut queue = Queue::default();
        queue.enqueue(&action(13), None)?;
        queue.enqueue(
            &Basic {
                event_kind: 0x18,
                source: Some(3),
                header: Some(10),
            }
            .into(),
            None,
        )?;
        queue.enqueue(&status(1), Some(9))?;
        queue.enqueue(&action(14), None)?;
        let mut receiver = Receiver {
            seen: vec![],
            queries: vec![],
        };
        queue.process(&mut receiver)?;
        assert_eq!(receiver.seen, [0x0c, 0x18, 0x75, 0x0c, 0x75]);
        assert_eq!(receiver.queries, [(9, 0x75), (901, 0x75)]);
        assert_eq!(queue.pending(), 0);
        Ok(())
    }
    #[test]
    fn reference_equivalence_is_mandatory_and_runs_before_other_fields() -> Result<()> {
        let left = CounterStatus {
            source: Some(1),
            header: None,
            previous: 0,
            current: 1,
            auxiliary: 0,
            reference: Some(10),
        };
        let mut right = left.clone();
        right.source = Some(99);
        right.reference = Some(20);
        let mut comparisons = vec![];
        assert!(!left.equivalent_with(&right, &mut |a, b| {
            comparisons.push((a, b));
            Ok(true)
        })?);
        assert_eq!(comparisons, [(10, 20)]);
        right.source = left.source;
        assert!(left.equivalent_with(&right, &mut |_, _| Ok(true))?);
        assert!(!left.equivalent_with(&right, &mut |_, _| Ok(false))?);
        right.auxiliary = 1;
        assert!(!left.equivalent_with(&right, &mut |_, _| Ok(true))?);
        right.reference = None;
        assert!(!left.equivalent_with(&right, &mut |_, _| anyhow::bail!(
            "Null mismatch must not call the comparator"
        ))?);
        Ok(())
    }
    #[test]
    fn unsupported_status_receiver_retains_failed_entry_and_action_tail() -> Result<()> {
        struct Receiver {
            unsupported: bool,
            seen: Vec<u16>,
        }
        impl Host<Event> for Receiver {
            fn listener_registered(&mut self, _: usize, _: u16) -> Result<bool> {
                Ok(true)
            }
            fn deliver(&mut self, _: &mut Queue, entry: &Entry<Event>) -> Result<()> {
                if self.unsupported && matches!(entry.event, Event::CounterStatus(_)) {
                    anyhow::bail!("Counter receiver missing");
                }
                self.seen.push(entry.event.kind());
                Ok(())
            }
        }
        let mut queue = Queue::default();
        queue.enqueue(&action(13), None)?;
        queue.enqueue(&status(1), None)?;
        queue.enqueue(&action(14), None)?;
        let mut receiver = Receiver {
            unsupported: true,
            seen: vec![],
        };
        assert!(queue.process(&mut receiver).is_err());
        assert_eq!(queue.pending(), 2);
        assert_eq!(receiver.seen, [0x0c]);
        receiver.unsupported = false;
        queue.process(&mut receiver)?;
        assert_eq!(receiver.seen, [0x0c, 0x75, 0x0c]);
        Ok(())
    }
    #[test]
    fn processing_requires_reference_comparator_without_losing_the_entry() -> Result<()> {
        struct Receiver;
        impl Host<Event> for Receiver {
            fn listener_registered(&mut self, _: usize, _: u16) -> Result<bool> {
                Ok(true)
            }
            fn deliver(&mut self, queue: &mut Queue, entry: &Entry<Event>) -> Result<()> {
                let Event::CounterStatus(left) = &entry.event else {
                    anyhow::bail!("Expected status")
                };
                let mut right = left.clone();
                right.reference = Some(20);
                assert!(queue.enqueue(&right.clone().into(), None).is_err());
                assert_eq!(queue.pending(), 1);
                assert_eq!(
                    queue.enqueue_with(&right.into(), None, &mut |a, b| {
                        assert_eq!((a, b), (10, 20));
                        Ok(true)
                    })?,
                    Admission::DuplicateWhileProcessing
                );
                Ok(())
            }
        }
        let mut queue = Queue::default();
        let Event::CounterStatus(mut event) = status(1) else {
            unreachable!()
        };
        event.reference = Some(10);
        queue.enqueue(&event.into(), None)?;
        queue.process(&mut Receiver)?;
        assert_eq!(queue.pending(), 0);
        Ok(())
    }
}
