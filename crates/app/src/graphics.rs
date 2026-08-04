//! Platform-specific graphics policy selected before eframe starts.

use plotx_core::settings::GraphicsPowerPreference;
use std::ffi::OsStr;
#[cfg(any(windows, test))]
use std::ffi::OsString;

pub(crate) const HIGH_PERFORMANCE_ARG: &str = "--graphics=high-performance";

pub(crate) fn high_performance_requested<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(HIGH_PERFORMANCE_ARG))
}

#[cfg(any(windows, test))]
pub(crate) fn high_performance_relaunch_args<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args: Vec<_> = args
        .into_iter()
        .filter(|arg| arg.as_ref() != OsStr::new(HIGH_PERFORMANCE_ARG))
        .map(|arg| arg.as_ref().to_owned())
        .collect();
    args.push(HIGH_PERFORMANCE_ARG.into());
    args
}

pub(crate) fn startup_renderer(preference: GraphicsPowerPreference) -> eframe::Renderer {
    #[cfg(windows)]
    {
        match preference {
            GraphicsPowerPreference::LowPower => eframe::Renderer::Glow,
            GraphicsPowerPreference::HighPerformance => eframe::Renderer::Wgpu,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = preference;
        eframe::Renderer::Wgpu
    }
}

pub(crate) fn wgpu_power_preference(
    preference: GraphicsPowerPreference,
) -> eframe::wgpu::PowerPreference {
    match preference {
        GraphicsPowerPreference::LowPower => eframe::wgpu::PowerPreference::LowPower,
        GraphicsPowerPreference::HighPerformance => eframe::wgpu::PowerPreference::HighPerformance,
    }
}

#[cfg(windows)]
pub(crate) fn log_gl_adapter(cc: &eframe::CreationContext<'_>) {
    use eframe::glow::HasContext as _;

    let Some(gl) = cc.gl.as_ref() else {
        log::warn!("power-saving graphics started without an OpenGL context");
        return;
    };
    // SAFETY: eframe calls the app creator while its current OpenGL context is
    // valid on this thread.
    let (vendor, renderer) = unsafe {
        (
            gl.get_parameter_string(eframe::glow::VENDOR),
            gl.get_parameter_string(eframe::glow::RENDERER),
        )
    };
    log::info!("OpenGL adapter: vendor={vendor:?}; renderer={renderer:?}");
    if vendor.to_ascii_lowercase().contains("nvidia")
        || renderer.to_ascii_lowercase().contains("nvidia")
    {
        log::warn!(
            "Windows assigned the NVIDIA adapter to the power-saving graphics path; check the per-app Windows graphics preference"
        );
    }
}

#[cfg(windows)]
fn offer_high_performance_restart(error: &eframe::Error) -> std::io::Result<bool> {
    use std::os::windows::process::CommandExt as _;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONERROR, MB_SETFOREGROUND, MB_TASKMODAL, MB_YESNO, MessageBoxW,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let message = format!(
        "PlotX could not start the Power saving graphics mode.\n\n\
         Start PlotX in High performance mode instead? The saved preference will not change.\n\n\
         Technical details: {error}"
    );
    let message = wide_string(&message);
    let title = wide_string("PlotX graphics startup failed");
    // SAFETY: both UTF-16 buffers are NUL-terminated and remain alive for the
    // duration of this modal call; a null owner is supported by MessageBoxW.
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_ICONERROR | MB_YESNO | MB_TASKMODAL | MB_SETFOREGROUND,
        )
    };
    if answer == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if answer != IDYES {
        return Ok(false);
    }

    let executable = std::env::current_exe()?;
    let args = high_performance_relaunch_args(std::env::args_os().skip(1));
    std::process::Command::new(executable)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(true)
}

pub(crate) fn recover_startup_error(
    error: &eframe::Error,
    preference: GraphicsPowerPreference,
    graphics_started: bool,
) -> bool {
    log::error!("graphics startup failed: {error}");
    #[cfg(windows)]
    if preference == GraphicsPowerPreference::LowPower && !graphics_started {
        match offer_high_performance_restart(error) {
            Ok(true) => {
                log::info!("relaunching with {HIGH_PERFORMANCE_ARG}");
                log::logger().flush();
                return true;
            }
            Ok(false) => {}
            Err(recovery_error) => {
                log::error!("could not offer graphics recovery: {recovery_error}");
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (preference, graphics_started);
    log::logger().flush();
    false
}

#[cfg(windows)]
fn wide_string(value: &str) -> Vec<u16> {
    value
        .encode_utf16()
        .map(|unit| if unit == 0 { u16::from(b' ') } else { unit })
        .chain(std::iter::once(0))
        .collect()
}
