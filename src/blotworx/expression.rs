//! Scalar expressions used by Mickey's behavior conditions and modifiers.
//! Compile once; evaluate RPN without parsing text in the simulation loop.
use anyhow::{bail, ensure, Context, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    Number(f32),
    Variable(usize),
    /// Values are the Operator enum serialized in the Wii behavior schema.
    Operator(u8),
}
#[derive(Debug)]
pub struct Program {
    instructions: Vec<Instruction>,
    stack_size: usize,
}
impl Program {
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }
    pub fn from_retail(tokens: &[RetailToken]) -> Result<Self> {
        let code = tokens
            .iter()
            .map(|token| {
                Ok(match token.kind {
                    1 => Instruction::Operator(token.operator),
                    2 => Instruction::Number(f32::from_bits(token.data_bits)),
                    3 => {
                        let index = f32::from_bits(token.data_bits);
                        ensure!(
                            index.is_finite()
                                && index >= 0.
                                && index.fract() == 0.
                                && (index as usize) as f32 == index,
                            "Invalid compiled variable index"
                        );
                        Instruction::Variable(index as usize)
                    }
                    other => bail!("Unsupported compiled token type {other}"),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Self::new(code)
    }
    pub fn compile(source: &str, variables: &[String]) -> Result<Self> {
        let mut parser = Parser {
            source: source.as_bytes(),
            at: 0,
            variables,
            code: vec![],
        };
        parser.expression(0)?;
        parser.whitespace();
        ensure!(
            parser.at == parser.source.len(),
            "Unexpected expression input at byte {}",
            parser.at
        );
        Self::new(parser.code)
    }
    pub fn new(instructions: Vec<Instruction>) -> Result<Self> {
        let (mut depth, mut maximum) = (0usize, 0);
        for instruction in &instructions {
            match instruction {
                Instruction::Number(_) | Instruction::Variable(_) => depth += 1,
                Instruction::Operator(op) => {
                    let arity = arity(*op)?;
                    ensure!(depth >= arity, "Expression stack underflow");
                    depth = depth + 1 - arity;
                }
            }
            maximum = maximum.max(depth);
        }
        ensure!(depth == 1, "Expression must leave exactly one result");
        Ok(Self {
            instructions,
            stack_size: maximum,
        })
    }
    pub fn scratch(&self) -> Vec<f32> {
        vec![0.; self.stack_size]
    }
    pub fn evaluate(&self, words: &[u32], types: &[u8], scratch: &mut [f32]) -> Result<f32> {
        self.evaluate_with_random(words, types, scratch, None)
    }
    pub fn evaluate_with_random(
        &self,
        words: &[u32],
        types: &[u8],
        scratch: &mut [f32],
        mut random: Option<&mut u32>,
    ) -> Result<f32> {
        ensure!(
            scratch.len() >= self.stack_size,
            "Expression scratch is too small"
        );
        ensure!(
            words.len() == types.len(),
            "Variable type/value count differs"
        );
        let mut depth = 0;
        for instruction in &self.instructions {
            let value = match *instruction {
                Instruction::Number(value) => value,
                Instruction::Variable(index) => {
                    let word = *words
                        .get(index)
                        .context("Expression variable out of range")?;
                    match types[index] {
                        0 => {
                            if word != 0 {
                                1.
                            } else {
                                0.
                            }
                        }
                        1 => (word as i8) as f32,
                        2 => (word as i16) as f32,
                        3 => (word as i32) as f32,
                        4 => f32::from_bits(word),
                        kind => bail!("Unsupported scalar variable type {kind}"),
                    }
                }
                Instruction::Operator(op) => {
                    let count = arity(op)?;
                    depth -= count;
                    let a = if count > 0 { scratch[depth] } else { 0. };
                    let b = if count >= 2 { scratch[depth + 1] } else { 0. };
                    let c = if count == 3 { scratch[depth + 2] } else { 0. };
                    let boolean = |v| if v { 1. } else { 0. };
                    match op {
                        // DOL80221488..802214cc: nullary rand01, shared
                        // character RNG; still evaluated in an unused cond arm.
                        1 => random_unit(random.as_deref_mut())?,
                        2 => boolean(a == 0.),
                        3 => -a,
                        4 => a,
                        5 => a.sin(), // BlotlingKamikaze's cooked twist expression.
                        10 => a.abs(), // DOL80221414: fabs, then frsp.
                        14 => a * b,
                        15 => a / b,
                        16 => a + b,
                        17 => a - b,
                        18 => boolean(a != 0. || b != 0.),
                        19 => boolean(a != 0. && b != 0.),
                        20 => boolean(a == b),
                        21 => boolean(a != b),
                        22 => boolean(a < b),
                        23 => boolean(a > b),
                        24 => boolean(a <= b),
                        25 => boolean(a >= b),
                        // Native EM2 hkbCompiledExpressionSet, cases 0x1b/0x1c:
                        // select using the rounded subtraction, preserving NaN/zero order.
                        27 => {
                            if b - a < 0. {
                                a
                            } else {
                                b
                            }
                        }
                        28 => {
                            if b - a < 0. {
                                b
                            } else {
                                a
                            }
                        }
                        29 => {
                            let unit = random_unit(random.as_deref_mut())?;
                            (b - a).mul_add(unit, a)
                        }
                        // DOL80220fcc..80220fdc selects after all three RPN
                        // arguments have been evaluated; zero chooses c.
                        43 => {
                            if a != 0. {
                                b
                            } else {
                                c
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            };
            scratch[depth] = value;
            depth += 1;
        }
        Ok(scratch[0])
    }
}
fn random_unit(random: Option<&mut u32>) -> Result<f32> {
    let seed = random.context("Random expression requires the character random generator")?;
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    Ok((*seed as f32) * (1.0 / 4_294_967_296.0))
}
#[derive(Debug, serde::Deserialize)]
pub struct RetailToken {
    pub data_bits: u32,
    #[serde(rename = "type")]
    pub kind: u8,
    pub operator: u8,
}
fn arity(op: u8) -> Result<usize> {
    match op {
        1 => Ok(0),
        2..=5 | 10 => Ok(1),
        14..=25 | 27..=29 => Ok(2),
        43 => Ok(3),
        _ => bail!("Unsupported expression operator {op}"),
    }
}
struct Parser<'a> {
    source: &'a [u8],
    at: usize,
    variables: &'a [String],
    code: Vec<Instruction>,
}
impl Parser<'_> {
    fn whitespace(&mut self) {
        while self
            .source
            .get(self.at)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.at += 1;
        }
    }
    fn take(&mut self, token: &[u8]) -> bool {
        self.whitespace();
        if self.source[self.at..].starts_with(token) {
            self.at += token.len();
            true
        } else {
            false
        }
    }
    fn expression(&mut self, minimum: u8) -> Result<()> {
        self.atom()?;
        loop {
            self.whitespace();
            // Longest tokens first; logical and/or have lower precedence.
            let found = [
                ("||", 1, 18),
                ("&&", 2, 19),
                ("==", 3, 20),
                ("!=", 3, 21),
                ("<=", 4, 24),
                (">=", 4, 25),
                ("<", 4, 22),
                (">", 4, 23),
                ("+", 5, 16),
                ("-", 5, 17),
                ("*", 6, 14),
                ("/", 6, 15),
            ]
            .into_iter()
            .find(|(text, _, _)| self.source[self.at..].starts_with(text.as_bytes()));
            let Some((text, precedence, op)) = found else {
                break;
            };
            if precedence < minimum {
                break;
            }
            self.at += text.len();
            self.expression(precedence + 1)?;
            self.code.push(Instruction::Operator(op));
        }
        Ok(())
    }
    fn atom(&mut self) -> Result<()> {
        self.whitespace();
        for (text, op) in [("!", 2), ("-", 3), ("+", 4)] {
            if self.take(text.as_bytes()) {
                self.atom()?;
                self.code.push(Instruction::Operator(op));
                return Ok(());
            }
        }
        if self.take(b"(") {
            self.expression(0)?;
            ensure!(self.take(b")"), "Unclosed expression parenthesis");
            return Ok(());
        }
        let start = self.at;
        let first = *self
            .source
            .get(start)
            .context("Expected expression operand")?;
        if first.is_ascii_digit() || first == b'.' {
            while self
                .source
                .get(self.at)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            {
                self.at += 1;
            }
            if self
                .source
                .get(self.at)
                .is_some_and(|c| matches!(c, b'e' | b'E'))
            {
                self.at += 1;
                if self
                    .source
                    .get(self.at)
                    .is_some_and(|c| matches!(c, b'+' | b'-'))
                {
                    self.at += 1;
                }
                while self.source.get(self.at).is_some_and(u8::is_ascii_digit) {
                    self.at += 1;
                }
            }
            let value: f32 = std::str::from_utf8(&self.source[start..self.at])?.parse()?;
            ensure!(value.is_finite(), "Non-finite expression literal");
            self.code.push(Instruction::Number(value));
        } else {
            ensure!(
                first.is_ascii_alphabetic() || first == b'_',
                "Unsupported operand at byte {start}"
            );
            while self
                .source
                .get(self.at)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                self.at += 1;
            }
            let name = std::str::from_utf8(&self.source[start..self.at])?;
            match name {
                "rand01" => {
                    ensure!(
                        self.take(b"(") && self.take(b")"),
                        "rand01 requires no arguments"
                    );
                    self.code.push(Instruction::Operator(1));
                }
                "max" | "min" => {
                    ensure!(
                        self.take(b"("),
                        "{name} requires two parenthesized arguments"
                    );
                    self.expression(0)?;
                    ensure!(self.take(b","), "{name} requires two arguments");
                    self.expression(0)?;
                    ensure!(self.take(b")"), "{name} requires exactly two arguments");
                    self.code
                        .push(Instruction::Operator(if name == "max" { 27 } else { 28 }));
                }
                "cond" => {
                    ensure!(
                        self.take(b"("),
                        "cond requires three parenthesized arguments"
                    );
                    self.expression(0)?;
                    ensure!(self.take(b","), "cond requires three arguments");
                    self.expression(0)?;
                    ensure!(self.take(b","), "cond requires three arguments");
                    self.expression(0)?;
                    ensure!(self.take(b")"), "cond requires exactly three arguments");
                    self.code.push(Instruction::Operator(43));
                }
                "fabs" | "sin" => {
                    ensure!(self.take(b"("), "{name} requires one parenthesized argument");
                    self.expression(0)?;
                    ensure!(self.take(b")"), "{name} requires exactly one argument");
                    self.code.push(Instruction::Operator(if name == "sin" { 5 } else { 10 }));
                }
                "randRange" => {
                    ensure!(
                        self.take(b"("),
                        "randRange requires two parenthesized arguments"
                    );
                    self.expression(0)?;
                    ensure!(self.take(b","), "randRange requires two arguments");
                    self.expression(0)?;
                    ensure!(self.take(b")"), "randRange requires exactly two arguments");
                    self.code.push(Instruction::Operator(29));
                }
                "true" => self.code.push(Instruction::Number(1.)),
                "false" => self.code.push(Instruction::Number(0.)),
                _ => {
                    let mut matches = self
                        .variables
                        .iter()
                        .enumerate()
                        .filter(|(_, v)| v.as_str() == name);
                    let (index, _) = matches
                        .next()
                        .with_context(|| format!("Unknown expression variable {name}"))?;
                    ensure!(matches.next().is_none(), "Ambiguous variable {name}");
                    self.code.push(Instruction::Variable(index));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_spatter_fabs_condition_uses_retail_unary_opcode() -> Result<()> {
        let program = Program::compile("fabs(VAR_Turn_Speed) < 0.01", &["VAR_Turn_Speed".into()])?;
        assert_eq!(
            program.instructions(),
            &[
                Instruction::Variable(0),
                Instruction::Operator(10),
                Instruction::Number(0.01),
                Instruction::Operator(22)
            ]
        );
        let mut scratch = program.scratch();
        for (speed, expected) in [
            (-0.02, 0.),
            (-0.005, 1.),
            (-0.0, 1.),
            (0.01, 0.),
            (f32::INFINITY, 0.),
        ] {
            assert_eq!(
                program.evaluate(&[speed.to_bits()], &[4], &mut scratch)?,
                expected
            );
        }
        let abs = Program::new(vec![Instruction::Variable(0), Instruction::Operator(10)])?;
        assert_eq!(
            abs.evaluate(&[(-0.0_f32).to_bits()], &[4], &mut scratch)?
                .to_bits(),
            0
        );
        assert!(Program::compile("fabs(1,2)", &[]).is_err());
        Ok(())
    }
    #[test]
    fn rand_range_uses_shared_character_seed_and_requires_context() -> Result<()> {
        let program = Program::compile("randRange(0,1)+randRange(0,1)", &[])?;
        let mut scratch = program.scratch();
        let mut seed = 0;
        let value = program.evaluate_with_random(&[], &[], &mut scratch, Some(&mut seed))?;
        assert_eq!(seed, 1_196_435_762);
        let expected = (1_013_904_223_u32 as f32) * (1.0 / 4_294_967_296.0)
            + (1_196_435_762_u32 as f32) * (1.0 / 4_294_967_296.0);
        assert_eq!(value.to_bits(), expected.to_bits());
        assert!(program.evaluate(&[], &[], &mut scratch).is_err());
        assert!(Program::compile("randRange(1)", &[]).is_err());
        Ok(())
    }
    #[test]
    fn authored_gremlin_and_oswald_functions_follow_retail_stack_semantics() -> Result<()> {
        let program = Program::compile("cond(flag,rand01(),randRange(4,5))", &["flag".into()])?;
        assert_eq!(
            program.instructions(),
            &[
                Instruction::Variable(0),
                Instruction::Operator(1),
                Instruction::Number(4.),
                Instruction::Number(5.),
                Instruction::Operator(29),
                Instruction::Operator(43)
            ]
        );
        let mut scratch = program.scratch();
        let mut seed = 0;
        let yes = program.evaluate_with_random(&[1], &[3], &mut scratch, Some(&mut seed))?;
        assert_eq!(
            seed, 1_196_435_762,
            "Both arms evaluate, including their random draws"
        );
        assert_eq!(
            yes.to_bits(),
            ((1_013_904_223_u32 as f32) * (1. / 4_294_967_296.)).to_bits()
        );
        seed = 0;
        let no = program.evaluate_with_random(&[0], &[3], &mut scratch, Some(&mut seed))?;
        assert_eq!(
            no.to_bits(),
            ((1_196_435_762_u32 as f32) * (1. / 4_294_967_296.) + 4.).to_bits()
        );
        assert!(program.evaluate(&[1], &[3], &mut scratch).is_err());
        for invalid in ["rand01(1)", "cond(1,2)", "cond(1,2,3,4)"] {
            assert!(
                Program::compile(invalid, &[]).is_err(),
                "Accepted {invalid}"
            );
        }
        Ok(())
    }
    #[test]
    fn scalar_type_ids_match_serialized_havok_enum() -> Result<()> {
        let program = Program::new(vec![Instruction::Variable(0)])?;
        let mut scratch = program.scratch();
        for (kind, word, expected) in [
            (0, 7, 1.),
            (1, 0xff, -1.),
            (2, 0xff, 255.),
            (2, 0xffff, -1.),
            (3, 0xffff, 65535.),
            (4, 1f32.to_bits(), 1.),
        ] {
            assert_eq!(program.evaluate(&[word], &[kind], &mut scratch)?, expected);
        }
        assert!(program.evaluate(&[0], &[5], &mut scratch).is_err());
        Ok(())
    }
    #[test]
    fn compiler_matches_captured_retail_programs() -> Result<()> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/blotworx/fixtures/behavior_conditions.json"
        ))?;
        let variables: Vec<String> = serde_json::from_value(fixture["variables"].clone())?;
        let programs = fixture["programs"].as_object().unwrap();
        assert_eq!(programs.len(), 7);
        for (id, program) in programs {
            let compiled = Program::compile(program["source"].as_str().unwrap(), &variables)?;
            let tokens: Vec<RetailToken> = serde_json::from_value(program["tokens"].clone())?;
            let reference = Program::from_retail(&tokens)?;
            assert_eq!(
                compiled.instructions(),
                reference.instructions(),
                "Compiler differs for {id}"
            );
        }
        Ok(())
    }
    #[test]
    fn numeric_precedence_types_and_boundaries() -> Result<()> {
        let variables = vec!["speed".into(), "state".into()];
        let p = Program::compile("speed >= 0.6 && (state == 2 || state == 1)", &variables)?;
        let mut scratch = p.scratch();
        assert_eq!(
            p.evaluate(&[0.6f32.to_bits(), 2], &[4, 3], &mut scratch)?,
            1.
        );
        assert_eq!(
            p.evaluate(&[(0.6f32.to_bits() - 1), 2], &[4, 3], &mut scratch)?,
            0.
        );
        assert_eq!(
            p.evaluate(&[0.7f32.to_bits(), 0], &[4, 3], &mut scratch)?,
            0.
        );
        let p = Program::compile("-(2 + 3) * 4 + 2e-1", &[])?;
        assert_eq!(p.evaluate(&[], &[], &mut p.scratch())?, -19.8);
        Ok(())
    }
    #[test]
    fn rejects_unknown_operations_and_malformed_programs() {
        for text in ["missing == 1", "1 +", "(1 == 1", "1 = 2", "sin(0)", "1 2"] {
            assert!(Program::compile(text, &[]).is_err(), "{text}");
        }
        assert!(Program::new(vec![Instruction::Operator(14)]).is_err());
        assert!(Program::new(vec![Instruction::Number(1.), Instruction::Operator(43)]).is_err());
    }
}

#[cfg(test)]
mod min_max_compatibility_tests {
    use super::*;
    #[test]
    fn native_selection_order_and_nested_arguments() -> Result<()> {
        for (source, expected) in [("max(0.3, 0.1)", 0.3), ("min(2, max(-3, 1))", 1.)] {
            let program = Program::compile(source, &[])?;
            assert_eq!(
                program.evaluate(&[], &[], &mut program.scratch())?,
                expected
            );
        }
        for (operator, expected) in [(27, -0.0f32), (28, 0.0f32)] {
            let program = Program::new(vec![
                Instruction::Number(0.),
                Instruction::Number(-0.),
                Instruction::Operator(operator),
            ])?;
            assert_eq!(
                program
                    .evaluate(&[], &[], &mut program.scratch())?
                    .to_bits(),
                expected.to_bits()
            );
        }
        assert!(Program::compile("max(1)", &[]).is_err());
        assert!(Program::compile("min(1, 2, 3)", &[]).is_err());
        Ok(())
    }
}
