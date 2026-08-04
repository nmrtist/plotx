use super::*;

const FULL_BLEED_ICON_PNG: &[u8] = include_bytes!("../../../assets/icon-256.png");
const MACOS_ICON_PNG: &[u8] = include_bytes!("../../../assets/icon-256-macos.png");

#[test]
fn embedded_application_icons_are_256_rgba() {
    for bytes in [FULL_BLEED_ICON_PNG, MACOS_ICON_PNG] {
        let icon = eframe::icon_data::from_png_bytes(bytes).expect("decode embedded icon");
        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}

#[test]
fn frame_latency_matches_platform_presentation_requirements() {
    #[cfg(target_os = "macos")]
    assert_eq!(desired_maximum_frame_latency(), None);

    #[cfg(not(target_os = "macos"))]
    assert_eq!(desired_maximum_frame_latency(), Some(1));
}

#[test]
fn startup_renderer_obeys_the_platform_policy() {
    assert_eq!(
        startup_renderer(GraphicsPowerPreference::HighPerformance),
        eframe::Renderer::Wgpu
    );

    #[cfg(windows)]
    assert_eq!(
        startup_renderer(GraphicsPowerPreference::LowPower),
        eframe::Renderer::Glow
    );

    #[cfg(not(windows))]
    assert_eq!(
        startup_renderer(GraphicsPowerPreference::LowPower),
        eframe::Renderer::Wgpu
    );
}

#[test]
fn high_performance_override_is_exact_and_one_shot() {
    assert!(high_performance_requested([HIGH_PERFORMANCE_ARG]));
    assert!(high_performance_requested([
        "project.plotx",
        HIGH_PERFORMANCE_ARG
    ]));
    assert!(!high_performance_requested([
        "--graphics",
        "high-performance"
    ]));
    assert!(!high_performance_requested(["--graphics=low-power"]));
}

#[test]
fn recovery_relaunch_preserves_other_arguments_and_adds_one_override() {
    assert_eq!(
        graphics::high_performance_relaunch_args(["project.plotx"]),
        [
            std::ffi::OsString::from("project.plotx"),
            std::ffi::OsString::from(HIGH_PERFORMANCE_ARG)
        ]
    );
    assert_eq!(
        graphics::high_performance_relaunch_args([HIGH_PERFORMANCE_ARG]),
        [std::ffi::OsString::from(HIGH_PERFORMANCE_ARG)]
    );
}

#[test]
fn macos_application_icon_has_dock_padding() {
    let icon = image::load_from_memory(MACOS_ICON_PNG)
        .expect("decode macOS icon")
        .to_rgba8();
    for (x, y, pixel) in icon.enumerate_pixels() {
        if !(23..233).contains(&x) || !(23..233).contains(&y) {
            assert_eq!(pixel[3], 0, "pixel ({x}, {y}) is outside Dock safe area");
        }
    }
    assert_ne!(icon.get_pixel(128, 128)[3], 0);
}

#[test]
fn canceling_close_clears_update_restart_intent() {
    RELAUNCH_REQUESTED.store(false, Ordering::Relaxed);
    request_relaunch();
    assert!(RELAUNCH_REQUESTED.load(Ordering::Relaxed));
    cancel_relaunch();
    assert!(!RELAUNCH_REQUESTED.load(Ordering::Relaxed));
}

#[test]
fn recovery_is_not_rewritten_without_a_new_generation() {
    assert!(recovery_needed(true, 7, None));
    assert!(!recovery_needed(true, 7, Some(7)));
    assert!(recovery_needed(true, 8, Some(7)));
    assert!(!recovery_needed(false, 8, Some(7)));
}

#[test]
fn transition_waits_when_an_edit_lands_after_the_save_snapshot() {
    assert!(transition_ready_after_save(true, 7, 7));
    assert!(!transition_ready_after_save(true, 7, 8));
    assert!(!transition_ready_after_save(false, 7, 7));
}
