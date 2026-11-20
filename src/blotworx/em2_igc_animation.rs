//! Original PC af3520 operation ordering; independent string-validity guards.
#[derive(Debug,PartialEq,Eq)]
pub(super) enum Operation {Link,Integer,Float,Event}
pub(super) fn operations(flags:u32,valid:[bool;5])->Vec<Operation> {
    let mut result=Vec::new();
    if flags&8!=0&&valid[0]&&valid[1]{result.push(Operation::Link);}
    if flags&2!=0&&valid[3]{result.push(Operation::Integer);}
    if flags&4!=0&&valid[4]{result.push(Operation::Float);}
    if flags&1!=0&&valid[2]{result.push(Operation::Event);}
    result
}
