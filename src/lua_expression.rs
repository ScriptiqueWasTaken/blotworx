//! Recognize the parenthesized operator expressions emitted by value blocks.
//! Unsupported expressions remain ordinary Lua without normalization.
use std::ops::Range;
pub const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%", "==", "~=", "<", "<=", ">", ">=", "and", "or",
];
pub struct Split {
    pub left: Range<usize>,
    pub right: Range<usize>,
    pub operator: &'static str,
}
pub fn split(source: &str) -> Option<Split> {
    let text = source.trim();
    if !text.starts_with('(') || !text.ends_with(')') {
        return None;
    }
    let offset = source.len() - source.trim_start().len();
    let mut mask = vec![true; text.len()];
    for (range, kind) in crate::lua_syntax::tokens(text) {
        if matches!(
            kind,
            crate::lua_syntax::Kind::String
                | crate::lua_syntax::Kind::Comment
                | crate::lua_syntax::Kind::Number
        ) {
            mask[range].fill(false);
        }
    }
    let mut depth = 0;
    let mut found = None;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !mask[i] {
            i += 1;
            continue;
        }
        match bytes[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 && i != bytes.len() - 1 {
                    return None;
                }
            }
            _ if depth == 1 => {
                let operator = [
                    "==", "~=", "<=", ">=", "and", "or", "+", "-", "*", "/", "%", "<", ">",
                ]
                .into_iter()
                .find(|op| {
                    bytes[i..].starts_with(op.as_bytes())
                        && (!op.as_bytes()[0].is_ascii_alphabetic()
                            || (i > 0
                                && bytes[i - 1].is_ascii_whitespace()
                                && bytes.get(i + op.len()).is_some_and(u8::is_ascii_whitespace)))
                });
                if let Some(operator) = operator {
                    if operator == "-" && text[1..i].trim().is_empty() {
                        i += 1;
                        continue;
                    }
                    if found.is_some() {
                        return None;
                    }
                    found = Some((i, operator));
                    i += operator.len();
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let (position, operator) = found?;
    let left = offset + 1..offset + position;
    let right = offset + position + operator.len()..offset + text.len() - 1;
    if source[left.clone()].trim().is_empty() || source[right.clone()].trim().is_empty() {
        return None;
    }
    Some(Split {
        left,
        right,
        operator,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_and_quoted_inputs_keep_correct_operator_boundaries() {
        let source = " ((1 + 2) * (4 - 1)) ";
        let parsed = split(source).unwrap();
        assert_eq!(parsed.operator, "*");
        assert_eq!(source[parsed.left].trim(), "(1 + 2)");
        assert_eq!(source[parsed.right].trim(), "(4 - 1)");
        assert_eq!(split("(\"a+b\" == \"a+b\")").unwrap().operator, "==");
        assert!(split("(1 + 2) * (3 + 4)").is_none());
        assert!(split("(1 + 2 + 3)").is_none());
        assert!(split("(-1)").is_none());
        assert!(split("(1e-2)").is_none());
        assert_eq!(split("(1e-2 + 3)").unwrap().operator, "+");
        let value: i32 = mlua::Lua::new()
            .load(format!("return {source}"))
            .eval()
            .unwrap();
        assert_eq!(value, 9);
    }
}
