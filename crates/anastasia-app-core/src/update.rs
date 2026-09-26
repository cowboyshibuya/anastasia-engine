use crate::build;
use crate::storage;
pub use anastasia_update_core::{
    DownloadProgress, GIT_PULL_DIVERGED_SUMMARY, GitHubAsset, GitHubRelease, PreparedUpdate,
    UpdateCheckResult, UpdateEstimate, format_download_progress_bar, summarize_update_error,
    summary_is_divergence,
};
use anastasia_update_core::{format_duration_estimate, summarize_git_pull_failure};
use anyhow::{Context, Result};

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[path = "update_metadata.rs"]
mod update_metadata;
#[path = "update_rate_limit.rs"]
mod update_rate_limit;
pub use update_metadata::UpdateMetadata;
use update_metadata::record_source_update_duration;
pub use update_rate_limit::{RATE_LIMIT_ERROR_PREFIX, is_rate_limit_error};
use update_rate_limit::{clear_rate_limit_backoff, rate_limit_error};

const GITHUB_REPO: &str = "1jehuang/anastasia";
/// Minimum gap between *automatic* update checks.
///
/// Every automatic check costs one or two unauthenticated `api.github.com`
/// requests, which share a 60 req/hour per-IP bucket with everything else on
/// the machine (and everything behind the same NAT). A 60s gap meant a user
/// who opens anastasia a few dozen times an hour exhausted the bucket and then saw
/// spurious 403s. Half an hour is far below any realistic release cadence and
/// keeps automatic checks to at most a couple of requests per hour.
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(30 * 60);
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
pub fn print_centered(msg: &str) {
    let msg = crate::output_style::terminal_text(msg);
    let width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    for line in msg.lines() {
        let visible_len = unicode_display_width(line);
        if visible_len >= width {
            println!("{}", line);
        } else {
            let pad = (width - visible_len) / 2;
            println!("{:>pad$}{}", "", line, pad = pad);
        }
    }
}

fn unicode_display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    let mut w = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
            continue;
        }
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        w += UnicodeWidthChar::width(c).unwrap_or(0);
    }
    w
}

pub fn is_release_build() -> bool {
    anastasia_build_meta::is_release_build()
}

fn source_build_root() -> Result<PathBuf> {
    Ok(storage::anastasia_dir()?.join("builds").join("source"))
}

pub fn should_auto_update() -> bool {
    false
}

pub fn run_git_pull_ff_only(repo_dir: &Path, quiet: bool) -> Result<()> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("pull").arg("--ff-only");
    if quiet {
        cmd.arg("-q");
    }
    let output = cmd
        .current_dir(repo_dir)
        .output()
        .context("Failed to run git pull")?;

    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!("{}", summarize_git_pull_failure(&output.stderr));
    }
}

pub fn fetch_latest_release_blocking() -> Result<GitHubRelease> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(UPDATE_CHECK_TIMEOUT)
        .user_agent("anastasia-updater")
        .build()?;

    let response = github_api_request(&client, &url)
        .send()
        .context("Failed to fetch release info")?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("No releases found");
    }

    if let Some(error) = rate_limit_error(&response) {
        return Err(error);
    }

    if !response.status().is_success() {
        anyhow::bail!("GitHub API error: {}", response.status());
    }

    let release: GitHubRelease = response.json().context("Failed to parse release info")?;
    clear_rate_limit_backoff();
    Ok(release)
}

fn github_api_request(
    client: &reqwest::blocking::Client,
    url: &str,
) -> reqwest::blocking::RequestBuilder {
    let request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");

    // Authenticated requests use the user's 5000 req/h quota instead of the
    // shared unauthenticated 60 req/h per-IP bucket, which other tools on the
    // same machine or NAT can exhaust and cause spurious 403s on update
    // checks. Falls back to unauthenticated when no token is available.
    if let Some(token) = anastasia_base::github::github_public_api_token() {
        request.bearer_auth(token)
    } else {
        request
    }
}

fn install_main_source_update_blocking(latest_sha: &str) -> Result<PathBuf> {
    let path = build_from_source()?;
    crate::logging::info(&format!(
        "Main channel: built successfully at {}",
        path.display()
    ));

    let mut metadata = UpdateMetadata::load().unwrap_or_default();
    let channel_version = format!("main-{}", latest_sha);
    build::install_binary_at_version(&path, &channel_version)
        .context("Failed to install built binary")?;
    // Carry the long-lived daemon's reload target forward too, but only when it
    // was tracking stable. A deliberately-promoted self-dev shared-server build
    // is left untouched so the update never silently wipes it out.
    if let Err(error) = build::advance_shared_server_if_tracking_stable(&channel_version) {
        crate::logging::warn(&format!(
            "update: failed to advance shared-server channel to {}: {}",
            channel_version, error
        ));
    }
    build::update_stable_symlink(&channel_version)?;
    build::update_current_symlink(&channel_version)?;
    build::update_launcher_symlink_to_current()?;

    metadata.installed_version = Some(channel_version.clone());
    metadata.installed_from = Some("source".to_string());
    metadata.last_check = SystemTime::now();
    metadata.save()?;

    Ok(path)
}

pub fn prepare_update_blocking() -> Result<PreparedUpdate> {
    anyhow::bail!("Update Anastasia from its source checkout with scripts/install-anastasia.sh")
}

/// Log the full error and return a single short line for the UI.
///
/// Update failures come from many layers and are often multi-line, so the
/// verbose text belongs in the log while the card/notice stay one line.
fn short_update_error(context: &str, error: &anyhow::Error) -> String {
    crate::logging::warn(&format!("update: {}: {:#}", context, error));
    summarize_update_error(&format!("{:#}", error))
}

pub fn spawn_background_session_update(session_id: String) {
    std::thread::spawn(move || {
        use crate::bus::{Bus, BusEvent, ClientMaintenanceAction, SessionUpdateStatus};

        let action = ClientMaintenanceAction::Update;

        let publish = |status| Bus::global().publish(BusEvent::SessionUpdateStatus(status));

        match prepare_update_blocking() {
            Ok(PreparedUpdate::None { current }) => {
                publish(SessionUpdateStatus::NoUpdate {
                    session_id,
                    current,
                });
            }
            Ok(PreparedUpdate::Stable { release, estimate }) => {
                publish(SessionUpdateStatus::Status {
                    session_id: session_id.clone(),
                    action,
                    message: estimate.summary,
                });
                publish(SessionUpdateStatus::Status {
                    session_id: session_id.clone(),
                    action,
                    message: format!(
                        "Downloading {} (estimated {})...",
                        release.tag_name,
                        format_duration_estimate(estimate.duration)
                    ),
                });
                let progress_session_id = session_id.clone();
                let progress_version = release.tag_name.clone();
                match download_and_install_blocking_with_progress(&release, |progress| {
                    publish(SessionUpdateStatus::Status {
                        session_id: progress_session_id.clone(),
                        action,
                        message: format!(
                            "{} {}",
                            progress_version,
                            format_download_progress_bar(progress)
                        ),
                    });
                }) {
                    Ok(_) => publish(SessionUpdateStatus::ReadyToReload {
                        session_id,
                        action,
                        version: release.tag_name,
                    }),
                    Err(error) => publish(SessionUpdateStatus::Error {
                        session_id,
                        action,
                        message: short_update_error("update failed", &error),
                    }),
                }
            }
            Ok(PreparedUpdate::MainSource {
                latest_sha,
                estimate,
            }) => {
                publish(SessionUpdateStatus::Status {
                    session_id: session_id.clone(),
                    action,
                    message: estimate.summary,
                });
                publish(SessionUpdateStatus::Status {
                    session_id: session_id.clone(),
                    action,
                    message: format!(
                        "Building main-{} in the background (estimated {})...",
                        latest_sha,
                        format_duration_estimate(estimate.duration)
                    ),
                });
                match install_main_source_update_blocking(&latest_sha) {
                    Ok(_) => publish(SessionUpdateStatus::ReadyToReload {
                        session_id,
                        action,
                        version: format!("main-{}", latest_sha),
                    }),
                    Err(error) => publish(SessionUpdateStatus::Error {
                        session_id,
                        action,
                        message: short_update_error("update failed", &error),
                    }),
                }
            }
            Err(error) => publish(SessionUpdateStatus::Error {
                session_id,
                action,
                message: short_update_error("update check failed", &error),
            }),
        }
    });
}

pub fn check_for_update_blocking() -> Result<Option<GitHubRelease>> {
    Ok(None)
}

/// Build anastasia from source by cloning/pulling the repo and running cargo build
fn build_from_source() -> Result<PathBuf> {
    let started = Instant::now();
    let build_dir = source_build_root()?;
    fs::create_dir_all(&build_dir)?;

    let repo_dir = build_dir.join("anastasia");

    if repo_dir.join(".git").exists() {
        // Pull latest
        crate::logging::info("Main channel: pulling latest from main...");
        let output = std::process::Command::new("git")
            .args(["pull", "--ff-only", "origin", "main"])
            .current_dir(&repo_dir)
            .output()
            .context("Failed to run git pull")?;

        if !output.status.success() {
            // If pull fails (e.g. diverged), reset to origin/main
            let summary = summarize_git_pull_failure(&output.stderr);
            crate::logging::warn(&format!("{}, trying reset", summary));
            let output = std::process::Command::new("git")
                .args(["fetch", "origin", "main"])
                .current_dir(&repo_dir)
                .output()
                .context("Failed to run git fetch")?;
            if !output.status.success() {
                anyhow::bail!(
                    "git fetch failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            let output = std::process::Command::new("git")
                .args(["reset", "--hard", "origin/main"])
                .current_dir(&repo_dir)
                .output()
                .context("Failed to run git reset")?;
            if !output.status.success() {
                anyhow::bail!(
                    "git reset failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    } else {
        // Clone
        crate::logging::info("Main channel: cloning repository...");
        let clone_url = format!("https://github.com/{}.git", GITHUB_REPO);
        let output = std::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                "main",
                &clone_url,
                "anastasia",
            ])
            .current_dir(&build_dir)
            .output()
            .context("Failed to run git clone")?;

        if !output.status.success() {
            anyhow::bail!(
                "git clone failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    // Build
    crate::logging::info("Main channel: building with cargo...");
    let output = std::process::Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&repo_dir)
        .env("ANASTASIA_CLI_RELEASE_BUILD", "1")
        .output()
        .context("Failed to run cargo build")?;

    if !output.status.success() {
        anyhow::bail!(
            "cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let binary = build::release_binary_path(&repo_dir);
    if !binary.exists() {
        anyhow::bail!("Built binary not found at {}", binary.display());
    }

    record_source_update_duration(started.elapsed());

    Ok(binary)
}

pub fn download_and_install_blocking_with_progress(
    _release: &GitHubRelease,
    _on_progress: impl FnMut(DownloadProgress),
) -> Result<PathBuf> {
    anyhow::bail!("Upstream binary updates are disabled for Anastasia")
}

pub fn check_and_maybe_update(auto_install: bool) -> UpdateCheckResult {
    use crate::bus::{Bus, BusEvent, UpdateStatus};

    if !should_auto_update() {
        return UpdateCheckResult::NoUpdate;
    }

    let metadata = UpdateMetadata::load().unwrap_or_default();
    if !metadata.should_check() {
        return UpdateCheckResult::NoUpdate;
    }

    Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Checking));

    match check_for_update_blocking() {
        Ok(Some(release)) => {
            let current = anastasia_build_meta::version().to_string();
            let latest = release.tag_name.clone();

            Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Available {
                current: current.clone(),
                latest: latest.clone(),
            }));

            if auto_install {
                let progress_version = latest.clone();
                match download_and_install_blocking_with_progress(&release, |progress| {
                    Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Downloading {
                        version: progress_version.clone(),
                        downloaded: progress.downloaded,
                        total: progress.total,
                    }));
                }) {
                    Ok(path) => {
                        Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Installed {
                            version: latest.clone(),
                        }));
                        UpdateCheckResult::UpdateInstalled {
                            version: latest,
                            path,
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to install: {}", e);
                        Bus::global()
                            .publish(BusEvent::UpdateStatus(UpdateStatus::Error(msg.clone())));
                        UpdateCheckResult::Error(msg)
                    }
                }
            } else {
                let mut metadata = UpdateMetadata::load().unwrap_or_default();
                metadata.last_check = SystemTime::now();
                let _ = metadata.save();
                UpdateCheckResult::UpdateAvailable {
                    current,
                    latest,
                    _release: release,
                }
            }
        }
        Ok(None) => {
            repair_stale_shared_server_after_no_update();
            Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::UpToDate));
            let mut metadata = UpdateMetadata::load().unwrap_or_default();
            metadata.last_check = SystemTime::now();
            let _ = metadata.save();
            UpdateCheckResult::NoUpdate
        }
        Err(e) => {
            let msg = short_update_error("update check failed", &e);
            if is_rate_limit_error(&msg) {
                // Throttling is not an update failure and there is nothing the
                // user needs to do, so keep it out of the UI. The backoff was
                // already persisted, so we stop retrying too.
                crate::logging::info(&msg);
                Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::UpToDate));
                return UpdateCheckResult::NoUpdate;
            }
            Bus::global().publish(BusEvent::UpdateStatus(UpdateStatus::Error(msg.clone())));
            UpdateCheckResult::Error(msg)
        }
    }
}

fn repair_stale_shared_server_after_no_update() {
    match build::repair_stale_shared_server_channel() {
        Ok(build::SharedServerRepair::Repaired {
            previous,
            repaired_to,
        }) => {
            crate::logging::info(&format!(
                "update: repaired stale shared-server channel {:?} -> {} after no-op update check",
                previous, repaired_to
            ));
        }
        Ok(build::SharedServerRepair::AlreadyCurrent) => {}
        Err(error) => {
            crate::logging::warn(&format!(
                "update: failed to repair stale shared-server channel after no-op update check: {}",
                error
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_download_progress_bar_known_total() {
        let rendered = format_download_progress_bar(DownloadProgress {
            downloaded: 512,
            total: Some(1024),
        });
        assert!(rendered.contains("50%"));
        assert!(rendered.contains("512 B/1.0 KiB"));
        assert!(rendered.contains('█'));
        assert!(rendered.contains('░'));
    }

    #[test]
    fn test_format_download_progress_bar_unknown_total() {
        let rendered = format_download_progress_bar(DownloadProgress {
            downloaded: 2 * 1024 * 1024,
            total: None,
        });
        assert_eq!(rendered, "Downloading update... 2.0 MiB downloaded");
    }

    #[test]
    fn test_is_release_build() {
        assert!(!is_release_build());
    }

    #[test]
    fn test_should_auto_update_dev_build() {
        assert!(!should_auto_update());
    }

    #[test]
    fn test_summarize_git_pull_failure_diverged() {
        let stderr = b"hint: You have divergent branches and need to specify how to reconcile them.\nfatal: Need to specify how to reconcile divergent branches.\n";
        assert_eq!(
            summarize_git_pull_failure(stderr),
            anastasia_update_core::GIT_PULL_DIVERGED_SUMMARY
        );
        assert!(anastasia_update_core::summary_is_divergence(
            &summarize_git_pull_failure(stderr)
        ));
    }

    #[test]
    fn test_summarize_git_pull_failure_no_tracking_branch() {
        let stderr = b"There is no tracking information for the current branch.\n";
        assert_eq!(
            summarize_git_pull_failure(stderr),
            "git pull failed: current branch has no upstream tracking branch"
        );
    }

    #[test]
    fn test_summarize_git_pull_failure_uses_first_non_hint_line() {
        let stderr = b"hint: test hint\nfatal: repository not found\n";
        assert_eq!(
            summarize_git_pull_failure(stderr),
            "git pull failed: repository not found"
        );
    }
}
