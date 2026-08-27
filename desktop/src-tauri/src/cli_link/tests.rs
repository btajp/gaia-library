use std::os::unix::{ffi::OsStringExt, fs::symlink};

use super::*;

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    link: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let target = root.join("App.app/Contents/MacOS/gaia");
        let link = root.join("home/.local/bin/gaia");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "test executable").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            _temporary: temporary,
            root,
            link,
            target,
        }
    }

    fn original_link(&self) {
        fs::create_dir_all(self.link.parent().unwrap()).unwrap();
        symlink("previous/gaia", &self.link).unwrap();
    }

    fn recovery_paths(&self) -> Vec<PathBuf> {
        fs::read_dir(self.link.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".gaia-cli-link-")
            })
            .collect()
    }
}

#[test]
fn status_reports_missing_correct_wrong_and_non_symlink() {
    let fixture = Fixture::new();
    assert_eq!(
        status_at(&fixture.link, &fixture.target).unwrap(),
        LinkStatus::Missing
    );
    create_at(&fixture.link, &fixture.target, None, &|_| {}).unwrap();
    assert_eq!(
        status_at(&fixture.link, &fixture.target).unwrap(),
        LinkStatus::Ok
    );
    fs::remove_file(&fixture.link).unwrap();
    symlink("missing/gaia", &fixture.link).unwrap();
    assert_eq!(
        status_at(&fixture.link, &fixture.target).unwrap(),
        LinkStatus::WrongTarget {
            current: "missing/gaia".into()
        }
    );
    fs::remove_file(&fixture.link).unwrap();
    fs::write(&fixture.link, "existing CLI").unwrap();
    assert_eq!(
        status_at(&fixture.link, &fixture.target).unwrap(),
        LinkStatus::NotSymlink
    );
    assert_eq!(
        serde_json::to_value(LinkStatus::Missing).unwrap(),
        serde_json::json!({"status": "missing"})
    );
}

#[test]
fn relative_symlink_to_bundled_cli_is_already_correct() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.link.parent().unwrap()).unwrap();
    let relative = "../../../App.app/Contents/MacOS/gaia";
    symlink(relative, &fixture.link).unwrap();
    assert_eq!(
        status_at(&fixture.link, &fixture.target).unwrap(),
        LinkStatus::Ok
    );
    create_at(&fixture.link, &fixture.target, None, &|_| {
        panic!("must not replace")
    })
    .unwrap();
    assert_eq!(fs::read_link(&fixture.link).unwrap(), Path::new(relative));
}

#[test]
fn wrong_symlink_is_replaced_without_leaving_a_backup() {
    let fixture = Fixture::new();
    fixture.original_link();
    create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|_| {},
    )
    .unwrap();
    assert_eq!(
        status_at(&fixture.link, &fixture.target).unwrap(),
        LinkStatus::Ok
    );
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn regular_files_and_directories_are_never_replaced() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.link.parent().unwrap()).unwrap();
    fs::write(&fixture.link, "protected CLI").unwrap();
    assert!(create_at(&fixture.link, &fixture.target, None, &|_| {}).is_err());
    assert_eq!(fs::read_to_string(&fixture.link).unwrap(), "protected CLI");
    fs::remove_file(&fixture.link).unwrap();
    fs::create_dir(&fixture.link).unwrap();
    fs::write(fixture.link.join("protected-data"), "retained").unwrap();
    assert!(create_at(&fixture.link, &fixture.target, None, &|_| {}).is_err());
    assert_eq!(
        fs::read_to_string(fixture.link.join("protected-data")).unwrap(),
        "retained"
    );
}

#[test]
fn missing_or_nonexecutable_bundle_does_not_create_a_link() {
    let fixture = Fixture::new();
    let executable = fixture.target.parent().unwrap().join("gaia-desktop");
    assert_eq!(bundled_cli_at(&executable).unwrap(), fixture.target);
    fs::set_permissions(&fixture.target, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(bundled_cli_at(&executable).is_err());
    fs::remove_file(&fixture.target).unwrap();
    assert!(bundled_cli_at(&executable).is_err());
    assert!(create_at(&fixture.link, &fixture.target, None, &|_| {}).is_err());
    assert!(!fixture.link.parent().unwrap().exists());
}

#[test]
fn link_path_uses_injected_home_and_rejects_missing_home() {
    let fixture = Fixture::new();
    let path = link_path_with(&|name| {
        (name == "HOME").then(|| fixture.root.join("home").into_os_string())
    })
    .unwrap();
    assert_eq!(path, fixture.link);
    assert!(link_path_with(&|_| None).is_err());
    assert!(link_path_with(&|_| Some(OsString::from("relative"))).is_err());
}

#[test]
fn concurrent_regular_file_during_creation_is_preserved() {
    let fixture = Fixture::new();
    let result = create_at(&fixture.link, &fixture.target, None, &|stage| {
        if matches!(stage, CreateStage::CreateMissing) {
            fs::write(&fixture.link, "concurrent CLI").unwrap();
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&fixture.link).unwrap(), "concurrent CLI");
}

#[test]
fn file_replacing_old_symlink_is_restored_and_rejected() {
    let fixture = Fixture::new();
    fixture.original_link();
    let result = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|stage| {
            if matches!(stage, CreateStage::MoveExisting) {
                fs::remove_file(&fixture.link).unwrap();
                fs::write(&fixture.link, "concurrent CLI").unwrap();
            }
        },
    );
    assert!(result.unwrap_err().contains("復元しました"));
    assert_eq!(fs::read_to_string(&fixture.link).unwrap(), "concurrent CLI");
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn directory_replacing_old_symlink_is_restored_without_deleting_contents() {
    let fixture = Fixture::new();
    fixture.original_link();
    let result = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|stage| {
            if matches!(stage, CreateStage::MoveExisting) {
                fs::remove_file(&fixture.link).unwrap();
                fs::create_dir(&fixture.link).unwrap();
                fs::write(fixture.link.join("important"), "concurrent data").unwrap();
            }
        },
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(fixture.link.join("important")).unwrap(),
        "concurrent data"
    );
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn replacement_conflict_preserves_both_the_new_file_and_old_symlink() {
    let fixture = Fixture::new();
    fixture.original_link();
    let result = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|stage| {
            if matches!(stage, CreateStage::PublishReplacement) {
                fs::write(&fixture.link, "concurrent CLI").unwrap();
            }
        },
    );
    let error = result.unwrap_err();
    assert_eq!(fs::read_to_string(&fixture.link).unwrap(), "concurrent CLI");
    let recovery = fixture.recovery_paths();
    assert_eq!(recovery.len(), 1);
    let previous = recovery[0].join("previous");
    assert_eq!(
        fs::read_link(&previous).unwrap(),
        Path::new("previous/gaia")
    );
    assert!(error.contains(&previous.to_string_lossy().to_string()));
}

#[test]
fn restore_conflict_preserves_a_moved_regular_file_for_recovery() {
    let fixture = Fixture::new();
    fixture.original_link();
    let result = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|stage| match stage {
            CreateStage::MoveExisting => {
                fs::remove_file(&fixture.link).unwrap();
                fs::write(&fixture.link, "first concurrent file").unwrap();
            }
            CreateStage::RestorePrevious => {
                fs::write(&fixture.link, "second concurrent file").unwrap();
            }
            _ => {}
        },
    );
    let error = result.unwrap_err();
    assert_eq!(
        fs::read_to_string(&fixture.link).unwrap(),
        "second concurrent file"
    );
    let recovery = fixture.recovery_paths();
    assert_eq!(recovery.len(), 1);
    let previous = recovery[0].join("previous");
    assert_eq!(
        fs::read_to_string(&previous).unwrap(),
        "first concurrent file"
    );
    assert!(error.contains(&previous.to_string_lossy().to_string()));
}

#[test]
fn missing_expectation_never_authorizes_replacing_a_different_symlink() {
    let fixture = Fixture::new();
    fixture.original_link();
    let error = create_at(&fixture.link, &fixture.target, None, &|_| {
        panic!("must not begin replacing an unconfirmed link")
    })
    .unwrap_err();
    assert!(error.contains("再読み込み"));
    assert_eq!(
        fs::read_link(&fixture.link).unwrap(),
        Path::new("previous/gaia")
    );
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn confirmed_target_changed_before_execution_is_preserved() {
    let fixture = Fixture::new();
    fixture.original_link();
    fs::remove_file(&fixture.link).unwrap();
    symlink("different/gaia", &fixture.link).unwrap();
    let error = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|_| panic!("must not begin replacing a changed link"),
    )
    .unwrap_err();
    assert!(error.contains("再確認"));
    assert_eq!(
        fs::read_link(&fixture.link).unwrap(),
        Path::new("different/gaia")
    );
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn confirmed_target_disappearing_requires_a_new_confirmation() {
    let fixture = Fixture::new();
    fixture.original_link();
    fs::remove_file(&fixture.link).unwrap();
    let error = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|_| {},
    )
    .unwrap_err();
    assert!(error.contains("再確認"));
    assert!(fs::symlink_metadata(&fixture.link).is_err());
}

#[test]
fn relative_targets_require_an_exact_raw_match() {
    let fixture = Fixture::new();
    fixture.original_link();
    let equivalent_but_unconfirmed = "./previous/gaia";
    assert!(
        create_at(
            &fixture.link,
            &fixture.target,
            Some(equivalent_but_unconfirmed),
            &|_| {}
        )
        .is_err()
    );
    assert_eq!(
        fs::read_link(&fixture.link).unwrap(),
        Path::new("previous/gaia")
    );
    create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|_| {},
    )
    .unwrap();
    assert_eq!(
        status_at(&fixture.link, &fixture.target).unwrap(),
        LinkStatus::Ok
    );
}

#[test]
fn target_changed_immediately_before_retirement_is_restored() {
    let fixture = Fixture::new();
    fixture.original_link();
    let error = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|stage| {
            if matches!(stage, CreateStage::MoveExisting) {
                fs::remove_file(&fixture.link).unwrap();
                symlink("concurrent/gaia", &fixture.link).unwrap();
            }
        },
    )
    .unwrap_err();
    assert!(error.contains("復元しました"));
    assert!(error.contains("再確認"));
    assert_eq!(
        fs::read_link(&fixture.link).unwrap(),
        Path::new("concurrent/gaia")
    );
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn missing_expectation_preserves_a_symlink_created_during_execution() {
    let fixture = Fixture::new();
    let error = create_at(&fixture.link, &fixture.target, None, &|stage| {
        if matches!(stage, CreateStage::CreateMissing) {
            symlink("concurrent/gaia", &fixture.link).unwrap();
        }
    })
    .unwrap_err();
    assert!(error.contains("再確認"));
    assert_eq!(
        fs::read_link(&fixture.link).unwrap(),
        Path::new("concurrent/gaia")
    );
}

#[test]
fn non_utf8_target_cannot_be_displayed_or_confirmed_lossily() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.link.parent().unwrap()).unwrap();
    let target = PathBuf::from(OsString::from_vec(b"non-utf8-\xff/gaia".to_vec()));
    symlink(&target, &fixture.link).unwrap();
    assert!(
        status_at(&fixture.link, &fixture.target)
            .unwrap_err()
            .contains("UTF-8")
    );
    assert!(
        create_at(
            &fixture.link,
            &fixture.target,
            Some(&target.to_string_lossy()),
            &|_| {}
        )
        .is_err()
    );
    assert_eq!(fs::read_link(&fixture.link).unwrap(), target);
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn unexpected_retired_contents_are_restored_instead_of_deleted() {
    let fixture = Fixture::new();
    fixture.original_link();
    let error = create_at(
        &fixture.link,
        &fixture.target,
        Some("previous/gaia"),
        &|stage| {
            if matches!(stage, CreateStage::PublishReplacement) {
                let recovery = fixture.recovery_paths();
                assert_eq!(recovery.len(), 1);
                let previous = recovery[0].join("previous");
                fs::remove_file(&previous).unwrap();
                fs::write(&previous, "unexpected protected data").unwrap();
            }
        },
    )
    .unwrap_err();
    assert!(error.contains("復元しました"));
    assert_eq!(
        fs::read_to_string(&fixture.link).unwrap(),
        "unexpected protected data"
    );
    assert!(fixture.recovery_paths().is_empty());
}

#[test]
fn confirmation_checks_raw_target_even_when_the_inode_matches() {
    let fixture = Fixture::new();
    fixture.original_link();
    let parent = File::open(fixture.link.parent().unwrap()).unwrap();
    let name = OsStr::new("gaia");
    let current = rustix::fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW).unwrap();
    assert!(confirm_link(&parent, name, &current, "different/gaia").is_err());
    assert!(confirm_link(&parent, name, &current, "previous/gaia").is_ok());
}
