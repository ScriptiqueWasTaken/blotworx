//! EM1 Apprentice format, verified against every unique retail BSQ in the catalog.
//! The two global databases share the extension but do not contain sequences.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Kind {
    String,
    U32,
    I32,
    Float,
    Short,
    SignedShort,
    Bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Count {
    U32,
    Short(u16),
    UntilEnd,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Node {
    Field {
        name: String,
        kind: Kind,
        value: Value,
        baseline: Value,
        raw: Vec<u8>,
    },
    Record {
        name: String,
        fields: Vec<Node>,
    },
    List {
        name: String,
        count: Count,
        items: Vec<Node>,
    },
}
impl Node {
    pub fn name(&self) -> &str {
        match self {
            Self::Field { name, .. } | Self::Record { name, .. } | Self::List { name, .. } => name,
        }
    }
    pub fn title(&self) -> String {
        if let Self::Record { name, fields } = self {
            let text = fields.iter().find_map(|n| match n {
                Self::Field {
                    name,
                    kind: Kind::String,
                    value,
                    ..
                } if matches!(name.as_str(), "Name" | "Label" | "Text key" | "Item") => {
                    value.as_str().filter(|s| !s.is_empty())
                }
                _ => None,
            });
            return text
                .map(|t| format!("{name}: {t}"))
                .unwrap_or_else(|| name.clone());
        }
        self.name().into()
    }
    pub fn value(&self, name: &str) -> Option<&Value> {
        match self {
            Self::Record { fields, .. } => fields.iter().find_map(|f| {
                if let Self::Field { name: n, value, .. } = f {
                    if n == name {
                        Some(value)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }),
            _ => None,
        }
    }
    pub fn strings(&self, out: &mut Vec<(String, String)>) {
        match self {
            Self::Field {
                name,
                kind: Kind::String,
                value,
                ..
            } => {
                if let Some(s) = value.as_str().filter(|s| !s.is_empty()) {
                    out.push((name.clone(), s.into()));
                }
            }
            Self::Record { fields, .. } => {
                for n in fields {
                    n.strings(out)
                }
            }
            Self::List { items, .. } => {
                for n in items {
                    n.strings(out)
                }
            }
            _ => {}
        }
    }
    fn write(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Field {
                kind,
                value,
                baseline,
                raw,
                ..
            } => {
                if value == baseline {
                    out.extend(raw);
                    return Ok(());
                }
                match kind {
                    Kind::String => {
                        let s = value.as_str().context("Expected text")?;
                        ensure!(!s.contains('\0'), "Text contains NUL");
                        let size = (s.len() + 3).div_ceil(4) * 4;
                        ensure!(size <= 65536, "Text exceeds BSQ limit");
                        out.push(size as u8);
                        out.push(if s.is_empty() { 0 } else { (s.len() + 1) as u8 });
                        out.extend(s.as_bytes());
                        out.push(0);
                        out.resize(out.len().div_ceil(4) * 4, 0);
                    }
                    Kind::Bool => out.extend(
                        if value.as_bool().context("Expected flag")? {
                            u32::MAX
                        } else {
                            0
                        }
                        .to_be_bytes(),
                    ),
                    Kind::Float => {
                        let v = value.as_f64().context("Expected number")? as f32;
                        ensure!(v.is_finite(), "Number must be finite");
                        out.extend(v.to_be_bytes());
                    }
                    Kind::U32 => out.extend(
                        u32::try_from(value.as_u64().context("Expected positive integer")?)?
                            .to_be_bytes(),
                    ),
                    Kind::I32 => out.extend(
                        i32::try_from(value.as_i64().context("Expected integer")?)?.to_be_bytes(),
                    ),
                    Kind::Short | Kind::SignedShort => {
                        let n = if *kind == Kind::Short {
                            u16::try_from(value.as_u64().context("Expected positive integer")?)?
                        } else {
                            i16::try_from(value.as_i64().context("Expected integer")?)? as u16
                        };
                        out.extend(n.to_be_bytes());
                        out.extend(raw.get(2..4).unwrap_or(&[0xcd, 0xcd]));
                    }
                }
            }
            Self::Record { fields, .. } => {
                for n in fields {
                    n.write(out)?;
                }
            }
            Self::List { count, items, .. } => {
                match count {
                    Count::U32 => out.extend(u32::try_from(items.len())?.to_be_bytes()),
                    Count::Short(pad) => {
                        out.extend(u16::try_from(items.len())?.to_be_bytes());
                        out.extend(pad.to_be_bytes());
                    }
                    Count::UntilEnd => {}
                };
                for n in items {
                    n.write(out)?;
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Document {
    pub root: Node,
    pub database: bool,
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    budget: usize,
    template: Option<u16>,
}
impl Reader<'_> {
    fn raw(&mut self, n: usize) -> Result<Vec<u8>> {
        let end = self.pos.checked_add(n).context("Offset overflow")?;
        let bytes = self
            .data
            .get(self.pos..end)
            .with_context(|| format!("Truncated BSQ at {:#x}", self.pos))?
            .to_vec();
        self.pos = end;
        Ok(bytes)
    }
    fn field(&mut self, name: &str, kind: Kind) -> Result<Node> {
        if let Some(tag) = self.template {
            let value = match kind {
                Kind::String => json!(""),
                Kind::Bool => json!(false),
                _ => json!(if name == "_type" { tag } else { 0 }),
            };
            return Ok(Node::Field {
                name: name.into(),
                kind,
                value,
                baseline: Value::Null,
                raw: vec![],
            });
        }

        ensure!(self.budget > 0, "BSQ node limit exceeded");
        self.budget -= 1;
        let start = self.pos;
        let value = match kind {
            Kind::String => {
                self.raw(2)?;
                let n = self.data[self.pos..]
                    .iter()
                    .position(|v| *v == 0)
                    .context("Unterminated BSQ string")?;
                ensure!(n <= 65533, "BSQ text limit exceeded");
                let text = String::from_utf8(self.raw(n)?)?;
                self.raw(1)?;
                let padding = (4 - self.pos % 4) % 4;
                self.raw(padding)?;
                json!(text)
            }
            _ => {
                let raw = self.raw(4)?;
                let v = u32::from_be_bytes(raw[..].try_into()?);
                match kind {
                    Kind::U32 => json!(v),
                    Kind::I32 => json!(v as i32),
                    Kind::Float => {
                        let f = f32::from_bits(v);
                        ensure!(f.is_finite(), "Invalid BSQ float");
                        json!(f)
                    }
                    // REL1f0118 reads the first byte and advances four bytes.
                    // The remaining bytes are padding, not part of the value.
                    // Keep all four raw bytes for unchanged-file preservation.
                    Kind::Bool => json!(raw[0] != 0),
                    Kind::Short | Kind::SignedShort => {
                        ensure!(
                            matches!(v as u16, 0xffff | 0xcdcd),
                            "Invalid short padding at {start:#x}"
                        );
                        if kind == Kind::Short {
                            json!(v >> 16)
                        } else {
                            json!((v >> 16) as i16)
                        }
                    }
                    _ => unreachable!(),
                }
            }
        };
        Ok(Node::Field {
            name: name.into(),
            kind,
            value: value.clone(),
            baseline: value,
            raw: self.data[start..self.pos].to_vec(),
        })
    }
    fn number(&mut self, short: bool) -> Result<(usize, Count)> {
        if self.template.is_some() {
            return Ok((
                0,
                if short {
                    Count::Short(0xcdcd)
                } else {
                    Count::U32
                },
            ));
        }
        let b = self.raw(4)?;
        let count = if short {
            u16::from_be_bytes(b[..2].try_into()?) as usize
        } else {
            u32::from_be_bytes(b[..].try_into()?) as usize
        };
        ensure!(
            count <= 100_000 && count <= self.data.len().saturating_sub(self.pos) / 4,
            "Invalid BSQ count at {:#x}",
            self.pos - 4
        );
        Ok((
            count,
            if short {
                Count::Short(u16::from_be_bytes(b[2..].try_into()?))
            } else {
                Count::U32
            },
        ))
    }
    fn list(
        &mut self,
        name: &str,
        short: bool,
        mut f: impl FnMut(&mut Self) -> Result<Node>,
    ) -> Result<Node> {
        let (n, count) = self.number(short)?;
        let mut items = vec![];
        for _ in 0..n {
            items.push(f(self)?)
        }
        Ok(Node::List {
            name: name.into(),
            count,
            items,
        })
    }
    fn fields(&mut self, spec: &[(&str, Kind)]) -> Result<Vec<Node>> {
        spec.iter().map(|(n, k)| self.field(n, k.clone())).collect()
    }
    fn expression(&mut self, depth: usize) -> Result<Node> {
        if self.template.is_some() {
            self.template = Some(0);
        }
        ensure!(depth < 64, "Condition nesting limit");
        use Kind::*;
        let tag = self.field("_type", Short)?;
        let n = if let Node::Field { value, .. } = &tag {
            value.as_u64().unwrap()
        } else {
            unreachable!()
        };
        let mut fields = vec![tag];
        let name = match n {
            0 => "All conditions",
            1 => "Any condition",
            2 => "State condition",
            3 => "Mood condition",
            4 => "Currency condition",
            5 => "Item condition",
            6 => "Quest condition",
            _ => bail!("Unknown EM1 condition {n} at {:#x}", self.pos),
        };
        match n {
            0 | 1 => fields.push(self.list("Conditions", false, |r| r.expression(depth + 1))?),
            2 | 4 | 5 => fields.extend(self.fields(&[
                ("Name", String),
                ("Value", SignedShort),
                ("Comparison", Short),
            ])?),
            3 => fields.push(self.field("Mood", String)?),
            6 => fields.extend(self.fields(&[("Quest ID", Short), ("State", Short)])?),
            _ => {}
        }
        Ok(Node::Record {
            name: name.into(),
            fields,
        })
    }
    fn item(&mut self) -> Result<Node> {
        use Kind::*;
        let mut fields = self.fields(&[
            ("Item", String),
            ("Name key", String),
            ("Description key", String),
            ("Icon", String),
            ("Category", Short),
            ("Quantity", U32),
            ("Price", I32),
            ("Flag", Bool),
        ])?;
        fields.push(self.list("State changes", false, |r| {
            Ok(Node::Record {
                name: "State".into(),
                fields: r.fields(&[("Name", String), ("Value", SignedShort)])?,
            })
        })?);
        fields.push(self.field("Lua", String)?);
        fields.push(self.expression(0)?);
        Ok(Node::Record {
            name: "Item".into(),
            fields,
        })
    }
    fn sequence(&mut self, depth: usize) -> Result<Node> {
        ensure!(depth < 32, "Sequence nesting limit");
        use Kind::*;
        let mut fields = self.fields(&[
            ("Name", String),
            ("Character", String),
            ("Area", String),
            ("Flags", Short),
            ("Priority", Short),
        ])?;
        fields.push(self.list("Events", false, |r| r.event(depth))?);
        fields.push(self.expression(0)?);
        Ok(Node::Record {
            name: "Sequence".into(),
            fields,
        })
    }
    fn event(&mut self, depth: usize) -> Result<Node> {
        use Kind::*;
        let tag = self.field("_type", Short)?;
        let n = if let Node::Field { value, .. } = &tag {
            value.as_u64().unwrap() as usize
        } else {
            unreachable!()
        };
        ensure!(n < EVENT_NAMES.len(), "Unknown EM1 event {n}");
        let mut fields = vec![tag, self.field("Label", String)?];
        match n {
            0 => fields.extend(self.fields(&[
                ("Speaker", String),
                ("Speaker key", String),
                ("Listener", String),
                ("Text key", String),
                ("Camera", String),
                ("Animation", String),
                ("Audio event", String),
                ("Voice", String),
                ("Flag", Bool),
            ])?),
            1 => {
                fields.extend(self.fields(&[
                    ("Speaker", String),
                    ("Speaker key", String),
                    ("Listener", String),
                    ("Selection mode", Short),
                ])?);
                fields.push(self.list("Lines", false, |r| {
                    Ok(Node::Record {
                        name: "Line".into(),
                        fields: r.fields(&[
                            ("Text key", String),
                            ("Camera", String),
                            ("Animation", String),
                            ("Audio event", String),
                            ("Voice", String),
                            ("Flag", Bool),
                        ])?,
                    })
                })?);
            }
            2 => {
                fields.extend(self.fields(&[
                    ("Speaker", String),
                    ("Speaker key", String),
                    ("Text key", String),
                    ("Animation", String),
                    ("Audio event", String),
                    ("Voice", String),
                    ("Flag", Bool),
                    ("Choice flag", Bool),
                ])?);
                fields.push(self.list("Choices", false, |r| {
                    Ok(Node::Record {
                        name: "Choice".into(),
                        fields: r.fields(&[("Text key", String), ("Target label", String)])?,
                    })
                })?);
            }
            3 => fields.extend(self.fields(&[("Quest ID", Short), ("State", Short)])?),
            4 => fields.extend(self.fields(&[
                ("Quest ID", Short),
                ("State", Short),
                ("Silent", Bool),
            ])?),
            5 => fields.extend(self.fields(&[
                ("Camera", String),
                ("Target", String),
                ("Blend seconds", Float),
                ("Flag", Bool),
                ("Wait", Bool),
            ])?),
            6 => fields.push(self.list("Branches", true, |r| r.sequence(depth + 1))?),
            7 => {
                fields.extend(self.fields(&[
                    ("Target label", String),
                    ("Target context", String),
                    ("Flag", Bool),
                ])?);
                fields.push(self.expression(0)?);
            }
            8 => {
                fields.push(self.item()?);
                fields.push(self.field("Amount", I32)?);
            }
            9 => fields.push(self.field("Lua", String)?),
            10 => fields.push(self.field("End mode", Short)?),
            11 => {
                fields.push(self.field("Flag", Bool)?);
                fields.push(self.list("State changes", false, |r| {
                    Ok(Node::Record {
                        name: "State".into(),
                        fields: r.fields(&[("Name", String), ("Value", SignedShort)])?,
                    })
                })?);
            }
            12 => fields.push(self.field("Seconds", Float)?),
            13 => fields.push(self.list("Items", false, |r| r.item())?),
            14 => fields.extend(self.fields(&[("Sequence", String), ("Flag", Bool)])?),
            15 | 16 => fields.push(self.field("Quest ID", Short)?),
            _ => unreachable!(),
        }
        Ok(Node::Record {
            name: EVENT_NAMES[n].into(),
            fields,
        })
    }
    fn quest(&mut self) -> Result<Node> {
        use Kind::*;
        let mut fields = self.fields(&[("ID", Short), ("Name", String), ("Icon", String)])?;
        fields.push(self.list("Icon conditions", false, |r| {
            Ok(Node::Record {
                name: "Icon condition".into(),
                fields: r.fields(&[("Icon", String), ("State", String), ("Context", String)])?,
            })
        })?);
        fields.push(self.list("Stages", false, |r| {
            Ok(Node::Record {
                name: "Stage".into(),
                fields: r.fields(&[
                    ("Area", String),
                    ("Character", String),
                    ("Description key", String),
                    ("Completion key", String),
                    ("State ID", Short),
                    ("Condition value", I32),
                    ("Completion type", Short),
                    ("Completion value", I32),
                ])?,
            })
        })?);
        Ok(Node::Record {
            name: "Quest".into(),
            fields,
        })
    }
}
pub const EVENT_NAMES: [&str; 17] = [
    "Dialogue",
    "Random dialogue",
    "Choice",
    "Quest state",
    "Quest state (silent option)",
    "Camera",
    "Branch",
    "Jump",
    "Inventory",
    "Lua call",
    "End",
    "Set state",
    "Delay",
    "Shop",
    "Chain sequence",
    "Quest event 15",
    "Quest event 16",
];
pub const CONDITION_NAMES: [&str; 7] = [
    "All conditions",
    "Any condition",
    "State condition",
    "Mood condition",
    "Currency condition",
    "Item condition",
    "Quest condition",
];
impl Document {
    pub fn new_condition(tag: usize) -> Result<Node> {
        ensure!(tag < CONDITION_NAMES.len(), "Unknown condition");
        use Kind::*;
        let mut r = Reader {
            data: &[],
            pos: 0,
            budget: 100,
            template: Some(tag as u16),
        };
        let mut fields = vec![r.field("_type", Short)?];
        match tag {
            0 | 1 => fields.push(Node::List {
                name: "Conditions".into(),
                count: Count::U32,
                items: vec![],
            }),
            2 | 4 | 5 => fields.extend(r.fields(&[
                ("Name", String),
                ("Value", SignedShort),
                ("Comparison", Short),
            ])?),
            3 => fields.push(r.field("Mood", String)?),
            6 => fields.extend(r.fields(&[("Quest ID", Short), ("State", Short)])?),
            _ => unreachable!(),
        }
        Ok(Node::Record {
            name: CONDITION_NAMES[tag].into(),
            fields,
        })
    }

    pub fn load(path: &str, data: &[u8]) -> Result<Self> {
        ensure!(
            data.len() >= 8 && data.len() <= 32 * 1024 * 1024,
            "Invalid BSQ size"
        );
        let mut r = Reader {
            data,
            pos: 0,
            budget: 2_000_000,
            template: None,
        };
        let version = r.field("_version", Kind::U32)?;
        ensure!(
            matches!(&version,Node::Field{value,..} if value.as_u64()==Some(30)),
            "Unsupported Apprentice version"
        );
        let path = path.to_lowercase();
        let mut fields = vec![version];
        let database =
            path.ends_with("apprenticequests.bsq") || path.ends_with("apprenticeglobalstate.bsq");
        if database {
            let quests = path.ends_with("apprenticequests.bsq");
            fields.push(r.field("_id_limit", if quests { Kind::U32 } else { Kind::Short })?);
            let mut items = vec![];
            while r.pos < data.len() {
                items.push(if quests {
                    r.quest()?
                } else {
                    Node::Record {
                        name: "State".into(),
                        fields: r.fields(&[
                            ("Name", Kind::String),
                            ("ID", Kind::Short),
                            ("Initial value", Kind::SignedShort),
                            ("Flag", Kind::Bool),
                        ])?,
                    }
                });
            }
            fields.push(Node::List {
                name: if quests { "Quests" } else { "States" }.into(),
                count: Count::UntilEnd,
                items,
            });
        } else {
            fields.push(r.sequence(0)?);
        }
        ensure!(
            r.pos == data.len(),
            "Unexpected data after BSQ at {:#x}",
            r.pos
        );
        Ok(Self {
            root: Node::Record {
                name: if database { "Database" } else { "Conversation" }.into(),
                fields,
            },
            database,
        })
    }
    pub fn new_event(tag: u16) -> Result<Node> {
        ensure!((tag as usize) < EVENT_NAMES.len(), "Unknown event");
        Reader {
            data: &[],
            pos: 0,
            budget: 1000,
            template: Some(tag),
        }
        .event(0)
    }
    pub fn new_sequence() -> Result<Self> {
        let mut r = Reader {
            data: &[],
            pos: 0,
            budget: 1000,
            template: Some(0),
        };
        let mut version = r.field("_version", Kind::U32)?;
        if let Node::Field { value, .. } = &mut version {
            *value = json!(30);
        }
        Ok(Self {
            root: Node::Record {
                name: "Conversation".into(),
                fields: vec![version, r.sequence(0)?],
            },
            database: false,
        })
    }
    pub fn new_list_item(name: &str) -> Result<Node> {
        use Kind::*;
        let mut r = Reader {
            data: &[],
            pos: 0,
            budget: 1000,
            template: Some(0),
        };
        let (label, spec) = match name {
            "Events" => return Self::new_event(0),
            "Branches" => return r.sequence(0),
            "Conditions" => return r.expression(0),
            "Items" => return r.item(),
            "Quests" => return r.quest(),
            "States" => (
                "State",
                vec![
                    ("Name", String),
                    ("ID", Short),
                    ("Initial value", SignedShort),
                    ("Flag", Bool),
                ],
            ),
            "State changes" => ("State", vec![("Name", String), ("Value", SignedShort)]),
            "Lines" => (
                "Line",
                vec![
                    ("Text key", String),
                    ("Camera", String),
                    ("Animation", String),
                    ("Audio event", String),
                    ("Voice", String),
                    ("Flag", Bool),
                ],
            ),
            "Choices" => (
                "Choice",
                vec![("Text key", String), ("Target label", String)],
            ),
            "Icon conditions" => (
                "Icon condition",
                vec![("Icon", String), ("State", String), ("Context", String)],
            ),
            "Stages" => (
                "Stage",
                vec![
                    ("Area", String),
                    ("Character", String),
                    ("Description key", String),
                    ("Completion key", String),
                    ("State ID", Short),
                    ("Condition value", I32),
                    ("Completion type", Short),
                    ("Completion value", I32),
                ],
            ),
            _ => bail!("Unknown sequence collection {name}"),
        };
        Ok(Node::Record {
            name: label.into(),
            fields: r.fields(&spec)?,
        })
    }
    pub fn save(&self) -> Result<Vec<u8>> {
        let mut bytes = vec![];
        self.root.write(&mut bytes)?;
        Ok(bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_boolean_uses_only_first_byte_and_preserves_padding() {
        for bytes in [
            [0, 0xff, 0xff, 0xff],
            [1, 0, 0, 0],
            [0x80, 0xcd, 0xcd, 0xcd],
            [0, 0, 0, 1],
        ] {
            let mut reader = Reader {
                data: &bytes,
                pos: 0,
                budget: 1,
                template: None,
            };
            let mut node = reader.field("Silent", Kind::Bool).unwrap();
            assert!(matches!(&node,Node::Field{value,..} if value.as_bool()==Some(bytes[0]!=0)));
            assert_eq!(reader.pos, 4);
            let mut output = vec![];
            node.write(&mut output).unwrap();
            assert_eq!(output, bytes);
            if let Node::Field { value, .. } = &mut node {
                *value = json!(bytes[0] == 0);
            }
            output.clear();
            node.write(&mut output).unwrap();
            assert_eq!(
                output,
                if bytes[0] == 0 {
                    vec![255; 4]
                } else {
                    vec![0; 4]
                }
            );
        }
    }
    #[test]
    fn native_quest_event_four_exposes_state_and_silent_flag() {
        // REL27f9c8 reads two padded shorts and one four-byte bool;
        //27faf8 branches to silent update24506c rather than notification244f64.
        let event = Document::new_event(4).unwrap();
        let Node::Record { fields, .. } = event else {
            panic!("Expected event record")
        };
        assert!(fields
            .iter()
            .any(|field| matches!(field,Node::Field{name,kind:Kind::Short,..} if name=="State")));
        assert!(fields
            .iter()
            .any(|field| matches!(field,Node::Field{name,kind:Kind::Bool,..} if name=="Silent")));
        assert!(!fields
            .iter()
            .any(|field| matches!(field,Node::Field{name,..} if name=="Value" || name=="Stage")));
    }
    #[test]
    fn every_event_template_and_nested_collection_can_be_authored() {
        for tag in 0..17 {
            let mut d = Document::new_sequence().unwrap();
            let mut event = Document::new_event(tag).unwrap();
            fn populate(n: &mut Node) {
                match n {
                    Node::Field {
                        kind: Kind::String,
                        value,
                        ..
                    } => *value = json!("New text é with a longer length"),
                    Node::Record { fields, .. } => {
                        for n in fields {
                            populate(n)
                        }
                    }
                    Node::List { name, items, .. } => {
                        if let Ok(item) = Document::new_list_item(name) {
                            items.push(item);
                        }
                    }
                    _ => {}
                }
            }
            populate(&mut event);
            if let Node::Record { fields, .. } = &mut d.root {
                if let Node::Record { fields, .. } = &mut fields[1] {
                    if let Node::List { items, .. } = &mut fields[5] {
                        items.push(event);
                    }
                }
            }
            let bytes = d.save().unwrap();
            let loaded =
                Document::load("new.bsq", &bytes).unwrap_or_else(|e| panic!("event {tag}: {e}"));
            assert_eq!(loaded.save().unwrap(), bytes);
            for n in 0..bytes.len() {
                assert!(
                    Document::load("new.bsq", &bytes[..n]).is_err(),
                    "event {tag}, {n}"
                );
            }
        }
    }
    #[test]
    #[ignore = "Requires local EM1 gameplay research fixtures"]
    fn every_retail_bsq_roundtrips() {
        let rows: Value =
            serde_json::from_slice(&std::fs::read("target/gameplay-research/audit.json").unwrap())
                .unwrap();
        let mut count = 0;
        for row in rows.as_array().unwrap() {
            let Some(name) = row["example"].as_str().filter(|s| s.ends_with(".bsq")) else {
                continue;
            };
            let data = std::fs::read(format!("target/gameplay-research/{name}")).unwrap();
            let d = Document::load(row["path"].as_str().unwrap(), &data)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(d.save().unwrap(), data, "{name}");
            count += 1;
        }
        assert_eq!(count, 1624);
    }
}
