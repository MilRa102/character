//! Local voice pipeline (MVP-2): wake-word, VAD, STT (whisper.cpp) and TTS (Piper).
//!
//! Everything runs on the device; audio captured before the wake-word is never stored.
//! The microphone is opened only in `character_types::Mode::Voice`.
