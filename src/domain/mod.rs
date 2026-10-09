//! The core: sessions, prompts and events. It knows no provider, audio or
//! network library — only the ports in `crate::ports`.

pub mod action;
pub mod assistant;
pub mod billing;
pub mod channel;
pub mod diarization;
pub mod echo;
pub mod events;
pub mod hub;
pub mod loudness;
pub mod people;
pub mod prompts;
pub mod segmenter;
pub mod session;
pub mod transcribers;
