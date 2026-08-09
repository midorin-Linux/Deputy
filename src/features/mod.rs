pub mod member_log;

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

    Ok(v)
}
