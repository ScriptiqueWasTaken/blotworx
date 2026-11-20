// Gameplay uses production driver flags regardless of Cargo debug_assertions.
// wgpu's logical API/resource validation remains enabled. Only optional native
// API debug layers/shader information are controlled here, with explicit
// diagnostic and standard WGPU_* environment overrides.
use anyhow::{Result,Context};
use std::sync::Arc;
type Renderer=ruffle_render_wgpu::backend::WgpuRenderBackend<ruffle_render_wgpu::target::TextureTarget>;
#[derive(serde::Serialize)]pub struct Info {
    adapter:String,backend:String,driver:String,flags:String,
    native_api_validation:bool,debug_information:bool,gpu_based_validation:bool,
}
fn base_flags(diagnostic:bool)->wgpu::InstanceFlags {
    if diagnostic {wgpu::InstanceFlags::debugging().difference(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL)}else{wgpu::InstanceFlags::empty()}
}
pub fn create(diagnostic:bool)->Result<(Renderer,Info)> {
    // Match Ruffle's indirect-validation exclusion: it does not issue indirect
    // draws, and that validation requires a device feature Ruffle may not use.
    let flags=base_flags(diagnostic).with_env().difference(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
    let instance=wgpu::Instance::new(wgpu::InstanceDescriptor{
        backends:wgpu::Backends::PRIMARY,flags,backend_options:Default::default(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let(adapter,device,queue)=pollster::block_on(ruffle_render_wgpu::backend::request_adapter_and_device(
        wgpu::Backends::PRIMARY,&instance,None,wgpu::PowerPreference::HighPerformance))
        .map_err(|error|anyhow::anyhow!("HUD graphics device: {error}"))?;
    let adapter_info=adapter.get_info();
    let info=Info{adapter:adapter_info.name,backend:format!("{:?}",adapter_info.backend),driver:adapter_info.driver,
        flags:format!("{flags:?}"),native_api_validation:flags.contains(wgpu::InstanceFlags::VALIDATION),
        debug_information:flags.contains(wgpu::InstanceFlags::DEBUG),gpu_based_validation:flags.contains(wgpu::InstanceFlags::GPU_BASED_VALIDATION)};
    let descriptors=ruffle_render_wgpu::descriptors::Descriptors::new(instance,adapter,device,queue);
    let target=ruffle_render_wgpu::target::TextureTarget::new(&descriptors.device,(853,480))
        .map_err(|error|anyhow::anyhow!("HUD target: {error}"))?;
    let renderer=Renderer::new(Arc::new(descriptors),target)
        .map_err(|error|anyhow::anyhow!("HUD renderer: {error}"))
        .context("Create production gameplay HUD graphics")?;
    Ok((renderer,info))
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]#[ignore="requires hardware graphics adapter"]
    fn production_keeps_wgpu_resource_validation()->Result<()> {
        let(renderer,info)=create(false)?;
        let device=renderer.device();
        let scope=device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _invalid=device.create_buffer(&wgpu::BufferDescriptor{
            label:Some("Intentional invalid HUD validation fixture"),size:4,
            usage:wgpu::BufferUsages::MAP_READ|wgpu::BufferUsages::VERTEX,mapped_at_creation:false,
        });
        let error=pollster::block_on(scope.pop()).context("wgpu accepted an invalid mapped vertex-buffer usage")?;
        anyhow::ensure!(matches!(error,wgpu::Error::Validation{..}),"Expected logical API validation error: {error}");
        std::fs::write("target/hud-production-resource-validation.json",serde_json::to_vec_pretty(&serde_json::json!({
            "graphics":info,"invalid_buffer_rejected":true,"error":error.to_string()}))?)?;
        Ok(())
    }
    #[test]fn production_does_not_inherit_cargo_driver_debugging(){assert!(base_flags(false).is_empty());}
    #[test]fn diagnostic_layers_are_explicit_and_indirect_validation_stays_excluded(){
        let flags=base_flags(true);
        assert!(flags.contains(wgpu::InstanceFlags::DEBUG|wgpu::InstanceFlags::VALIDATION));
        assert!(!flags.intersects(wgpu::InstanceFlags::GPU_BASED_VALIDATION|wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL));
    }
}
