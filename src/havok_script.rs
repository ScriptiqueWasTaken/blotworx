//! HavokScript 0x0d structural reader and direct instruction compatibility.
//! Evidence: supplied EM2 Wii U BytecodeReader and hkscdis-rs research.
//! No reconstructed source or original engine code is executed.
use anyhow::{bail, ensure, Context, Result};
#[derive(Debug)]
enum Constant {
    Nil,
    Boolean(u8),
    Number(f64),
    String(Vec<u8>),
}
struct Function {
    up: u8,
    params: u8,
    vararg: u8,
    stack: u8,
    code: Vec<u32>,
    constants: Vec<Constant>,
    children: Vec<Function>,
    source: Vec<u8>,
    lines: Vec<u32>,
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    little: bool,
    size: usize,
    number: usize,
}
impl Reader<'_> {
    fn raw(&mut self, n: usize) -> Result<&[u8]> {
        let end = self
            .at
            .checked_add(n)
            .context("HavokScript offset overflow")?;
        let out = self
            .bytes
            .get(self.at..end)
            .context("Truncated HavokScript")?;
        self.at = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.raw(1)?[0])
    }
    fn word(&mut self) -> Result<u32> {
        let b = self.raw(4)?.try_into()?;
        Ok(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
    fn count(&mut self) -> Result<usize> {
        let n = self.word()? as usize;
        ensure!(
            n <= self.bytes.len() - self.at,
            "HavokScript count exceeds remaining data"
        );
        Ok(n)
    }
    fn size(&mut self) -> Result<usize> {
        if self.size == 4 {
            Ok(self.word()? as usize)
        } else {
            let b = self.raw(8)?.try_into()?;
            usize::try_from(if self.little {
                u64::from_le_bytes(b)
            } else {
                u64::from_be_bytes(b)
            })
            .context("HavokScript size overflow")
        }
    }
    fn string(&mut self) -> Result<Vec<u8>> {
        let n = self.size()?;
        if n == 0 {
            return Ok(Vec::new());
        }
        let s = self.raw(n)?;
        ensure!(s.last() == Some(&0), "Unterminated HavokScript string");
        Ok(s[..n - 1].to_vec())
    }
    fn function(&mut self, depth: usize) -> Result<Function> {
        ensure!(depth < 128, "HavokScript native prototype depth exceeded");
        // Format 13: first field is nups, last field is stack size, verified
        // from native Method offsets and register use in the PC tutorial.
        let up = u8::try_from(self.word()?).context("Too many HKS upvalues")?;
        let params = u8::try_from(self.word()?).context("Too many HKS parameters")?;
        let vararg = self.byte()?;
        let stack = u8::try_from(self.word()?).context("HKS stack exceeds host register layout")?;
        ensure!(
            stack >= params && (2..=250).contains(&stack),
            "Invalid HKS register frame"
        );
        let n = self.size()?;
        self.raw((4 - self.at % 4) % 4)?;
        ensure!(
            n <= (self.bytes.len() - self.at) / 4,
            "Truncated HKS instructions"
        );
        let mut code = Vec::with_capacity(n);
        for _ in 0..n {
            code.push(self.word()?);
        }
        let n = self.count()?;
        let mut constants = Vec::with_capacity(n);
        for _ in 0..n {
            constants.push(match self.byte()? {
                0 => Constant::Nil,
                1 => {
                    let b = self.byte()?;
                    ensure!(b <= 1, "Invalid HKS boolean");
                    Constant::Boolean(b)
                }
                3 => {
                    let number = if self.number == 4 {
                        f32::from_bits(self.word()?) as f64
                    } else {
                        let b = self.raw(8)?.try_into()?;
                        f64::from_bits(if self.little {
                            u64::from_le_bytes(b)
                        } else {
                            u64::from_be_bytes(b)
                        })
                    };
                    Constant::Number(number)
                }
                4 => Constant::String(self.string()?),
                tag => bail!("Unsupported HavokScript constant type {tag}"),
            });
        }
        let debug = self.word()?;
        let mut source = Vec::new();
        let mut lines = Vec::new();
        // Native field is the debug-allocation size, not a Boolean.
        if debug != 0 {
            self.word()?;
            self.word()?;
            source = self.string()?;
            self.string()?;
            for _ in 0..self.count()? {
                lines.push(self.word()?);
            }
            for _ in 0..self.count()? {
                self.string()?;
                self.word()?;
                self.word()?;
            }
            for _ in 0..self.count()? {
                self.string()?;
            }
        }
        ensure!(
            lines.is_empty() || lines.len() == code.len(),
            "HKS debug line count differs from code"
        );
        let n = self.count()?;
        let mut children = Vec::with_capacity(n);
        for _ in 0..n {
            children.push(self.function(depth + 1)?);
        }
        Ok(Function {
            up,
            params,
            vararg,
            stack,
            code,
            constants,
            children,
            source,
            lines,
        })
    }
}
/// Canonical Lua 5.1 chunk; the existing host adapter converts size_t/endian.
/// f32 constants widen exactly; arithmetic and numeric loops use installed
/// f32 helpers. Original VM equivalence still requires reference validation.
pub fn translate(bytes: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        bytes.len() >= 18 && bytes[..6] == *b"\x1bLua\x51\x0d",
        "Expected HavokScript format 13"
    );
    ensure!(
        bytes[6] <= 1
            && bytes[7] == 4
            && matches!(bytes[8], 4 | 8)
            && bytes[9] == 4
            && bytes[10] == 4
            && bytes[11] == 0,
        "Unsupported HavokScript scalar layout"
    );
    ensure!(
        bytes[12] & !3 == 0 && bytes[13] == 0,
        "Unsupported HKS build features or shared-code mode"
    );
    let mut r = Reader {
        bytes,
        at: 14,
        little: bytes[6] != 0,
        size: bytes[8] as usize,
        number: bytes[10] as usize,
    };
    for _ in 0..r.count()? {
        r.word()?;
        r.string()?;
    }
    let f = r.function(0)?;
    if bytes[12] & 2 != 0 {
        ensure!(r.word()? == 1, "Invalid HKS structure table");
        ensure!(
            r.string()?.is_empty(),
            "HavokScript native structures are not implemented"
        );
    }
    ensure!(r.at == bytes.len(), "Trailing HavokScript data");
    let mut out = b"\x1bLua\x51\0\x01\x04\x04\x04\x08\0".to_vec();
    emit_function(&f, &mut out)?;
    Ok(out)
}
fn word(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn string(out: &mut Vec<u8>, s: &[u8]) -> Result<()> {
    if s.is_empty() {
        word(out, 0)
    } else {
        word(out, u32::try_from(s.len() + 1)?);
        out.extend(s);
        out.push(0)
    }
    Ok(())
}
fn abc(op: u32, a: u32, b: u32, c: u32) -> u32 {
    op | (a << 6) | (c << 14) | (b << 23)
}
fn abx(op: u32, a: u32, b: u32) -> u32 {
    op | (a << 6) | (b << 14)
}

/// Register f32 operations in the existing VM. Numeric instructions and loop
/// indices round at the same operation boundaries as the original f32 VM.
/// Nonnumeric arithmetic retains Lua metamethod dispatch instead of coercing
/// game userdata to numbers or silently dropping the operation.
pub fn install(lua: &mlua::Lua) -> mlua::Result<()> {
    use mlua::Value;
    for (name, expr, operation) in [
        ("add", "a+b", 0),
        ("sub", "a-b", 1),
        ("mul", "a*b", 2),
        ("div", "a/b", 3),
        ("mod", "a%b", 4),
        ("pow", "a^b", 5),
    ] {
        let fallback: mlua::Function = lua
            .load(format!("return function(a,b)return {expr} end"))
            .eval()?;
        let function = lua.create_function(move |lua, (a, b): (Value, Value)| {
            match (lua.coerce_number(a.clone())?, lua.coerce_number(b.clone())?) {
                (Some(a), Some(b)) => {
                    let (a, b) = (a as f32, b as f32);
                    let v = match operation {
                        0 => a + b,
                        1 => a - b,
                        2 => a * b,
                        3 => a / b,
                        4 => a - (a / b).floor() * b,
                        _ => a.powf(b),
                    };
                    Ok(Value::Number(v as f64))
                }
                _ => fallback.call::<Value>((a, b)),
            }
        })?;
        lua.globals()
            .raw_set(format!("__blotworx_hks_{name}"), function)?;
    }
    lua.globals().raw_set(
        "__blotworx_hks_for_init",
        lua.create_function(|lua, (a, b, c): (Value, Value, Value)| {
            let convert = |v| {
                lua.coerce_number(v)?.map(|v| v as f32).ok_or_else(|| {
                    mlua::Error::RuntimeError("HavokScript numeric for requires numbers".into())
                })
            };
            let (index, limit, step) = (convert(a)?, convert(b)?, convert(c)?);
            Ok(((index - step) as f64, limit as f64, step as f64))
        })?,
    )?;
    lua.globals().raw_set(
        "__blotworx_hks_for_next",
        lua.create_function(|_, (index, limit, step): (f64, f64, f64)| {
            let (limit, step) = (limit as f32, step as f32);
            let index = index as f32 + step;
            Ok((
                index as f64,
                if step > 0. {
                    index <= limit
                } else {
                    limit <= index
                },
            ))
        })?,
    )?;
    Ok(())
}
fn emit_function(f: &Function, out: &mut Vec<u8>) -> Result<()> {
    let has_arithmetic = f.code.iter().any(|v| matches!(v >> 25, 40..=51 | 62 | 63));
    let has_for = f.code.iter().any(|v| matches!(v >> 25, 62 | 63));
    let scratch = f.stack as u32;
    let stack = if has_arithmetic {
        scratch + if has_for { 4 } else { 3 }
    } else {
        scratch
    };
    ensure!(
        stack <= 250,
        "HKS arithmetic scratch registers exceed host frame"
    );
    let mut extras = Vec::<String>::new();
    let mut helper = |name: &str| -> Result<u32> {
        let name = format!("__blotworx_hks_{name}");
        let index = if let Some(i) = extras.iter().position(|s| *s == name) {
            i
        } else {
            extras.push(name);
            extras.len() - 1
        };
        u32::try_from(f.constants.len() + index).context("HKS helper constant overflow")
    };
    let mut code = Vec::new();
    let mut source_lines = Vec::new();
    let mut offsets = vec![0; f.code.len() + 1];
    let mut relocations = Vec::<(usize, usize, u32, u32)>::new();
    let mut bindings = 0usize;
    let mut cache_data = false;
    for (pc, &v) in f.code.iter().enumerate() {
        offsets[pc] = code.len();
        let start = code.len();
        let (op, a, b, c, bx) = (
            v >> 25,
            v & 255,
            (v >> 17) & 255,
            (v >> 8) & 511,
            (v >> 8) & 0x1ffff,
        );
        let name = String::from_utf8_lossy(&f.source);
        if bindings > 0 {
            ensure!(op == 76, "Expected HKS closure capture at {name}:{pc}");
            bindings -= 1;
            code.push(match a {
                1 => abc(0, 0, bx, 0),
                0 => abc(4, 0, bx, 0),
                _ => bail!("Unsupported HKS closure binding {a} at {name}:{pc}"),
            });
        } else if (40..=51).contains(&op) {
            let operation = ["add", "sub", "mul", "div", "mod", "pow"][((op - 40) / 2) as usize];
            code.push(abx(5, scratch, helper(operation)?));
            code.push(if op & 1 != 0 {
                abx(1, scratch + 1, b)
            } else {
                abc(0, scratch + 1, b, 0)
            });
            code.push(if c & 256 != 0 {
                abx(1, scratch + 2, c & 255)
            } else {
                abc(0, scratch + 2, c, 0)
            });
            code.push(abc(28, scratch, 3, 2));
            code.push(abc(0, a, scratch, 0));
        } else if matches!(op, 62 | 63) {
            let target = pc as i64 + 1 + bx as i64 - 65535;
            ensure!(
                target >= 0 && target < f.code.len() as i64,
                "HKS loop jump leaves function"
            );
            code.push(abx(
                5,
                scratch,
                helper(if op == 62 { "for_init" } else { "for_next" })?,
            ));
            for i in 0..3 {
                code.push(abc(0, scratch + 1 + i, a + i, 0));
            }
            if op == 62 {
                code.push(abc(28, scratch, 4, 4));
                for i in 0..3 {
                    code.push(abc(0, a + i, scratch + i, 0));
                }
            } else {
                code.push(abc(28, scratch, 4, 3));
                code.push(abc(0, a, scratch, 0));
                code.push(abc(26, scratch + 1, 0, 0));
                relocations.push((code.len(), pc + 1, 22, 0));
                code.push(0);
                code.push(abc(0, a + 3, a, 0));
            }
            relocations.push((code.len(), target as usize, 22, 0));
            code.push(0);
        } else {
            let instruction = match op {
                76 => {
                    ensure!(cache_data, "Unexpected HKS DATA at {name}:{pc}");
                    abc(0, 0, 0, 0)
                }
                0 | 73 | 88 => abc(6, a, b, c | 256),
                7 => abc(0, a, b, 0),
                8 => abc(11, a, b, c),
                6 | 91 => {
                    ensure!(
                        matches!(f.constants.get(bx as usize), Some(Constant::String(_))),
                        "Invalid HKS global name"
                    );
                    abx(5, a, bx)
                }
                27 => {
                    ensure!(
                        matches!(f.constants.get(bx as usize), Some(Constant::String(_))),
                        "Invalid HKS global name"
                    );
                    abx(7, a, bx)
                }
                25 => {
                    ensure!(
                        (bx as usize) < f.constants.len(),
                        "Invalid HKS constant index"
                    );
                    abx(1, a, bx)
                }
                26 => abc(3, a, b, 0),
                13 => abc(2, a, b, 0),
                38 => abc(4, a, b, 0),
                39 | 70 => abc(8, a, b, 0),
                10 | 11 | 12 => abc(6, a, b, c),
                15 | 74 => abc(9, a, b | 256, c),
                16 | 18 | 20 => abc(9, a, b, c),
                17 | 19 | 21 => abc(9, a, b | 256, c),
                2 | 3 | 29 | 30 | 69 => abc(28, a, b, c),
                22 | 23 | 24 | 37 | 68 => abc(29, a, b, c),
                9 => abc(30, a, b, 0),
                1 | 71 => abc(26, a, 0, c),
                61 => abc(27, a, b, c),
                4 => abc(23, a, b, c),
                5 => abc(23, a, b | 256, c),
                56 => abc(24, a, b, c),
                57 => abc(24, a, b | 256, c),
                58 => abc(25, a, b, c),
                59 => abc(25, a, b | 256, c),
                52 => abc(10, a, b, c),
                53 => abc(18, a, b, 0),
                54 | 72 => abc(19, a, b, 0),
                55 => abc(20, a, b, 0),
                60 => abc(21, a, b, c),
                28 => {
                    let target = pc as i64 + 1 + bx as i64 - 65535;
                    ensure!(
                        target >= 0 && target < f.code.len() as i64,
                        "HKS jump leaves function"
                    );
                    relocations.push((code.len(), target as usize, 22, a));
                    0
                }
                14 => abc(33, a, 0, c),
                64 => {
                    ensure!(c != 0, "Extended HKS SETLIST is unsupported");
                    abc(34, a, b, c)
                }
                65 => abc(35, a, 0, 0),
                67 => abc(37, a, b, 0),
                66 => {
                    let child = f
                        .children
                        .get(bx as usize)
                        .context("Invalid HKS child function index")?;
                    bindings = child.up as usize;
                    abx(36, a, bx)
                }
                _ => bail!("Unsupported HavokScript opcode {op} at {name}:{pc}"),
            };
            code.push(instruction);
            if matches!(op, 1 | 4 | 5 | 14 | 56..=59 | 61 | 71) {
                // Explicit successors keep skip semantics intact when the next
                // original operation expands into several host instructions.
                ensure!(pc + 2 < f.code.len(), "HKS conditional leaves function");
                relocations.push((code.len(), pc + 1, 22, 0));
                code.push(0);
                relocations.push((code.len(), pc + 2, 22, 0));
                code.push(0);
            } else if op == 13 && c != 0 {
                ensure!(pc + 2 < f.code.len(), "HKS Boolean skip leaves function");
                relocations.push((code.len(), pc + 2, 22, 0));
                code.push(0);
            }
        }
        if op != 76 {
            cache_data = matches!(op, 0 | 10 | 11 | 12 | 73 | 88 | 91);
        }
        source_lines.extend(std::iter::repeat_n(
            f.lines.get(pc).copied().unwrap_or(0),
            code.len() - start,
        ));
    }
    offsets[f.code.len()] = code.len();
    ensure!(bindings == 0, "Truncated HKS closure captures");
    for (at, target, op, a) in relocations {
        let delta = offsets[target] as i64 - at as i64 - 1;
        ensure!(
            (-131071..=131072).contains(&delta),
            "Translated HKS jump exceeds host range"
        );
        code[at] = abx(op, a, (delta + 131071) as u32);
    }
    string(out, &f.source)?;
    word(out, 0);
    word(out, 0);
    out.extend([f.up, f.params, f.vararg, stack as u8]);
    word(out, u32::try_from(code.len())?);
    for v in code {
        word(out, v)
    }
    word(out, u32::try_from(f.constants.len() + extras.len())?);
    for c in &f.constants {
        match c {
            Constant::Nil => out.push(0),
            Constant::Boolean(b) => out.extend([1, *b]),
            Constant::Number(n) => {
                out.push(3);
                out.extend(n.to_le_bytes())
            }
            Constant::String(s) => {
                out.push(4);
                word(out, u32::try_from(s.len() + 1)?);
                out.extend(s);
                out.push(0)
            }
        }
    }
    for extra in extras {
        out.push(4);
        string(out, extra.as_bytes())?;
    }
    word(out, u32::try_from(f.children.len())?);
    for child in &f.children {
        emit_function(child, out)?;
    }
    if f.lines.is_empty() {
        word(out, 0)
    } else {
        word(out, u32::try_from(source_lines.len())?);
        for line in source_lines {
            word(out, line)
        }
    }
    word(out, 0);
    word(out, 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(little: bool, opcode: u32) -> Vec<u8> {
        let mut b = b"\x1bLua\x51\x0d\x01\x04\x04\x04\x04\0\0\0".to_vec();
        b[6] = little as u8;
        let put = |b: &mut Vec<u8>, v: u32| {
            b.extend(if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            })
        };
        put(&mut b, 0);
        put(&mut b, 0);
        put(&mut b, 0);
        b.push(0);
        put(&mut b, 2);
        put(&mut b, 2);
        while b.len() % 4 != 0 {
            b.push(0)
        }
        put(&mut b, opcode << 25);
        put(&mut b, (9 << 25) | (2 << 17));
        put(&mut b, 1);
        b.push(3);
        put(&mut b, 0.1f32.to_bits());
        put(&mut b, 0);
        put(&mut b, 0);
        b
    }
    #[test]
    fn empty_string_constant_is_not_a_null_string_pointer() -> Result<()> {
        let f = Function {
            up: 0,
            params: 0,
            vararg: 0,
            stack: 2,
            code: vec![25 << 25, (9 << 25) | (2 << 17)],
            constants: vec![Constant::String(Vec::new())],
            children: Vec::new(),
            source: Vec::new(),
            lines: Vec::new(),
        };
        let mut chunk = b"\x1bLua\x51\0\x01\x04\x04\x04\x08\0".to_vec();
        emit_function(&f, &mut chunk)?;
        let lua = mlua::Lua::new();
        let value: mlua::String = lua.load(crate::lua_cook::host_chunk(&chunk)?).eval()?;
        assert!(value.as_bytes().is_empty());
        lua.gc_collect()?;
        Ok(())
    }
    #[test]
    fn both_endians_execute_and_every_truncated_prefix_is_rejected() -> Result<()> {
        for little in [false, true] {
            let bytes = fixture(little, 25);
            let lua = mlua::Lua::new();
            let chunk = crate::lua_cook::host_chunk(&bytes)?;
            let value: f64 = lua.load(&chunk).eval()?;
            assert_eq!(value, 0.1f32 as f64);
            for end in 0..bytes.len() {
                assert!(translate(&bytes[..end]).is_err(), "accepted prefix {end}");
            }
        }
        assert!(translate(&fixture(true, 75))
            .unwrap_err()
            .to_string()
            .contains("opcode 75"));
        Ok(())
    }
    #[test]
    fn arithmetic_helpers_round_f32_and_preserve_metamethods() -> Result<()> {
        let lua = mlua::Lua::new();
        install(&lua)?;
        lua.load(
            r#"
          assert(__blotworx_hks_add(16777216,1)==16777216)
          local t=setmetatable({},{__add=function(a,b)return 'metamethod' end})
          assert(__blotworx_hks_add(t,1)=='metamethod')
          assert(__blotworx_hks_mod(-3,2)==1)
        "#,
        )
        .exec()?;
        Ok(())
    }
    #[test]
    #[ignore = "requires original PC tutorial chunk in BLOTWORX_HKS_TUTORIAL"]
    fn original_tutorial_branches_waits_upvalues_and_f32_fades() -> Result<()> {
        let path =
            std::env::var_os("BLOTWORX_HKS_TUTORIAL").context("Set BLOTWORX_HKS_TUTORIAL")?;
        let bytes = std::fs::read(path)?;
        let lua = mlua::Lua::new();
        install(&lua)?;
        lua.load(crate::lua_cook::host_chunk(&bytes)?).exec()?;
        lua.load(r#"
            calls={};flags={};PLAYER_ONE=0;PLAYER_TWO=1
            function record(name,...) calls[#calls+1]={name,...} end
            function GetPlayer() return 'fixture-player' end
            function GetGlobal(name)return flags[name] or 0 end
            function SetGlobal(name,value) record('SetGlobal',name,value);flags[name]=value end
            for _,name in ipairs({'EnablePlayer','EnableSplitScreen','PlayMovie','StartFadeIn','HideHud',
                'SetCursorOnly','UnlockAbility','HidePlayerTool','PlayerSetDripsVisible','SetDefaultCameraValues',
                'SetTravelCameraValues','LockAbility','SetCurrencyMax','GiveCollectibleNoPopup','Unmute',
                'StartSweetMusic','SaveCheckpoint','SetPropertyFloat','SetActiveSubMap','SetActiveFullMap'}) do
                local method=name;_G[method]=function(...)record(method,...)end
            end
            function wait(seconds) record('wait',seconds);return coroutine.yield(seconds) end
            YS_SlowMickey();YS_RegularMickey();assert(calls[1][4]==2.5 and calls[2][4]==5)
            calls={};YSL_SetActiveMap('map','both');assert(#calls==2 and calls[2][1]=='SetActiveFullMap')
            calls={};YSL_SetActiveMap('map','sub');assert(#calls==1 and calls[1][1]=='SetActiveSubMap')
            calls={}
        "#).exec()?;
        let task = lua.create_thread(lua.globals().get::<mlua::Function>("Tut_LevelSetup")?)?;
        assert_eq!(task.resume::<f64>("setup-owner")?, 0.);
        assert_eq!(task.resume::<f64>(())?, 0.1f32 as f64);
        task.resume::<()>(())?;
        lua.load(r#"
            local names={};for i,c in ipairs(calls)do names[i]=c[1] end
            assert(table.concat(names,',')=='SetGlobal,EnablePlayer,EnableSplitScreen,PlayMovie,StartFadeIn,HideHud,SetCursorOnly,UnlockAbility,UnlockAbility,HidePlayerTool,PlayerSetDripsVisible,SetPropertyFloat,SetDefaultCameraValues,SetTravelCameraValues,LockAbility,LockAbility,LockAbility,SetCurrencyMax,SetCurrencyMax,SetCurrencyMax,SetCurrencyMax,GiveCollectibleNoPopup,wait,Unmute,StartSweetMusic,wait,SaveCheckpoint')
            assert(calls[4][3]=='fmv_intro.bik')
            assert(calls[12][4]==2.5)
            assert(calls[27][3]=='YSL_CheckpointReload' and calls[27][4]=='V1ReloadTrigLoc')
            local count=#calls;Tut_LevelSetup('setup-owner');assert(#calls==count)
            calls={}
            function GetPropertyEntity() return 'parent' end
            function GetPropertyFloat() return 2 end
            function AnimVarFloat(...)record('AnimVarFloat',...)end
            function AnimEvent(...)record('AnimEvent',...)end
        "#).exec()?;
        let bounce = lua.globals().get::<mlua::Function>("YS_BounceReact")?;
        let one = lua.create_thread(bounce.clone())?;
        assert_eq!(one.resume::<f64>(("broom", "normal"))?, 1.);
        let two = lua.create_thread(bounce.clone())?;
        two.resume::<()>(("broom-two", "normal"))?;
        assert_eq!(two.status(), mlua::ThreadStatus::Finished);
        lua.load("assert(#calls==2 and calls[1][1]=='AnimVarFloat' and calls[2][1]=='wait')")
            .exec()?;
        one.resume::<()>(())?;
        let three = lua.create_thread(bounce)?;
        assert_eq!(three.resume::<f64>(("broom", "normal"))?, 1.);
        three.resume::<()>(())?;
        lua.load(
            r#"
            alphas={}
            function Print()end
            function StartEmitters()end
            function StopEmitters()end
            function vector4(x,y,z,w)return {x=x,y=y,z=z,w=w}end
            function SetR3MTConstReg(target,index,color)alphas[#alphas+1]=color.w end
        "#,
        )
        .exec()?;
        for paint in [true, false] {
            lua.load("alphas={}").exec()?;
            let task = lua.create_thread(lua.globals().get::<mlua::Function>("YSL_PaintStars")?)?;
            task.resume::<mlua::Value>(("stars", if paint { "paint" } else { "thin" }))?;
            let mut resumes = 0;
            while task.status() == mlua::ThreadStatus::Resumable {
                task.resume::<mlua::Value>(())?;
                resumes += 1;
                ensure!(resumes < 100, "Test fade did not terminate");
            }
            let actual = lua
                .globals()
                .get::<mlua::Table>("alphas")?
                .sequence_values::<f64>()
                .collect::<mlua::Result<Vec<_>>>()?;
            let step = if paint { 0.0333f32 } else { -0.0333f32 };
            let mut i = if paint { 0f32 } else { 1f32 };
            let mut expected = Vec::new();
            while if paint { i <= 1. } else { i >= 0. } {
                expected.push(i as f64);
                i += step;
            }
            assert_eq!(
                actual, expected,
                "Original fade must retain f32 loop progression"
            );
        }
        Ok(())
    }
}
