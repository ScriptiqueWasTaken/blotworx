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
    create_sized(diagnostic, (853, 480))
}
pub(super) fn create_sized(diagnostic: bool, size: (u32, u32)) -> Result<(Renderer, Info)> {
    // Match Ruffle's indirect-validation exclusion: it does not issue indirect
    // draws, and that validation requires a device feature Ruffle may not use.
    let flags=base_flags(diagnostic).with_env().difference(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL);
    let defaults: &[wgpu::Backends] = if cfg!(windows) {
        &[wgpu::Backends::DX12, wgpu::Backends::VULKAN]
    } else {
        &[wgpu::Backends::PRIMARY]
    };
    let configured = wgpu::Backends::from_env().map(|backend| [backend]);
    let mut selected = None;
    let mut failures = Vec::new();
    for &backends in configured.as_ref().map_or(defaults, |choices| &choices[..]) {
        log::info!(target: "blotworx_phase", "Opening UI graphics backend {backends:?}");
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends, flags, backend_options: Default::default(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        match pollster::block_on(ruffle_render_wgpu::backend::request_adapter_and_device(
            backends, &instance, None, wgpu::PowerPreference::HighPerformance,
        )) {
            Ok((adapter, device, queue)) => {
                selected = Some((instance, adapter, device, queue));
                break;
            }
            Err(error) => failures.push(format!("{backends:?}: {error}")),
        }
    }
    let (instance, adapter, device, queue) =
        selected.with_context(|| format!("UI graphics device: {}", failures.join("; ")))?;
    let adapter_info=adapter.get_info();
    log::info!(target: "blotworx_phase", "UI graphics ready on {:?} {}", adapter_info.backend, adapter_info.name);
    let info=Info{adapter:adapter_info.name,backend:format!("{:?}",adapter_info.backend),driver:adapter_info.driver,
        flags:format!("{flags:?}"),native_api_validation:flags.contains(wgpu::InstanceFlags::VALIDATION),
        debug_information:flags.contains(wgpu::InstanceFlags::DEBUG),gpu_based_validation:flags.contains(wgpu::InstanceFlags::GPU_BASED_VALIDATION)};
    let descriptors=ruffle_render_wgpu::descriptors::Descriptors::new(instance,adapter,device,queue);
    let target=ruffle_render_wgpu::target::TextureTarget::new(&descriptors.device,size)
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
