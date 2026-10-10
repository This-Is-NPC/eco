//! Every external system, behind a port.

// Platform-neutral; only Windows' audio needs it outside the tests.
#[cfg(any(windows, test))]
pub mod audio_convert;
pub mod audio_ffmpeg;
pub mod audio_file;
#[cfg(unix)]
pub mod audio_pipewire;
#[cfg(windows)]
pub mod audio_wasapi;
pub mod control_socket;
#[cfg(unix)]
pub mod desktop_hyprland;
#[cfg(windows)]
pub mod desktop_windows;
pub mod devices;
pub mod diarizer_process;
#[cfg(unix)]
pub mod echo_cancel;
#[cfg(all(test, unix))]
pub mod fake_program;
pub mod fbank;
pub mod hook_shell;
pub mod http;
pub mod llm_openai;
pub mod local_socket;
pub mod omapass;
pub mod overlay;
pub mod people_files;
pub mod pipe;
#[cfg(unix)]
pub mod pipewire_devices;
#[cfg(unix)]
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
pub mod vad_silero;
pub mod websocket;
pub mod webvtt;
// Platform-neutral; only Windows controls its windows by it outside the tests.
#[cfg(any(windows, test))]
pub mod window_host;
#[cfg(unix)]
pub mod window_hyprland;
