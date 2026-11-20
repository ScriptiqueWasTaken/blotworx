// Included in the HUD worker module. This compositor uses the same egui Glow
// painter/shader, premultiplied pixels, linear sampler and quad as eager HUD
// presentation. Only GPU scheduling changes; no frame is reused or delayed.
use super::*;
#[derive(Default)]
pub(super) struct State {
    painter:Option<egui_glow::Painter>,pub image:Option<egui::ColorImage>,
    pub error:Option<String>,frames:u64,uploads:u64,
    wait:HudTiming,upload:HudTiming,draw:HudTiming,
}
static RETIRED:Mutex<Vec<Arc<Mutex<State>>>>=Mutex::new(Vec::new());
pub(super) fn retire(state:Arc<Mutex<State>>) {RETIRED.lock().unwrap().push(state);}
pub(super) fn collect(force:bool) {
    RETIRED.lock().unwrap().retain(|state| {
        if !force && Arc::strong_count(state)>1 {return true;}
        destroy(state);false
    });
}
pub(super) fn destroy(state:&Arc<Mutex<State>>) {
    if let Some(mut painter)=state.lock().unwrap().painter.take(){painter.destroy();}
}
pub(super) fn summary(state:&Arc<Mutex<State>>)->serde_json::Value {
    let state=state.lock().unwrap();serde_json::json!({"same_frame":true,"frames":state.frames,"uploads":state.uploads,
        "gpu_callback_wait":state.wait,"texture_upload":state.upload,"quad_draw":state.draw,"error":state.error})
}
pub(super) fn timing_lines(state:&Arc<Mutex<State>>,lines:&mut Vec<String>) {
    let state=state.lock().unwrap();
    crate::blotworx::performance::append_line(lines,"HUD pixel wait",&state.wait);
    crate::blotworx::performance::append_line(lines,"HUD upload",&state.upload);
}
pub(super) fn schedule(ui:&egui::Ui,rect:egui::Rect,job:Arc<Mutex<PixelJob>>,state:Arc<Mutex<State>>) {
    let callback=egui::PaintCallback{rect,callback:Arc::new(egui_glow::CallbackFn::new(move|info,parent| {
        collect(false);
        let start=std::time::Instant::now();
        let image=job.lock().unwrap().take_image();let waited=start.elapsed();
        let mut state=state.lock().unwrap();state.wait.record(waited);
        let result=(||->Result<()> {
            if let Some(image)=image? {state.image=Some(image);}
            if state.painter.is_none(){state.painter=Some(egui_glow::Painter::new(parent.gl().clone(),"",None,true)
                .map_err(|error|anyhow::anyhow!("HUD compositor: {error}"))?);}
            if let Some(image)=state.image.take() {
                let start=std::time::Instant::now();
                state.painter.as_mut().unwrap().set_texture(egui::TextureId::Managed(0),&egui::epaint::ImageDelta::full(image,egui::TextureOptions::LINEAR));
                state.upload.record(start.elapsed());state.uploads+=1;
            }
            let painter=state.painter.as_mut().unwrap();
            // No frame exists before the original movie requests its first
            // render. Match the eager path's absence of a texture in that case.
            if painter.texture(egui::TextureId::Managed(0)).is_some() {
                let mut mesh=egui::Mesh::with_texture(egui::TextureId::Managed(0));
                mesh.add_rect_with_uv(rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);
                let start=std::time::Instant::now();
                painter.paint_primitives(info.screen_size_px,info.pixels_per_point,&[egui::ClippedPrimitive{
                    clip_rect:info.clip_rect,primitive:egui::epaint::Primitive::Mesh(mesh)}]);
                state.draw.record(start.elapsed());
            }
            state.frames+=1;Ok(())
        })();
        if let Err(error)=result {state.error=Some(format!("{error:#}"));}
    }))};
    ui.painter().with_clip_rect(rect).add(egui::Shape::Callback(callback));
}
