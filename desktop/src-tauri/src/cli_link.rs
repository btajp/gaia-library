//! 明示操作でのみ CLI リンクを作る。通常ファイルは競合時も上書きしない。
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use rustix::{
    fs::{AtFlags, FileType, Mode, OFlags, RenameFlags},
    io::Errno,
};
use serde::Serialize;
use uuid::Uuid;

const CONFIRM_AGAIN: &str =
    "確認後に CLI 配置先が変化しました。状態を再読み込みしてリンク先を再確認してください";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LinkStatus {
    Ok,
    Missing,
    WrongTarget { current: String },
    NotSymlink,
}

pub fn link_path() -> Result<PathBuf, String> {
    link_path_with(&|name| std::env::var_os(name))
}

fn link_path_with(lookup: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf, String> {
    let home = lookup("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| "HOME が未設定か絶対パスではありません".to_string())?;
    Ok(home.join(".local/bin/gaia"))
}

pub fn bundled_cli() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    bundled_cli_at(&executable)
}

fn bundled_cli_at(executable: &Path) -> Result<PathBuf, String> {
    let parent = executable
        .parent()
        .ok_or_else(|| "アプリの配置先を特定できません".to_string())?;
    validate_target(&parent.join("gaia"))
}

fn validate_target(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("同梱 CLI が見つからないか読み取れません: {error}"))?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err("同梱 CLI が実行可能な通常ファイルではありません".into());
    }
    path.canonicalize()
        .map_err(|error| format!("同梱 CLI の場所を解決できません: {error}"))
}

pub fn status() -> Result<LinkStatus, String> {
    status_at(&link_path()?, &bundled_cli()?)
}

fn status_at(link: &Path, target: &Path) -> Result<LinkStatus, String> {
    let target = validate_target(target)?;
    let metadata = match fs::symlink_metadata(link) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(LinkStatus::Missing),
        Err(error) => return Err(format!("CLI リンクを確認できません: {error}")),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(LinkStatus::NotSymlink);
    }
    let current =
        fs::read_link(link).map_err(|error| format!("CLI リンクを読めません: {error}"))?;
    let confirmable = current
        .to_str()
        .ok_or_else(|| "CLI リンク先が UTF-8 ではないため表示・確認できません".to_string())?
        .to_owned();
    let resolved = if current.is_absolute() {
        current.clone()
    } else {
        link.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(&current)
    };
    match resolved.canonicalize() {
        Ok(resolved) if resolved == target => return Ok(LinkStatus::Ok),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("現在の CLI リンクを解決できません: {error}")),
    }
    Ok(LinkStatus::WrongTarget {
        current: confirmable,
    })
}

/// None は確認時に未設置、Some は利用者が確認した read_link の生の文字列。
pub fn create(expected_target: Option<&str>) -> Result<(), String> {
    create_at(&link_path()?, &bundled_cli()?, expected_target, &|_| {})
}

fn create_at(
    link: &Path,
    target: &Path,
    expected_target: Option<&str>,
    hook: &dyn Fn(CreateStage),
) -> Result<(), String> {
    let target = validate_target(target)?;
    match status_at(link, &target)? {
        LinkStatus::Ok => return Ok(()),
        LinkStatus::Missing if expected_target.is_none() => {}
        LinkStatus::WrongTarget { current } if expected_target == Some(current.as_str()) => {}
        LinkStatus::Missing | LinkStatus::WrongTarget { .. } | LinkStatus::NotSymlink => {
            return Err(CONFIRM_AGAIN.into());
        }
    }
    let parent_path = link
        .parent()
        .ok_or_else(|| "CLI リンクの親がありません".to_string())?;
    let name = link
        .file_name()
        .ok_or_else(|| "CLI リンクの名前がありません".to_string())?;
    fs::create_dir_all(parent_path)
        .map_err(|error| format!("CLI 配置先を作成できません: {error}"))?;
    let parent = File::from(
        rustix::fs::open(
            parent_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| format!("CLI 配置先を開けません: {error}"))?,
    );
    let before = match rustix::fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) if expected_target.is_none() => {
            hook(CreateStage::CreateMissing);
            return rustix::fs::symlinkat(&target, &parent, name).map_err(|error| {
                format!(
                    "CLI リンクを新設できません（既存項目は変更しません）: {error}。{CONFIRM_AGAIN}"
                )
            });
        }
        Ok(stat)
            if expected_target.is_some()
                && FileType::from_raw_mode(stat.st_mode) == FileType::Symlink =>
        {
            stat
        }
        Ok(_) | Err(Errno::NOENT) => return Err(CONFIRM_AGAIN.into()),
        Err(error) => return Err(format!("CLI 配置先を確認できません: {error}")),
    };
    let expected_target = expected_target.ok_or_else(|| CONFIRM_AGAIN.to_string())?;
    confirm_link(&parent, name, &before, expected_target)?;
    let stage = StagedLink::new(&parent, parent_path)?;
    hook(CreateStage::MoveExisting);
    rustix::fs::renameat_with(
        &parent,
        name,
        &stage.directory,
        "previous",
        RenameFlags::NOREPLACE,
    )
    .map_err(|error| format!("既存 CLI リンクを退避できません: {error}。{CONFIRM_AGAIN}"))?;
    let previous_name = OsStr::new("previous");
    if let Err(error) = confirm_link(&stage.directory, previous_name, &before, expected_target) {
        return Err(stage.restore(&parent, name, &error, hook));
    }
    hook(CreateStage::PublishReplacement);
    if let Err(error) = confirm_link(&stage.directory, previous_name, &before, expected_target) {
        return Err(stage.restore(&parent, name, &error, hook));
    }
    if let Err(error) = rustix::fs::symlinkat(&target, &parent, name) {
        return Err(stage.restore(
            &parent,
            name,
            &format!("CLI リンクを新設できません: {error}。{CONFIRM_AGAIN}"),
            hook,
        ));
    }
    // 退避物が元の symlink と確認できた場合だけ破棄する。通常ファイルは削除しない。
    if let Err(error) = confirm_link(&stage.directory, previous_name, &before, expected_target) {
        return Err(format!(
            "CLI リンクは作成済みですが、退避物を削除せず保持しています: {}（{error}）",
            stage.recovery_path().display()
        ));
    }
    rustix::fs::unlinkat(&stage.directory, "previous", AtFlags::empty()).map_err(|error| {
        format!(
            "CLI リンクは作成済みですが旧リンクを除去できません: {error}（退避先: {}）",
            stage.recovery_path().display()
        )
    })
}

fn confirm_link(
    directory: &File,
    name: &OsStr,
    before: &rustix::fs::Stat,
    expected_target: &str,
) -> Result<(), String> {
    let current = rustix::fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|error| format!("{CONFIRM_AGAIN}（{error}）"))?;
    if FileType::from_raw_mode(current.st_mode) != FileType::Symlink
        || current.st_dev != before.st_dev
        || current.st_ino != before.st_ino
    {
        return Err(CONFIRM_AGAIN.into());
    }
    let target = rustix::fs::readlinkat(directory, name, Vec::new())
        .map_err(|error| format!("{CONFIRM_AGAIN}（{error}）"))?;
    if target.to_str().ok() != Some(expected_target) {
        return Err(CONFIRM_AGAIN.into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum CreateStage {
    CreateMissing,
    MoveExisting,
    PublishReplacement,
    RestorePrevious,
}

struct StagedLink<'a> {
    parent: &'a File,
    directory: File,
    name: String,
    parent_path: &'a Path,
}

impl<'a> StagedLink<'a> {
    fn new(parent: &'a File, parent_path: &'a Path) -> Result<Self, String> {
        let name = format!(".gaia-cli-link-{}", Uuid::new_v4());
        rustix::fs::mkdirat(parent, name.as_str(), Mode::from_raw_mode(0o700))
            .map_err(|error| format!("CLI リンクの退避先を作成できません: {error}"))?;
        let directory = match rustix::fs::openat(
            parent,
            name.as_str(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        ) {
            Ok(directory) => directory,
            Err(error) => {
                let _ = rustix::fs::unlinkat(parent, name.as_str(), AtFlags::REMOVEDIR);
                return Err(format!("CLI リンクの退避先を開けません: {error}"));
            }
        };
        Ok(Self {
            parent,
            directory: File::from(directory),
            name,
            parent_path,
        })
    }

    fn recovery_path(&self) -> PathBuf {
        self.parent_path.join(&self.name).join("previous")
    }

    fn restore(
        &self,
        parent: &File,
        name: &OsStr,
        reason: &str,
        hook: &dyn Fn(CreateStage),
    ) -> String {
        hook(CreateStage::RestorePrevious);
        match rustix::fs::renameat_with(
            &self.directory,
            "previous",
            parent,
            name,
            RenameFlags::NOREPLACE,
        ) {
            Ok(()) => format!("{reason}。既存項目は元の場所に復元しました"),
            Err(error) => format!(
                "{reason}。復元先も変化したため退避物を保持しています: {}（{error}）",
                self.recovery_path().display()
            ),
        }
    }
}

impl Drop for StagedLink<'_> {
    fn drop(&mut self) {
        // 非再帰の rmdir のみ。復元できなかった退避物があればそのまま残す。
        let _ = rustix::fs::unlinkat(self.parent, self.name.as_str(), AtFlags::REMOVEDIR);
    }
}

#[cfg(test)]
mod tests;
