#[cfg(feature = "blotworx-memory-profile")]
#[global_allocator]
static MEMORY_PROFILE_ALLOCATOR: blotworx::memory_profile::TrackingAllocator =
    blotworx::memory_profile::TrackingAllocator;

fn main() -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-spline-events") {return blotworx::blotworx::validate_spline_events();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-inactive-motion") {return blotworx::blotworx::validate_inactive_motion();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-owned-cleanup") {return blotworx::blotworx::validate_owned_cleanup();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-palette-root") {return blotworx::blotworx::validate_palette_root();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-airlock-spawns") {return blotworx::blotworx::validate_airlock_spawns();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-audio-mutes") {return blotworx::blotworx::validate_audio_mutes();}
    #[cfg(all(debug_assertions,feature="blotworx-ui-render"))]
    if std::env::args().any(|arg|arg=="--validate-animation-audio") {return blotworx::blotworx::validate_animation_audio();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-animation-payloads") {return blotworx::blotworx::validate_animation_payloads();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-conversation-clones") {return blotworx::blotworx::validate_conversation_clones();}
    let _ = env_logger::try_init();
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-story-rewards") {return blotworx::blotworx::validate_story_rewards();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-story-teardown") {return blotworx::blotworx::validate_story_teardown();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-streamed-story") {return blotworx::blotworx::validate_streamed_story();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-random-dialogue") {return blotworx::blotworx::validate_random_dialogue();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-usable-runtime") {return blotworx::blotworx::validate_usable_runtime();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-wii-morph-mesh") {return blotworx::blotworx::validate_wii_morph_mesh();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-model-billboards") {return blotworx::blotworx::validate_model_billboards();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-io-admission") {return blotworx::blotworx::validate_io_admission();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-idle-command") {
        return blotworx::blotworx::validate_idle_command();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-relay") {
        return blotworx::blotworx::validate_relay();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-completed-spawner") {
        return blotworx::blotworx::validate_completed_spawner();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-directional-combat") {
        return blotworx::blotworx::validate_directional_combat();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-streamed-triggers") {
        return blotworx::blotworx::validate_streamed_triggers();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-player-handoff") {
        return blotworx::blotworx::validate_player_handoff();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-live-inventory") {
        return blotworx::blotworx::validate_live_inventory();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-material-gpu") {
        struct Validation;
        impl eframe::App for Validation {
            fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        return eframe::run_native(
            "Renderer validation",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_visible(false),
                ..Default::default()
            },
            Box::new(|cc| {
                unsafe {
                    blotworx::render_support::viewport3d::validate_material_gpu(
                        cc.gl.as_ref().expect("OpenGL context"),
                    )?;
                }
                Ok(Box::new(Validation))
            }),
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()));
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg| arg == "--validate-demo-progression") {
        return blotworx::blotworx::validate_demo_progression();
    }
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-brush-puzzle") {return blotworx::blotworx::validate_brush_puzzle();}
    #[cfg(debug_assertions)]
    if std::env::args().any(|arg|arg=="--validate-gv-encounter") {return blotworx::blotworx::validate_gv_encounter();}
    blotworx::blotworx::run()
}
