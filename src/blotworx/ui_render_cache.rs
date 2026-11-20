// Exact immutable vector reuse and identical-target reuse for the HUD.
// Every movie frame runs. No hashing, quantization or time-based throttling.
mod identity {include!("ui_frame_identity.rs");}
use ruffle_render::{backend::*,bitmap::*,commands::CommandList,
    error::Error,filters::Filter,
    pixel_bender::{PixelBenderShader,PixelBenderShaderHandle},pixel_bender_support::PixelBenderShaderArgument,
    quality::StageQuality,shape_utils::DistilledShape};
use std::{borrow::Cow,num::NonZeroU32,sync::Arc};
// Keep only registrations from the preceding/current frame. No persistent
// catalogue of every animated morph ratio. Bitmap fills are excluded because
// BitmapSource can resolve different resources for the same character ID.
#[derive(Clone,PartialEq)]enum Edge {
    Move(swf::Point<swf::Twips>),Line(swf::Point<swf::Twips>),
    Quadratic(swf::Point<swf::Twips>,swf::Point<swf::Twips>),
    Cubic(swf::Point<swf::Twips>,swf::Point<swf::Twips>,swf::Point<swf::Twips>),
}
#[derive(Clone,PartialEq)]enum PathKey {
    Fill(swf::FillStyle,Vec<Edge>,ruffle_render::shape_utils::FillRule),
    Stroke(swf::LineStyle,Vec<Edge>,bool),
}
#[derive(Clone,PartialEq)]struct ShapeKey {
    paths:Vec<PathKey>,bounds:swf::Rectangle<swf::Twips>,edges:swf::Rectangle<swf::Twips>,id:u16,scale:u32,scaled_registration:bool,
}
impl ShapeKey {
    fn new(shape:&DistilledShape,scale:Option<f32>)->Option<Self>{
        let scaled_registration=scale.is_some();let scale=scale.unwrap_or(1.);
        use ruffle_render::shape_utils::{DrawCommand as E,DrawPath};
        if !scale.is_finite(){return None;}
        let edges=|commands:&[E]|commands.iter().map(|e|match e {
            E::MoveTo(p)=>Edge::Move(*p),E::LineTo(p)=>Edge::Line(*p),
            E::QuadraticCurveTo{control,anchor}=>Edge::Quadratic(*control,*anchor),
            E::CubicCurveTo{control_a,control_b,anchor}=>Edge::Cubic(*control_a,*control_b,*anchor),
        }).collect();
        let mut paths=Vec::with_capacity(shape.paths.len());
        for path in &shape.paths {paths.push(match path {
            DrawPath::Fill{style,commands,winding_rule}=>{
                if matches!(style,swf::FillStyle::Bitmap{..}){return None;}
                PathKey::Fill((*style).clone(),edges(commands),*winding_rule)
            },
            DrawPath::Stroke{style,commands,is_closed}=>{
                if matches!(style.fill_style(),swf::FillStyle::Bitmap{..}){return None;}
                PathKey::Stroke((*style).clone(),edges(commands),*is_closed)
            },
        });}
        Some(Self{paths,bounds:shape.shape_bounds,edges:shape.edge_bounds,id:shape.id,scale:scale.to_bits(),scaled_registration})
    }
}
struct ShapeEntry {key:ShapeKey,handle:ShapeHandle}
pub struct Cached<B> {
    pub inner:B,pub enabled:bool,pub submitted:u64,pub identical_frames:u64,
    last_frame:Option<(swf::Color,CommandList)>,external_gpu_mutation:bool,
    previous_shapes:Vec<ShapeEntry>,current_shapes:Vec<ShapeEntry>,pub shape_requests:u64,pub shape_reused:u64,
}
impl<B> Cached<B> {
    pub fn new(inner:B)->Self{Self{inner,enabled:true,submitted:0,identical_frames:0,last_frame:None,external_gpu_mutation:false,previous_shapes:Vec::new(),current_shapes:Vec::new(),shape_requests:0,shape_reused:0}}
}
impl<B:RenderBackend> Cached<B> {
    fn shape(&mut self,shape:DistilledShape,bitmaps:&dyn BitmapSource,scale:Option<f32>)->ShapeHandle {
        self.shape_requests+=1;
        let key=self.enabled.then(||ShapeKey::new(&shape,scale)).flatten();
        if let Some(key)=&key {
            if let Some(entry)=self.current_shapes.iter().find(|e|e.key==*key){self.shape_reused+=1;return entry.handle.clone();}
            if let Some(entry)=self.previous_shapes.iter().find(|e|e.key==*key){
                let handle=entry.handle.clone();self.shape_reused+=1;
                self.current_shapes.push(ShapeEntry{key:key.clone(),handle:handle.clone()});return handle;
            }
        }
        let handle=if let Some(scale)=scale{self.inner.register_shape_with_scale(shape,bitmaps,scale)}else{self.inner.register_shape(shape,bitmaps)};
        if let Some(key)=key{self.current_shapes.push(ShapeEntry{key,handle:handle.clone()});}
        handle
    }
}
impl<B:RenderBackend> RenderBackend for Cached<B> {
    fn viewport_dimensions(&self)->ViewportDimensions{self.inner.viewport_dimensions()}
    fn set_viewport_dimensions(&mut self,d:ViewportDimensions){self.last_frame=None;self.inner.set_viewport_dimensions(d)}
    fn register_shape(&mut self,s:DistilledShape,b:&dyn BitmapSource)->ShapeHandle{self.shape(s,b,None)}
    fn register_shape_with_scale(&mut self,s:DistilledShape,b:&dyn BitmapSource,scale:f32)->ShapeHandle{self.shape(s,b,Some(scale))}
    fn render_offscreen(&mut self,h:BitmapHandle,c:CommandList,q:StageQuality,b:PixelRegion)->Option<Box<dyn SyncHandle>>{
        self.last_frame=None;self.inner.render_offscreen(h,c,q,b)
    }
    fn apply_filter(&mut self,s:BitmapHandle,p:(u32,u32),z:(u32,u32),d:BitmapHandle,dp:(i32,i32),f:Filter)->Option<Box<dyn SyncHandle>>{
        self.last_frame=None;self.inner.apply_filter(s,p,z,d,dp,f)
    }
    fn is_filter_supported(&self,f:&Filter)->bool{self.inner.is_filter_supported(f)}
    fn is_offscreen_supported(&self)->bool{self.inner.is_offscreen_supported()}
    fn submit_frame(&mut self,clear:swf::Color,commands:CommandList,cache:Vec<BitmapCacheEntry>){
        self.previous_shapes=std::mem::take(&mut self.current_shapes);
        // Any cache render may mutate a bitmap used by an otherwise identical
        // command list. Such frames must always execute, as must Stage3D and
        // custom shader operations whose mutations are owned outside this API.
        let retain=self.enabled && !self.external_gpu_mutation && cache.is_empty()
            && identity::commands(&commands,&commands);
        let identical=retain && self.last_frame.as_ref().is_some_and(|(old_clear,old)|
            *old_clear==clear && identity::commands(old,&commands));
        if identical {self.identical_frames+=1;return;}
        self.last_frame=retain.then(||(clear,commands.clone()));
        self.inner.submit_frame(clear,commands,cache);self.submitted+=1;
    }
    fn create_empty_texture(&mut self,w:NonZeroU32,h:NonZeroU32)->Result<BitmapHandle,Error>{self.inner.create_empty_texture(w,h)}
    fn register_bitmap(&mut self,b:Bitmap<'_>)->Result<BitmapHandle,Error>{self.inner.register_bitmap(b)}
    fn update_texture(&mut self,h:&BitmapHandle,b:Bitmap<'_>,r:PixelRegion)->Result<(),Error>{self.last_frame=None;self.inner.update_texture(h,b,r)}
    fn create_context3d(&mut self,p:Context3DProfile)->Result<Box<dyn Context3D>,Error>{self.last_frame=None;self.external_gpu_mutation=true;self.inner.create_context3d(p)}
    fn debug_info(&self)->Cow<'static,str>{self.inner.debug_info()}
    fn name(&self)->&'static str{self.inner.name()}
    fn set_quality(&mut self,q:StageQuality){self.last_frame=None;self.previous_shapes.clear();self.current_shapes.clear();self.inner.set_quality(q)}
    fn compile_pixelbender_shader(&mut self,s:PixelBenderShader)->Result<PixelBenderShaderHandle,Error>{self.inner.compile_pixelbender_shader(s)}
    fn run_pixelbender_shader(&mut self,h:PixelBenderShaderHandle,a:&[PixelBenderShaderArgument],t:&PixelBenderTarget)->Result<PixelBenderOutput,Error>{self.last_frame=None;self.inner.run_pixelbender_shader(h,a,t)}
    fn resolve_sync_handle(&mut self,h:Box<dyn SyncHandle>,r:RgbaBufRead)->Result<(),Error>{self.inner.resolve_sync_handle(h,r)}
}
#[cfg(test)]mod tests {
    use super::*;use ruffle_render::{commands::CommandHandler,matrix::Matrix};
    fn commands()->CommandList{let mut c=CommandList::new();c.draw_rect(swf::Color::WHITE,Matrix::IDENTITY);c}
    struct NoBitmaps;
    impl BitmapSource for NoBitmaps {
        fn bitmap_size(&self,_:u16)->Option<BitmapSize>{None}
        fn bitmap_handle(&self,_:u16,_:&mut dyn RenderBackend)->Option<BitmapHandle>{None}
    }
    #[test]fn shape_reuse_is_exact_resource_independent_and_bounded_by_frames(){
        use ruffle_render::shape_utils::{DrawPath,DrawCommand as E,FillRule};
        let style=swf::FillStyle::Color(swf::Color::WHITE);
        let shape=DistilledShape{paths:vec![DrawPath::Fill{style:&style,commands:vec![E::MoveTo(swf::Point::new(swf::Twips::ZERO,swf::Twips::ZERO)),E::LineTo(swf::Point::new(swf::Twips::new(20),swf::Twips::ZERO))],winding_rule:FillRule::EvenOdd}],shape_bounds:Default::default(),edge_bounds:Default::default(),id:7};
        let mut r=Cached::new(ruffle_render::backend::null::NullRenderer::new(ViewportDimensions{width:4,height:4,scale_factor:1.}));
        let a=r.register_shape(shape.clone(),&NoBitmaps);let b=r.register_shape(shape.clone(),&NoBitmaps);assert!(Arc::ptr_eq(&a.0,&b.0));
        r.submit_frame(swf::Color::BLACK,commands(),vec![]);
        let b=r.register_shape(shape.clone(),&NoBitmaps);assert!(Arc::ptr_eq(&a.0,&b.0));
        let b=r.register_shape_with_scale(shape.clone(),&NoBitmaps,1.);assert!(!Arc::ptr_eq(&a.0,&b.0));
        let c=r.register_shape_with_scale(shape.clone(),&NoBitmaps,2.);assert!(!Arc::ptr_eq(&b.0,&c.0));
        let mut changed=shape.clone();if let DrawPath::Fill{commands,..}=&mut changed.paths[0]{commands.push(E::LineTo(swf::Point::new(swf::Twips::new(21),swf::Twips::ZERO)));}
        let b=r.register_shape(changed,&NoBitmaps);assert!(!Arc::ptr_eq(&a.0,&b.0));
        r.set_quality(StageQuality::High);let b=r.register_shape(shape.clone(),&NoBitmaps);assert!(!Arc::ptr_eq(&a.0,&b.0));
        let bitmap=swf::FillStyle::Bitmap{id:1,matrix:Default::default(),is_smoothed:true,is_repeating:false};
        let mut bitmap_shape=shape.clone();if let DrawPath::Fill{style,..}=&mut bitmap_shape.paths[0]{*style=&bitmap;}
        assert!(ShapeKey::new(&bitmap_shape,None).is_none());
        for _ in 0..4 {r.submit_frame(swf::Color::BLACK,commands(),vec![]);}
        assert!(r.previous_shapes.is_empty()&&r.current_shapes.is_empty());
    }
}

#[cfg(test)] mod frame_tests {
    use super::*;use ruffle_render::{commands::CommandHandler,matrix::Matrix};
    fn commands()->CommandList {let mut list=CommandList::new();list.draw_rect(swf::Color::WHITE,Matrix::IDENTITY);list}
    #[test]fn resource_mutations_and_render_target_changes_force_submission() {
        let size=ViewportDimensions{width:4,height:4,scale_factor:1.};
        let mut r=Cached::new(ruffle_render::backend::null::NullRenderer::new(size));
        r.submit_frame(swf::Color::BLACK,commands(),vec![]);
        r.submit_frame(swf::Color::BLACK,commands(),vec![]);
        assert_eq!((r.submitted,r.identical_frames),(1,1));
        r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,2);
        r.set_viewport_dimensions(size);r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,3);
        let bitmap=||Bitmap::new(4,4,BitmapFormat::Rgba,vec![255;64]);
        let handle=r.register_bitmap(bitmap()).unwrap();let bounds=PixelRegion::for_whole_size(4,4);
        r.update_texture(&handle,bitmap(),bounds).unwrap();
        r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,4);
        r.render_offscreen(handle.clone(),commands(),StageQuality::High,bounds);
        r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,5);
        r.set_quality(StageQuality::High);r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,6);
        r.submit_frame(swf::Color::WHITE,commands(),vec![BitmapCacheEntry{handle,commands:commands(),clear:swf::Color::WHITE,filters:vec![]}]);
        assert_eq!(r.submitted,7);
        r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,8);
        r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,8);
        r.enabled=false;r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,9);
        r.enabled=true;r.submit_frame(swf::Color::WHITE,commands(),vec![]);assert_eq!(r.submitted,10);
    }
}
