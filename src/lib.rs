pub mod app;
pub mod config;
pub mod language;
pub mod ltengine;
pub mod pinyin_conv;
pub mod single_instance;
pub mod translation;
pub mod ui;

#[cfg(feature = "ocr")]
pub mod ocr;

#[cfg(feature = "transcription")]
pub mod transcription;
