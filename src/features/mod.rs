pub mod honeypot;
pub mod member_log;
pub mod tts;
pub mod voice_log;

use std::sync::Arc;

use crate::core::{config::Config, feature::Feature, store::LogStore};

/// 機能の登録の唯一の場所。有効/無効は設定ファイルで完結する。
pub fn all(cfg: &Config, store: Arc<dyn LogStore>) -> anyhow::Result<Vec<Box<dyn Feature>>> {
    let mut v: Vec<Box<dyn Feature>> = Vec::new();

    if let Some(member_log_cfg) = member_log::config::MemberLogConfig::load(cfg)? {
        v.push(Box::new(member_log::MemberLog::new(
            member_log_cfg,
            store.clone(),
        )));
    }

    if let Some(voice_log_cfg) = voice_log::config::VoiceLogConfig::load(cfg)? {
        v.push(Box::new(voice_log::VoiceLog::new(
            voice_log_cfg,
            store.clone(),
        )));
    }

    if let Some(tts_cfg) = tts::config::TtsConfig::load(cfg)? {
        v.push(Box::new(tts::Tts::new(tts_cfg)?));
    }

    // priority最高。ハニーポットが処分したメッセージは他機能へ流さない。
    if let Some(honeypot_cfg) = honeypot::config::HoneypotConfig::load(cfg)? {
        v.push(Box::new(honeypot::Honeypot::new(
            honeypot_cfg,
            &cfg.ai,
            store.clone(),
        )?));
    }

    Ok(v)
}
