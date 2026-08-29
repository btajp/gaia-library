//! resolve_source の解決器の組み立て。`file` / `url` は gaia-core、`narumi` はこの crate が持つ。
//! CLI と desktop はこの `registry` と `ToolService::with_sources` だけを使う。
pub mod narumi;
pub mod narumi_uri;

use std::{path::Path, sync::Arc};

use gaia_core::sources::{
    ConfigFileSettings, FileResolver, ProtectedPaths, SourceRegistry, UrlResolver,
};

pub use narumi::NarumiResolver;

/// 設定ファイルを呼び出しごとに読み直す `file` / `url` / `narumi` の登録簿。
pub fn registry(config_path: &Path, protected: ProtectedPaths) -> SourceRegistry {
    let mut registry =
        SourceRegistry::new(Arc::new(ConfigFileSettings::new(config_path.to_path_buf())));
    registry
        .register(Arc::new(FileResolver::new(protected)))
        .expect("file resolver is registered once");
    registry
        .register(Arc::new(UrlResolver::public_only()))
        .expect("url resolver is registered once");
    registry
        .register(Arc::new(NarumiResolver::new()))
        .expect("narumi resolver is registered once");
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use gaia_core::config::Config;

    #[test]
    fn registry_holds_all_three_resolvers_and_reads_settings_per_call() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let registry = registry(&path, ProtectedPaths::new(dir.path(), dir.path()));
        assert_eq!(registry.systems(), vec!["file", "narumi", "url"]);
        assert!(
            registry.settings().is_err(),
            "missing config is fail-closed"
        );
        let mut config = Config::default();
        config.sources.url.allow_hosts = vec!["example.com".into()];
        config.save(&path).unwrap();
        let settings = registry.settings().unwrap();
        assert_eq!(registry.ready_systems(&settings), vec!["url"]);
    }
}
