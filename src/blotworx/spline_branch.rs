//! Original forward branch choice3bbbc4..3bbc2c. Full spline geometry/time,
//! reverse traversal and native controller scheduling remain separate work.
pub(super) fn select(current:usize,normal:Option<usize>,branch:Option<usize>,chance:f32,random:u32)->(usize,bool) {
    let sample=random as f32/32767.;
    if let Some(branch)=branch {if sample<chance{return (branch,true);}}
    (normal.unwrap_or(current),false)
}
#[cfg(debug_assertions)]
pub(super) fn validate()->anyhow::Result<()> {
    let reference:serde_json::Value=serde_json::from_str(include_str!("../../tools/blotworx/fixtures/spline_branch.json"))?;
    for c in reference["cases"].as_array().unwrap() {
        let actual=select(11,c["normal"].as_bool().unwrap().then_some(33),c["branch"].as_bool().unwrap().then_some(22),c["chance"].as_f64().unwrap() as f32,c["random"].as_u64().unwrap() as u32);
        anyhow::ensure!(actual==(c["selected"].as_u64().unwrap() as usize,c["branched"].as_bool().unwrap()),"Native spline branch differs: {c}");
    }
    println!("PASS:100 native spline branch cases, including strict endpoint and missing-target choices");Ok(())
}
