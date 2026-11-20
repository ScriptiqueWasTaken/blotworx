//! C-locale decimal integer conversion DOL8001c084. Overflow saturates and
//! writes ERANGE34; successful/no-digit conversion preserves the prior errno.
//! The caller must diagnose a null C pointer rather than invent an empty string.
pub fn atoi(text:&[u8],errno:&mut i32)->i32 {
    let mut index=0;
    while text.get(index).is_some_and(|b|matches!(*b,b' '|b'\t'|b'\n'|b'\x0b'|b'\x0c'|b'\r')) {index+=1;}
    let negative=text.get(index)==Some(&b'-');
    if text.get(index).is_some_and(|b|matches!(*b,b'+'|b'-')) {index+=1;}
    let mut magnitude=0u64;
    while let Some(&byte)=text.get(index).filter(|b|b.is_ascii_digit()) {
        magnitude=magnitude.saturating_mul(10).saturating_add((byte-b'0') as u64);index+=1;
    }
    let limit=if negative{2147483648}else{2147483647};
    if magnitude>limit {*errno=34;return if negative{i32::MIN}else{i32::MAX};}
    if negative {-(magnitude as i64) as i32}else{magnitude as i32}
}
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::Value;
    #[test]
    fn decimal_integer_and_errno_match_original_complete_conversion()->Result<()> {
        let fixture:Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/native_atoi.json"))?;
        for case in fixture["cases"].as_array().unwrap() {
            let text=case["input"]["bytes"].as_array().unwrap().iter().map(|v|v.as_u64().unwrap() as u8).collect::<Vec<_>>();
            let mut errno=case["input"]["errno"].as_i64().unwrap() as i32;
            let value=atoi(&text,&mut errno);
            assert_eq!((value as i64,errno as i64),(case["value"].as_i64().unwrap(),case["errno"].as_i64().unwrap()),"{text:?}");
        }
        Ok(())
    }
}
