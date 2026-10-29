//! Every external system, behind a port.

#[cfg(any(windows, target_os = "macos"))]
pub mod audio_cpal;
// Platform-neutral; only the cpal audio needs it outside the tests.
#[cfg(any(windows, target_os = "macos", test))]
pub mod audio_convert;
pub mod audio_ffmpeg;
pub mod audio_file;
#[cfg(target_os = "linux")]
pub mod audio_pipewire;
pub mod control_socket;
pub mod credentials;
#[cfg(any(windows, target_os = "macos"))]
pub mod daemon_log;
#[cfg(target_os = "linux")]
pub mod desktop_hyprland;
#[cfg(windows)]
pub mod desktop_windows;
pub mod devices;
pub mod diarizer_process;
#[cfg(target_os = "linux")]
pub mod echo_cancel;
#[cfg(all(test, unix))]
pub mod fake_program;
pub mod fbank;
pub mod hook_shell;
pub mod http;
#[cfg(target_os = "linux")]
pub mod installer_linux;
#[cfg(windows)]
pub mod installer_windows;
pub mod llm_openai;
pub mod local_socket;
#[cfg(target_os = "linux")]
pub mod notify_linux;
#[cfg(windows)]
pub mod notify_windows;
pub mod omapass;
pub mod overlay;
pub mod people_files;
pub mod pipe;
#[cfg(target_os = "linux")]
pub mod pipewire_devices;
pub mod releases_github;
#[cfg(target_os = "macos")]
pub mod service_launchd;
#[cfg(target_os = "linux")]
pub mod service_systemd;
#[cfg(windows)]
pub mod service_windows;
pub mod session_files;
#[cfg(windows)]
pub mod shortcuts_windows;
pub mod speaker_tract;
pub mod stt_deepgram;
pub mod stt_elevenlabs;
pub mod stt_openai;
pub mod terminal;
#[cfg(target_os = "macos")]
pub mod unsupported;
pub mod vad_silero;
pub mod websocket;
pub mod webvtt;
// Platform-neutral; only Windows controls its windows by it outside the tests.
#[cfg(any(windows, test))]
pub mod window_host;
#[cfg(target_os = "linux")]
pub mod window_hyprland;
