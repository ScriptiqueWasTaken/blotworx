//! Owned Rust boundary for the bundled, unmodified MojoShader translator.
//! Translation preserves its binding metadata; it is not an effect pass or draw.
use anyhow::{ensure, Context, Result};
use std::{ffi::{c_char,c_void,CStr},ptr::NonNull};
unsafe extern "C" {
    fn bw_shader_translate(data:*const u8,size:u32)->*const c_void;
    fn bw_shader_translation_free(handle:*const c_void);
    fn bw_shader_translation_errors(handle:*const c_void)->i32;
    fn bw_shader_has_preshader(handle:*const c_void)->i32;
    fn bw_shader_constant_count(handle:*const c_void)->i32;
    fn bw_shader_constant(handle:*const c_void,slot:i32,kind:*mut i32,index:*mut i32,words:*mut u32)->i32;
    fn bw_shader_translation_error(handle:*const c_void,index:i32)->*const c_char;
    fn bw_shader_translation_source(handle:*const c_void,size:*mut u32)->*const c_char;
    fn bw_shader_binding_count(handle:*const c_void,group:i32)->i32;
    fn bw_shader_binding(handle:*const c_void,group:i32,slot:i32,kind:*mut i32,index:*mut i32,count:*mut i32,constant:*mut i32,name:*mut *const c_char)->i32;
}
#[derive(Debug)]
struct Native(NonNull<c_void>);
impl Drop for Native {fn drop(&mut self){unsafe {bw_shader_translation_free(self.0.as_ptr())}}}
#[derive(Debug,serde::Serialize)]
pub(super) struct Binding {
    /// Uniform type, sampler dimensionality or attribute semantic, by group.
    pub kind:i32,
    pub index:i32,
    pub count:i32,
    pub constant:i32,
    pub name:Option<String>,
}
#[derive(Debug)]
pub(super) struct Translation {
    pub source:String,
    pub uniforms:Vec<Binding>, pub samplers:Vec<Binding>,
    pub inputs:Vec<Binding>, pub outputs:Vec<Binding>,
    pub has_preshader:bool,
    /// Exact original CPU programs, separated from GPU code. Evaluation and
    /// effect-parameter mapping are required before submitting a game draw.
    pub preshaders:Vec<Vec<u8>>,
    pub cpu_programs:Vec<Result<super::d3d9_preshader::Program,String>>,
    pub constants:Vec<Constant>,
}
#[derive(Debug)]
pub(super) struct Constant {pub kind:i32,pub index:i32,pub words:[u32;4]}
#[derive(Debug,Default)]
pub(super) struct PackedUniforms {pub floats:Vec<f32>,pub integers:Vec<i32>,pub booleans:Vec<i32>}
fn split_cpu_programs(bytecode:&[u8])->Result<(Vec<u8>,Vec<Vec<u8>>)> {
    ensure!(bytecode.len()>=8&&bytecode.len()%4==0,"Invalid original shader size");
    let mut gpu=bytecode[..4].to_vec();let mut preshaders=Vec::new();let mut cursor=4;
    while cursor<bytecode.len() {
        let token=u32::from_le_bytes(bytecode[cursor..cursor+4].try_into()?);
        let opcode=token&0xffff;
        let parameters=if opcode==0xfffe {(token>>16)&0x7fff}else if opcode==0xffff {0}else{(token>>24)&15};
        let end=cursor.checked_add(4+parameters as usize*4).context("Shader CPU/GPU split overflow")?;
        let instruction=bytecode.get(cursor..end).context("Truncated shader CPU/GPU instruction")?;
        if opcode==0xfffe&&instruction.get(4..8)==Some(b"PRES") {
            preshaders.push(instruction.to_vec());
        }else{gpu.extend_from_slice(instruction);}
        cursor=end;
        if opcode==0xffff {ensure!(cursor==bytecode.len(),"Shader data after terminator");return Ok((gpu,preshaders));}
    }
    anyhow::bail!("Original shader has no terminator")
}
fn text(value:*const c_char)->Result<Option<String>> {
    if value.is_null(){return Ok(None);}
    Ok(Some(unsafe {CStr::from_ptr(value)}.to_str()?.to_owned()))
}
impl Translation {
    /// GLSL compacts original registers in reflection order. c17 is not
    /// necessarily array element 17. DEF values override even array lanes.
    pub fn pack_uniforms(&self,floats:&[f32],integers:&[i32],booleans:&[u32],packed:&mut PackedUniforms)->Result<()> {
        packed.floats.clear();packed.integers.clear();packed.booleans.clear();
        for uniform in &self.uniforms {
            if uniform.constant!=0 {continue;}
            let first=usize::try_from(uniform.index).context("Negative shader uniform register")?;
            let count=usize::try_from(uniform.count).context("Negative shader uniform count")?.max(1);
            let end=first.checked_add(count).context("Shader uniform range overflow")?;
            let vector_range=||->Result<std::ops::Range<usize>> {Ok(first.checked_mul(4).context("Shader register offset overflow")?..end.checked_mul(4).context("Shader register end overflow")?)};
            let (base,width)=match uniform.kind {
                0=>{let base=packed.floats.len();packed.floats.extend_from_slice(floats.get(vector_range()?).context("Float shader uniform outside bank")?);(base,4)},
                1=>{let base=packed.integers.len();packed.integers.extend_from_slice(integers.get(vector_range()?).context("Integer shader uniform outside bank")?);(base,4)},
                2=>{let base=packed.booleans.len();packed.booleans.extend(booleans.get(first..end).context("Boolean shader uniform outside bank")?.iter().map(|v|(*v!=0) as i32));(base,1)},
                _=>anyhow::bail!("Unknown shader uniform type"),
            };
            for constant in &self.constants {
                if constant.kind!=uniform.kind {continue;}
                let index=usize::try_from(constant.index).context("Negative shader DEF register")?;
                if !(first..end).contains(&index){continue;}
                let offset=base+(index-first)*width;
                for lane in 0..width {
                    match uniform.kind {
                        0=>packed.floats[offset+lane]=f32::from_bits(constant.words[lane]),
                        1=>packed.integers[offset+lane]=constant.words[lane] as i32,
                        2=>packed.booleans[offset]=(constant.words[0]!=0) as i32,
                        _=>unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }
    /// GLSL ES3 and desktop330 share the generated explicit input/output
    /// interface. Keep all generated operations and register expressions.
    pub fn desktop_source(&self)->Result<String> {
        let body=self.source.strip_prefix("#version 300 es").context("Unexpected translator GLSL profile")?;
        Ok(format!("#version 330 core{body}"))
    }
    pub fn parse(bytecode:&[u8])->Result<Self> {
        let (gpu,preshaders)=split_cpu_programs(bytecode)?;
        let size=u32::try_from(gpu.len()).context("Original shader exceeds translator size range")?;
        let native=Native(NonNull::new(unsafe {bw_shader_translate(gpu.as_ptr(),size)} as *mut c_void).context("Shader translator allocation failed")?);
        let handle=native.0.as_ptr();let errors=unsafe {bw_shader_translation_errors(handle)};
        ensure!(errors>=0,"Invalid shader translator result");
        if errors!=0 {
            let messages=(0..errors).map(|index|text(unsafe {bw_shader_translation_error(handle,index)}).map(|s|s.unwrap_or_else(||"Unknown shader translation error".into()))).collect::<Result<Vec<_>>>()?;
            anyhow::bail!("Original shader translation: {}",messages.join("; "));
        }
        let mut length=0;let pointer=unsafe {bw_shader_translation_source(handle,&mut length)};
        ensure!(!pointer.is_null(),"Shader translator returned no source");
        let source=std::str::from_utf8(unsafe {std::slice::from_raw_parts(pointer as *const u8,length as usize)})?.to_owned();
        let bindings=|group|->Result<Vec<Binding>> {
            let count=unsafe {bw_shader_binding_count(handle,group)};
            ensure!(count>=0,"Shader translator returned invalid binding count");
            let mut bindings=Vec::with_capacity(count as usize);
            for slot in 0..count {
                let (mut kind,mut index,mut count,mut constant,mut name)=(0,0,0,0,std::ptr::null());
                ensure!(unsafe {bw_shader_binding(handle,group,slot,&mut kind,&mut index,&mut count,&mut constant,&mut name)}!=0,"Shader binding disappeared during translation");
                bindings.push(Binding {kind,index,count,constant,name:text(name)?});
            }
            Ok(bindings)
        };
        let cpu_programs=preshaders.iter().map(|bytes|super::d3d9_preshader::Program::parse(bytes).map_err(|e|format!("{e:#}"))).collect();
        let count=unsafe {bw_shader_constant_count(handle)};ensure!(count>=0,"Invalid shader DEF count");
        let mut constants=Vec::with_capacity(count as usize);
        for slot in 0..count {
            let mut kind=0;let mut index=0;let mut words=[0;4];
            ensure!(unsafe {bw_shader_constant(handle,slot,&mut kind,&mut index,words.as_mut_ptr())}!=0,"Shader DEF disappeared during translation");
            constants.push(Constant {kind,index,words});
        }
        // Every returned value is an owned Rust copy. Release the native parse
        // allocation here instead of retaining a duplicate source/metadata
        // graph per cached stage or carrying a raw pointer into render jobs.
        Ok(Self {source,uniforms:bindings(0)?,samplers:bindings(1)?,inputs:bindings(2)?,outputs:bindings(3)?,
            has_preshader:!preshaders.is_empty(),preshaders,cpu_programs,constants})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn bytes(words:&[u32])->Vec<u8> {words.iter().flat_map(|w|w.to_le_bytes()).collect()}

    fn reflected(uniforms:Vec<Binding>,constants:Vec<Constant>)->Translation {
        Translation {source:String::new(),uniforms,samplers:vec![],inputs:vec![],outputs:vec![],has_preshader:false,preshaders:vec![],cpu_programs:vec![],constants}
    }
    fn uniform(kind:i32,index:i32,count:i32)->Binding {Binding {kind,index,count,constant:0,name:None}}
    #[test]
    fn sparse_registers_are_compacted_and_shader_defs_override_array_lanes()->Result<()> {
        let t=reflected(vec![uniform(0,17,0),uniform(0,3,3),uniform(1,5,0),uniform(2,2,0)],vec![Constant {kind:0,index:4,words:[99f32.to_bits();4]},Constant {kind:1,index:5,words:[u32::MAX,2,3,4]},Constant {kind:2,index:2,words:[5,0,0,0]}]);
        let floats=(0..72).map(|v|v as f32).collect::<Vec<_>>();let mut packed=PackedUniforms::default();
        t.pack_uniforms(&floats,&[0;24],&[0;3],&mut packed)?;
        assert_eq!(&packed.floats[..4],&floats[68..72]);assert_eq!(&packed.floats[4..8],&floats[12..16]);
        assert_eq!(&packed.floats[8..12],&[99.;4]);assert_eq!(&packed.floats[12..],&floats[20..24]);
        assert_eq!(packed.integers,[-1,2,3,4]);assert_eq!(packed.booleans,[1]);
        t.pack_uniforms(&floats,&[0;24],&[0;3],&mut packed)?;assert_eq!(packed.floats.len(),16);Ok(())
    }
    #[test]
    fn invalid_uniform_banks_fail_before_gpu_submission() {
        let t=reflected(vec![uniform(0,i32::MAX,1)],vec![]);assert!(t.pack_uniforms(&[],&[],&[],&mut PackedUniforms::default()).is_err());
        let t=reflected(vec![uniform(0,-1,0)],vec![]);assert!(t.pack_uniforms(&[],&[],&[],&mut PackedUniforms::default()).is_err());
    }
    #[test]
    fn cpu_program_is_retained_without_entering_native_preshader_parser()->Result<()> {
        fn assert_send_sync<T:Send+Sync>(){}
        assert_send_sync::<Translation>();
        let cpu=[0x0004fffe,u32::from_le_bytes(*b"PRES"),0x46580201,0x70e00001,6];
        let mut words=vec![0xffff0300];words.extend(cpu);
        words.extend([0x05000051,0xa00f0000,1f32.to_bits(),0,0,1f32.to_bits(),
            0x02000001,0x800f0800,0xa0e40000,0x0000ffff]);
        let translated=Translation::parse(&bytes(&words))?;
        assert!(translated.has_preshader);assert_eq!(translated.preshaders,[bytes(&cpu)]);
        assert!(translated.desktop_source()?.starts_with("#version 330 core"));
        assert!(!translated.source.contains("70e0"));Ok(())
    }
    #[test]
    fn incomplete_comment_is_rejected_before_native_translation() {
        assert!(Translation::parse(&bytes(&[0xffff0300,0x00fffffe,u32::from_le_bytes(*b"PRES")])).is_err());
        assert!(Translation::parse(&[]).is_err());
    }
}
