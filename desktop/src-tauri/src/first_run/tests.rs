use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

use gaia_core::auth::AuthTable;
use serde_json::json;

use super::*;

fn paths(dir: &tempfile::TempDir) -> (PathBuf, PathBuf) {
    (
        dir.path().join("settings/config.toml"),
        dir.path().join("data/gaia.db"),
    )
}

#[test]
fn setup_trims_names_and_publishes_only_the_agent_hash() {
    let dir = tempfile::tempdir().unwrap();
    let (config_path, db_path) = paths(&dir);
    let (runtime, response) = setup(&config_path, &db_path, "  所属  ", "  利用者  ").unwrap();
    let config = Config::load(&config_path).unwrap();
    assert_eq!(config.cli.default_client.as_deref(), Some("desktop:利用者"));
    assert_eq!(config.db_path.as_deref(), Some(db_path.as_path()));
    assert_eq!(config.clients.len(), 2);
    assert_eq!(config.client("desktop:利用者").unwrap().role, Role::Human);
    assert_eq!(config.client(AGENT_CLIENT).unwrap().role, Role::Agent);
    for client in &config.clients {
        assert_eq!(client.default_scope.as_deref(), Some("所属"));
    }
    assert_eq!(config.keys.len(), 1);
    assert!(config.keys.contains_key(AGENT_CLIENT));
    let identity = AuthTable::from_path(&config_path)
        .unwrap()
        .verify(&response.agent_key)
        .unwrap();
    assert_eq!(identity.name, AGENT_CLIENT);
    assert!(
        !fs::read_to_string(&config_path)
            .unwrap()
            .contains(&response.agent_key)
    );
    let affiliations = admin::list_affiliations(runtime.service.db()).unwrap();
    assert_eq!(affiliations.len(), 1);
    assert_eq!(affiliations[0].name, "所属");
    let info = runtime
        .service
        .call(&runtime.human, "get_server_info", json!({}))
        .unwrap();
    assert_eq!(info["client"]["role"], "human");
    assert_eq!(info["client"]["name"], "desktop:利用者");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&config_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn empty_inputs_have_no_filesystem_side_effects() {
    for (affiliation, user) in [(" \t", "user"), ("scope", "\n ")] {
        let dir = tempfile::tempdir().unwrap();
        let (config_path, db_path) = paths(&dir);
        assert!(setup(&config_path, &db_path, affiliation, user).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[test]
fn existing_config_is_preserved_without_opening_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let (config_path, db_path) = paths(&dir);
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    let original = "intentionally invalid existing config";
    fs::write(&config_path, original).unwrap();
    assert!(setup(&config_path, &db_path, "scope", "user").is_err());
    assert_eq!(fs::read_to_string(&config_path).unwrap(), original);
    assert!(!db_path.exists());
}

#[test]
fn config_save_failure_leaves_a_retriable_database() {
    let dir = tempfile::tempdir().unwrap();
    let (config_path, db_path) = paths(&dir);
    let failed = setup_with_publisher(&config_path, &db_path, "scope", "user", |_, _| {
        Err("injected save failure".into())
    });
    assert!(matches!(failed, Err(error) if error == "injected save failure"));
    assert!(!config_path.exists());
    {
        let db = Db::open(&db_path).unwrap();
        let affiliations = admin::list_affiliations(&db).unwrap();
        assert_eq!(affiliations.len(), 1);
        assert_eq!(affiliations[0].name, "scope");
    }
    let (runtime, _) = setup(&config_path, &db_path, "scope", "user").unwrap();
    assert_eq!(
        admin::list_affiliations(runtime.service.db())
            .unwrap()
            .len(),
        1
    );
    assert!(Config::load(&config_path).is_ok());
}

#[test]
fn database_failure_does_not_publish_config() {
    let dir = tempfile::tempdir().unwrap();
    let (config_path, db_path) = paths(&dir);
    fs::create_dir_all(&db_path).unwrap();
    assert!(setup(&config_path, &db_path, "scope", "user").is_err());
    assert!(!config_path.exists());
}

#[test]
fn competing_config_publications_never_overwrite_the_winner() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = Arc::new(dir.path().join("config.toml"));
    let barrier = Arc::new(Barrier::new(2));
    let publications: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|name| {
            let config_path = config_path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut config = Config::default();
                config.cli.default_client = Some(name.into());
                barrier.wait();
                (name, publish_config(&config, &config_path))
            })
        })
        .collect();
    let results: Vec<_> = publications
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect();
    let winners: Vec<_> = results
        .iter()
        .filter(|(_, result)| result.is_ok())
        .collect();
    assert_eq!(winners.len(), 1);
    let saved = Config::load(&config_path).unwrap();
    assert_eq!(saved.cli.default_client.as_deref(), Some(winners[0].0));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn setup_and_publication_reject_existing_and_dangling_symlinks() {
    use std::os::unix::fs::symlink;

    for target_exists in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let target_path = dir.path().join("target.toml");
        let db_path = dir.path().join("gaia.db");
        if target_exists {
            fs::write(&target_path, "original").unwrap();
        }
        symlink(&target_path, &config_path).unwrap();
        assert!(setup(&config_path, &db_path, "scope", "user").is_err());
        assert!(publish_config(&Config::default(), &config_path).is_err());
        assert_eq!(fs::read_link(&config_path).unwrap(), target_path);
        assert_eq!(target_path.exists(), target_exists);
        if target_exists {
            assert_eq!(fs::read_to_string(&target_path).unwrap(), "original");
        }
        assert!(!db_path.exists());
    }
}
