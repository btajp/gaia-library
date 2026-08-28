use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

use super::*;

#[derive(Default)]
struct FakeBackend {
    fail_store: Cell<bool>,
    fail_load: Cell<bool>,
    keys: RefCell<BTreeMap<String, String>>,
    loads: Cell<usize>,
}

impl KeyBackend for FakeBackend {
    fn store(&self, client: &str, plaintext: &str) -> Result<(), ()> {
        if self.fail_store.get() {
            return Err(());
        }
        self.keys
            .borrow_mut()
            .insert(client.into(), plaintext.into());
        Ok(())
    }

    fn load(&self, client: &str) -> Result<Option<String>, ()> {
        self.loads.set(self.loads.get() + 1);
        if self.fail_load.get() {
            Err(())
        } else {
            Ok(self.keys.borrow().get(client).cloned())
        }
    }
}

struct Fixture {
    _temporary: tempfile::TempDir,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().canonicalize().unwrap();
        Self {
            _temporary: temporary,
            home,
        }
    }

    fn lookup(&self, name: &str) -> Option<OsString> {
        (name == "HOME").then(|| self.home.clone().into_os_string())
    }

    fn root(&self) -> PathBuf {
        fallback_root(&|name| self.lookup(name)).unwrap()
    }

    fn path(&self, client: &str) -> PathBuf {
        self.root().join(key_filename(client))
    }

    fn store(&self, backend: &FakeBackend, client: &str, key: &str) -> StoreLocation {
        store_with(client, key, backend, &|name| self.lookup(name)).unwrap()
    }

    fn load(
        &self,
        backend: &FakeBackend,
        client: &str,
        hash: Option<&str>,
    ) -> Result<Option<(String, StoreLocation)>, String> {
        load_with(client, hash, backend, &|name| self.lookup(name))
    }
}

fn fallback_backend() -> FakeBackend {
    let backend = FakeBackend::default();
    backend.fail_store.set(true);
    backend
}

#[test]
fn keychain_success_never_resolves_or_writes_fallback_paths() {
    let backend = FakeBackend::default();
    let no_paths = |_: &str| panic!("fallback must not be accessed");
    assert_eq!(
        store_with("bot", "test-secret", &backend, &no_paths).unwrap(),
        StoreLocation::Keychain
    );
    assert_eq!(
        load_with("bot", None, &backend, &no_paths).unwrap(),
        Some(("test-secret".into(), StoreLocation::Keychain))
    );
    assert_eq!(
        serde_json::to_value(StoreLocation::Keychain).unwrap(),
        "keychain"
    );
    assert_eq!(serde_json::to_value(StoreLocation::File).unwrap(), "file");
}

#[test]
fn fallback_files_are_private_and_atomically_replaced() {
    let fixture = Fixture::new();
    let backend = fallback_backend();
    assert_eq!(
        fixture.store(&backend, "bot", "old-test-secret"),
        StoreLocation::File
    );
    assert_eq!(
        fs::metadata(fixture.root()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(fixture.path("bot"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let mut previous = File::open(fixture.path("bot")).unwrap();
    fixture.store(&backend, "bot", "new-test-secret");
    let mut previous_value = String::new();
    previous.read_to_string(&mut previous_value).unwrap();
    assert_eq!(previous_value, "old-test-secret");
    assert_eq!(
        fixture.load(&backend, "bot", None).unwrap(),
        Some(("new-test-secret".into(), StoreLocation::File))
    );
    assert_eq!(fs::read_dir(fixture.root()).unwrap().count(), 1);
}

#[test]
fn matching_file_is_found_even_when_keychain_contains_an_old_key() {
    let fixture = Fixture::new();
    let backend = FakeBackend::default();
    fixture.store(&backend, "bot", "old-test-secret");
    backend.fail_store.set(true);
    fixture.store(&backend, "bot", "current-test-secret");
    assert_eq!(
        fixture
            .load(&backend, "bot", Some(&hash_key("current-test-secret")))
            .unwrap(),
        Some(("current-test-secret".into(), StoreLocation::File))
    );
    assert!(
        fixture
            .load(&backend, "bot", Some(&hash_key("neither-key")))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .load(
                &backend,
                "bot",
                Some(&hash_key("old-test-secret").to_uppercase())
            )
            .unwrap(),
        Some(("old-test-secret".into(), StoreLocation::Keychain))
    );
}

#[test]
fn read_failure_is_not_reported_as_a_missing_key() {
    let fixture = Fixture::new();
    let backend = fallback_backend();
    assert!(fixture.load(&backend, "bot", None).unwrap().is_none());
    backend.fail_load.set(true);
    assert!(fixture.load(&backend, "bot", None).is_err());
    fixture.store(&backend, "bot", "available-test-secret");
    assert_eq!(
        fixture.load(&backend, "bot", None).unwrap(),
        Some(("available-test-secret".into(), StoreLocation::File))
    );
}

#[test]
fn client_names_never_become_path_components() {
    let fixture = Fixture::new();
    let backend = fallback_backend();
    for client in [
        "../escape",
        "/absolute/key",
        "desktop: 利用者\n",
        "",
        "a/b\\c",
    ] {
        fixture.store(&backend, client, "test-secret");
        let filename = key_filename(client);
        assert_eq!(filename.len(), 68);
        assert!(filename.ends_with(".key"));
        assert!(filename[..64].bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(
            fixture.load(&backend, client, None).unwrap(),
            Some(("test-secret".into(), StoreLocation::File))
        );
    }
    assert_eq!(fs::read_dir(fixture.root()).unwrap().count(), 5);
}

#[test]
fn fallback_paths_prefer_xdg_without_mutating_environment() {
    let fixture = Fixture::new();
    let xdg = fixture.home.join("xdg");
    let lookup = |name: &str| match name {
        "XDG_DATA_HOME" => Some(xdg.clone().into_os_string()),
        _ => fixture.lookup(name),
    };
    assert_eq!(
        fallback_root(&lookup).unwrap(),
        xdg.join("gaia-library/keys")
    );
    assert!(fallback_root(&|_| None).is_err());
    assert!(fallback_root(&|_| Some(OsString::from("relative"))).is_err());
    assert!(fallback_root(&|_| Some(fixture.home.join("../escape").into_os_string())).is_err());
}

#[test]
fn symlink_key_file_is_never_read_or_written_through() {
    let fixture = Fixture::new();
    let backend = fallback_backend();
    open_directory(&fixture.root(), true).unwrap();
    let protected = fixture.home.join("protected");
    fs::write(&protected, "not-a-key-protected-content").unwrap();
    symlink(&protected, fixture.path("bot")).unwrap();
    let error =
        store_with("bot", "test-secret", &backend, &|name| fixture.lookup(name)).unwrap_err();
    assert!(!error.contains("test-secret"));
    assert!(fixture.load(&backend, "bot", None).is_err());
    assert_eq!(
        fs::read_to_string(&protected).unwrap(),
        "not-a-key-protected-content"
    );
    assert_eq!(fs::read_link(fixture.path("bot")).unwrap(), protected);
}

#[test]
fn symlink_directories_are_never_followed() {
    for linked_component in ["gaia-library", "keys"] {
        let fixture = Fixture::new();
        let backend = fallback_backend();
        let outside = fixture.home.join("outside");
        fs::create_dir(&outside).unwrap();
        let linked = if linked_component == "gaia-library" {
            fixture.root().parent().unwrap().to_path_buf()
        } else {
            fixture.root()
        };
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        symlink(&outside, &linked).unwrap();
        assert!(store_with("bot", "test-secret", &backend, &|name| fixture.lookup(name)).is_err());
        assert!(fixture.load(&backend, "bot", None).is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    }
}

#[test]
fn public_permissions_and_nonregular_files_fail_closed() {
    let fixture = Fixture::new();
    let backend = fallback_backend();
    fixture.store(&backend, "bot", "test-secret");
    fs::set_permissions(fixture.path("bot"), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(fixture.load(&backend, "bot", None).is_err());
    fixture.store(&backend, "bot", "test-secret");
    fs::set_permissions(fixture.root(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(fixture.load(&backend, "bot", None).is_err());
    fixture.store(&backend, "bot", "test-secret");
    fs::create_dir(fixture.path("directory")).unwrap();
    assert!(fixture.load(&backend, "directory", None).is_err());
}

#[test]
fn read_errors_do_not_include_stored_bytes() {
    let fixture = Fixture::new();
    let backend = fallback_backend();
    fixture.store(&backend, "bot", "test-secret");
    fs::write(
        fixture.path("bot"),
        [b's', b'e', b'c', b'r', b'e', b't', 0xff],
    )
    .unwrap();
    let error = fixture.load(&backend, "bot", None).unwrap_err();
    assert!(!error.contains("secret"));
    assert!(error.contains("文字コード"));
    fs::write(fixture.path("bot"), vec![b'x'; MAX_KEY_BYTES + 1]).unwrap();
    assert!(fixture.load(&backend, "bot", None).is_err());
}

#[test]
fn hard_linked_file_is_not_read_and_atomic_store_preserves_other_name() {
    let fixture = Fixture::new();
    let backend = fallback_backend();
    fixture.store(&backend, "bot", "old-test-secret");
    let other = fixture.home.join("other-file");
    fs::hard_link(fixture.path("bot"), &other).unwrap();
    assert!(fixture.load(&backend, "bot", None).is_err());
    fixture.store(&backend, "bot", "new-test-secret");
    assert_eq!(fs::read_to_string(&other).unwrap(), "old-test-secret");
    assert_eq!(
        fixture.load(&backend, "bot", None).unwrap().unwrap().0,
        "new-test-secret"
    );
}

#[test]
fn invalid_input_and_total_storage_failure_are_reported_without_key_values() {
    let backend = fallback_backend();
    assert!(store_with("bot", "", &backend, &|_| None).is_err());
    assert!(store_with("bot", &"x".repeat(MAX_KEY_BYTES + 1), &backend, &|_| None).is_err());
    let error = store_with("bot", "must-not-be-in-error", &backend, &|_| None).unwrap_err();
    assert!(!error.contains("must-not-be-in-error"));
    assert!(load_with("bot", Some("invalid-hash"), &backend, &|_| None).is_err());
    assert_eq!(backend.loads.get(), 0);
}
