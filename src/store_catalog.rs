//! Persisted, project-scoped application listings for the local store.

use crate::workspace::WorkspaceProject;
use crate::workspace_releases::RepositoryReleases;
use release_desk_application::use_case::app_store::{AppInstallationProbe, FsAppInstallationProbe};
use release_desk_core::app_catalog::AppActionState;
use release_desk_store::workspace_releases::{
    WorkspaceAsset, WorkspaceRelease, release_asset_type,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreAsset {
    pub name: String,
    pub download_url: String,
    pub size: Option<u64>,
    pub platform: String,
    pub architecture: Option<String>,
    #[serde(default)]
    pub local_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreRelease {
    pub tag: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub assets: Vec<StoreAsset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoreApp {
    pub key: String,
    pub project_id: u128,
    pub project_name: String,
    pub project_directory: PathBuf,
    pub provider: String,
    pub owner: String,
    pub repository: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub icon_path: Option<PathBuf>,
    pub images: Vec<PathBuf>,
    pub bundle_name: String,
    pub releases: Vec<StoreRelease>,
    #[serde(default)]
    pub show_in_store: Option<bool>,
}

#[derive(Default, Deserialize)]
struct StoreManifest {
    name: Option<String>,
    description: Option<String>,
    category: Option<String>,
    icon: Option<String>,
    #[serde(default)]
    images: Vec<String>,
    bundle_name: Option<String>,
    show_in_store: Option<bool>,
}

impl StoreApp {
    pub fn display_name(&self) -> String {
        if self.name == self.repository {
            app_title(&self.repository)
        } else {
            self.name.clone()
        }
    }

    pub fn display_category(&self) -> String {
        self.category.clone()
    }

    pub fn is_app_listing(&self) -> bool {
        if self.show_in_store == Some(false) {
            return false;
        }
        let Some(asset) = self.platform_asset() else {
            return false;
        };
        if self.show_in_store == Some(true) {
            return true;
        }
        let lower = asset.name.to_ascii_lowercase();
        [".dmg", ".pkg", ".msi", ".exe", ".appimage", ".deb", ".rpm"]
            .iter()
            .any(|extension| lower.ends_with(extension))
    }

    pub fn latest(&self) -> Option<&StoreRelease> {
        self.releases.first()
    }

    pub fn platform_release(&self) -> Option<&StoreRelease> {
        self.releases.iter().find(|release| {
            release.assets.iter().any(|asset| {
                self.asset_belongs_to_app(asset) && asset_matches_current_platform(asset)
            })
        })
    }

    pub fn platform_asset(&self) -> Option<&StoreAsset> {
        self.platform_release()?
            .assets
            .iter()
            .filter(|asset| {
                self.asset_belongs_to_app(asset) && asset_matches_current_platform(asset)
            })
            .min_by_key(|asset| asset_priority(&asset.name))
    }

    pub fn state(&self, applications_dir: &Path) -> AppActionState {
        let installed =
            FsAppInstallationProbe.probe_installed_version(applications_dir, &self.bundle_name);
        AppActionState::derive(
            installed.as_deref(),
            self.platform_release().map(|r| r.tag.as_str()),
        )
    }

    fn asset_belongs_to_app(&self, asset: &StoreAsset) -> bool {
        if self.provider != "github" {
            return true;
        }
        let Ok(url) = url::Url::parse(&asset.download_url) else {
            return false;
        };
        let path = url.path().to_ascii_lowercase();
        let owner = self.owner.to_ascii_lowercase();
        let repository = self.repository.to_ascii_lowercase();
        match url.host_str() {
            Some("github.com") => path.starts_with(&format!("/{owner}/{repository}/releases/")),
            Some("api.github.com") => {
                path.starts_with(&format!("/repos/{owner}/{repository}/releases/"))
            }
            _ => false,
        }
    }
}

fn app_title(repository: &str) -> String {
    repository
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn asset_matches_current_platform(asset: &StoreAsset) -> bool {
    asset.platform == std::env::consts::OS
        && asset
            .architecture
            .as_deref()
            .is_none_or(|value| value == std::env::consts::ARCH)
}

fn asset_priority(name: &str) -> u8 {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".dmg") || name.ends_with(".appimage") || name.ends_with(".msi") {
        0
    } else if name.ends_with(".pkg") || name.ends_with(".deb") || name.ends_with(".exe") {
        1
    } else {
        2
    }
}

pub fn catalog_path(library_dir: &Path) -> PathBuf {
    library_dir.join("store/catalog.sqlite3")
}

pub fn load(path: &Path) -> Result<Vec<StoreApp>, String> {
    if !path.exists() {
        let old_path = path.with_file_name("catalog.json");
        if old_path.is_file() {
            let bytes = std::fs::read(&old_path).map_err(|error| error.to_string())?;
            let apps: Vec<StoreApp> = serde_json::from_slice(&bytes)
                .map_err(|error| format!("Cannot read old store catalog: {error}"))?;
            save(path, &apps)?;
            return Ok(apps);
        }
        return Ok(Vec::new());
    }
    let connection = rusqlite::Connection::open(path).map_err(|error| error.to_string())?;
    initialize_catalog(&connection)?;
    let mut statement = connection
        .prepare("SELECT payload FROM store_apps ORDER BY key")
        .map_err(|error| error.to_string())?;
    let records = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut apps: Vec<StoreApp> = records
        .map(|record| {
            let payload = record.map_err(|error| error.to_string())?;
            serde_json::from_str(&payload).map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Invalidate any loaded icon_path that is not a genuine square icon or has low resolution (< 128px)
    for app in &mut apps {
        if let Some(ref icon) = app.icon_path {
            let res = crate::modules::artwork::read_icon_resolution(icon);
            if !crate::modules::artwork::is_valid_icon_file(icon) || (res > 0 && res < 128) {
                app.icon_path = None;
            }
        }
    }

    Ok(apps)
}

/// Recheck explicit manifest artwork and cached agent icons without contacting a provider.
/// Repository scanning runs after a new clone or from project settings.
pub fn refresh_local_artwork(apps: &mut [StoreApp], projects: &[WorkspaceProject]) {
    for app in apps {
        let Some(project) = projects.iter().find(|project| project.id == app.project_id) else {
            continue;
        };
        let repo_dir = crate::repository_git::resolve_target_path(
            &project.directory,
            project.disk_layout,
            &app.owner,
            &app.repository,
        );
        let manifest = std::fs::read(repo_dir.join("release-desk-store.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StoreManifest>(&bytes).ok())
            .unwrap_or_default();
        let candidate = manifest
            .icon
            .as_deref()
            .and_then(|path| local_artwork(&repo_dir, path))
            .or_else(|| {
                crate::modules::icon_agent::resolve_icon_for(
                    Some(&project.directory),
                    &app.provider,
                    &app.owner,
                    &app.repository,
                )
            });
        app.icon_path = candidate.filter(|path| crate::modules::artwork::is_valid_icon_file(path));
        app.images = if manifest.images.is_empty() {
            discover_images(&repo_dir)
        } else {
            manifest
                .images
                .iter()
                .filter_map(|path| local_artwork(&repo_dir, path))
                .collect()
        };
    }
}

pub fn save(path: &Path, apps: &[StoreApp]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Store catalog has no parent directory")?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Cannot create {}: {error}", parent.display()))?;
    let mut connection = rusqlite::Connection::open(path).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    initialize_catalog(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute("DELETE FROM store_apps", [])
        .map_err(|error| error.to_string())?;
    {
        let mut statement = transaction
            .prepare("INSERT INTO store_apps (key, payload) VALUES (?1, ?2)")
            .map_err(|error| error.to_string())?;
        for app in apps {
            let payload = serde_json::to_string(app).map_err(|error| error.to_string())?;
            statement
                .execute(rusqlite::params![app.key, payload])
                .map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn initialize_catalog(connection: &rusqlite::Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS store_apps (key TEXT PRIMARY KEY, payload TEXT NOT NULL);",
        )
        .map_err(|error| error.to_string())
}

/// Replace only this project's records. Other projects retain their last good fetch.
pub fn merge_project(existing: &mut Vec<StoreApp>, project_id: u128, fresh: Vec<StoreApp>) {
    existing.retain(|app| app.project_id != project_id);
    existing.extend(fresh);
    existing.sort_by_key(|app| app.name.to_lowercase());
}

pub fn apps_for_project(project: &WorkspaceProject, repos: &[RepositoryReleases]) -> Vec<StoreApp> {
    repos
        .iter()
        .filter_map(|repo| {
            let releases = repo.result.as_ref().ok()?;
            let releases: Vec<_> = releases
                .iter()
                .filter(|release| !release.is_draft && !release.is_prerelease.unwrap_or(false))
                .map(|release| store_release(&project.directory, release))
                .collect();
            if releases.is_empty() {
                return None;
            }

            let repo_dir = crate::repository_git::resolve_target_path(
                &project.directory,
                project.disk_layout,
                &repo.owner,
                &repo.repository,
            );
            let manifest = std::fs::read(repo_dir.join("release-desk-store.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<StoreManifest>(&bytes).ok())
                .unwrap_or_default();
            let name = manifest
                .name
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| app_title(&repo.repository));
            let repository_description = project
                .sources
                .iter()
                .flat_map(|source| &source.repositories)
                .find(|item| {
                    item.name == repo.repository
                        && (item.full_name.is_empty()
                            || item.full_name == format!("{}/{}", repo.owner, repo.repository))
                })
                .and_then(|item| item.description.as_deref())
                .filter(|value| !value.trim().is_empty());
            let description = manifest
                .description
                .filter(|value| !value.trim().is_empty())
                .or_else(|| repository_description.map(str::to_string))
                .unwrap_or_else(|| format!("Releases from {}/{}", repo.owner, repo.repository));
            let category = manifest
                .category
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Other".to_string());
            let icon_path = manifest
                .icon
                .as_deref()
                .and_then(|value| local_artwork(&repo_dir, value))
                .or_else(|| {
                    crate::modules::icon_agent::resolve_icon_for(
                        Some(&project.directory),
                        &repo.provider,
                        &repo.owner,
                        &repo.repository,
                    )
                })
                .filter(|path| crate::modules::artwork::is_valid_icon_file(path));
            let images = manifest
                .images
                .iter()
                .filter_map(|value| local_artwork(&repo_dir, value))
                .collect::<Vec<_>>();
            let images = if images.is_empty() {
                discover_images(&repo_dir)
            } else {
                images
            };
            let bundle_name = manifest
                .bundle_name
                .filter(|value| safe_bundle_name(value))
                .unwrap_or_else(|| format!("{}.app", repo.repository));
            Some(StoreApp {
                key: format!(
                    "{}:{}:{}/{}",
                    project.id, repo.provider, repo.owner, repo.repository
                ),
                project_id: project.id,
                project_name: project.name.clone(),
                project_directory: project.directory.clone(),
                provider: repo.provider.clone(),
                owner: repo.owner.clone(),
                repository: repo.repository.clone(),
                name,
                description,
                category,
                icon_path,
                images,
                bundle_name,
                releases,
                show_in_store: manifest.show_in_store,
            })
        })
        .collect()
}

fn store_release(project_directory: &Path, release: &WorkspaceRelease) -> StoreRelease {
    StoreRelease {
        tag: release.tag_name.clone(),
        notes: release.description.clone().unwrap_or_default(),
        published_at: release.published_at.clone(),
        assets: release
            .assets
            .iter()
            .filter_map(|asset| store_asset(project_directory, asset))
            .collect(),
    }
}

fn store_asset(project_directory: &Path, asset: &WorkspaceAsset) -> Option<StoreAsset> {
    let kind = release_asset_type(&asset.name)?;
    let lower = asset.name.to_ascii_lowercase();
    let platform = match kind.extension {
        "dmg" | "pkg" => "macos",
        "exe" | "msi" => "windows",
        "appimage" | "deb" | "rpm" => "linux",
        "apk" | "aab" => "android",
        "zip" | "tar.gz" | "tgz" | "tar.xz" | "tar.bz2" => {
            if lower.contains("macos") || lower.contains("darwin") || lower.contains("osx") {
                "macos"
            } else if lower.contains("windows") || lower.contains("win32") {
                "windows"
            } else if lower.contains("linux") {
                "linux"
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let architecture = if lower.contains("aarch64") || lower.contains("arm64") {
        Some("aarch64")
    } else if lower.contains("x86_64") || lower.contains("amd64") {
        Some("x86_64")
    } else {
        None
    };
    Some(StoreAsset {
        name: asset.name.clone(),
        download_url: asset.download_url.clone(),
        size: asset.size,
        platform: platform.to_string(),
        architecture: architecture.map(str::to_string),
        local_path: Path::new(&asset.name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
            .then(|| project_directory.join(kind.folder).join(&asset.name)),
    })
}

fn local_artwork(repo_dir: &Path, value: &str) -> Option<PathBuf> {
    let relative = Path::new(value);
    if !relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return None;
    }
    let canonical_root = repo_dir.canonicalize().ok()?;
    let candidate = repo_dir.join(relative).canonicalize().ok()?;
    (candidate.starts_with(canonical_root) && candidate.is_file()).then_some(candidate)
}

fn discover_images(repo_dir: &Path) -> Vec<PathBuf> {
    let mut images = Vec::new();
    let Ok(root) = repo_dir.canonicalize() else {
        return images;
    };
    for folder in [
        "assets/app-store",
        "assets/screenshots",
        "screenshots",
        "docs/screenshots",
        "docs/images",
    ] {
        let Ok(entries) = std::fs::read_dir(repo_dir.join(folder)) else {
            continue;
        };
        let mut candidates: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let image = matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("png" | "jpg" | "jpeg" | "webp")
                );
                image
                    && (folder != "docs/images"
                        || name.contains("screenshot")
                        || name.starts_with("ui-")
                        || name.contains("preview"))
            })
            .collect();
        candidates.sort();
        for path in candidates {
            if let Ok(canonical) = path.canonicalize()
                && canonical.starts_with(&root)
                && canonical.is_file()
            {
                images.push(canonical);
                if images.len() == 5 {
                    return images;
                }
            }
        }
    }
    images
}

pub(crate) fn safe_bundle_name(value: &str) -> bool {
    let path = Path::new(value);
    value.ends_with(".app")
        && path.components().count() == 1
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::ProviderChoice;
    use crate::workspace_sources::RepositoryListMode;
    use release_desk_core::project::DiskLayout;

    #[test]
    fn catalog_keeps_projects_separate_and_restores_from_disk() {
        let dir = tempfile::tempdir().expect("temp directory");
        let mut apps = Vec::new();
        let release = WorkspaceRelease {
            tag_name: "v2.0.0".into(),
            description: Some("Fixed a crash and improved startup.".into()),
            is_draft: false,
            is_prerelease: Some(false),
            published_at: Some("2026-01-02T00:00:00Z".into()),
            assets: vec![WorkspaceAsset {
                name: "Sample-arm64.dmg".into(),
                download_url: "https://example.com/Sample-arm64.dmg".into(),
                size: Some(42),
            }],
        };
        for id in [1, 2] {
            let project = WorkspaceProject {
                id,
                name: format!("Project {id}"),
                directory: dir.path().to_path_buf(),
                provider: ProviderChoice::GitHub,
                account: "owner".into(),
                custom_host: None,
                credential_ref: None,
                disk_layout: DiskLayout::Flat,
                sources: Vec::new(),
                repository_list_mode: RepositoryListMode::Grouped,
            };
            let repos = vec![RepositoryReleases {
                provider: "github".into(),
                owner: "owner".into(),
                repository: "sample".into(),
                result: Ok(vec![release.clone()]),
            }];
            merge_project(&mut apps, id, apps_for_project(&project, &repos));
        }
        assert_eq!(apps.len(), 2);
        assert_ne!(apps[0].key, apps[1].key);
        assert_eq!(
            apps[0].latest().expect("release").notes,
            "Fixed a crash and improved startup."
        );
        assert_eq!(
            apps[0].latest().expect("release").assets[0].platform,
            "macos"
        );
        assert_eq!(
            apps[0].latest().expect("release").assets[0].local_path,
            Some(dir.path().join("Release Dmgs/Sample-arm64.dmg"))
        );
        let unrelated = &apps[0].latest().expect("release").assets[0];
        assert!(!apps[0].asset_belongs_to_app(unrelated));
        let mut owned = unrelated.clone();
        owned.download_url =
            "https://github.com/owner/sample/releases/download/v2/Sample.dmg".into();
        assert!(apps[0].asset_belongs_to_app(&owned));
        owned.download_url =
            "https://github.com/owner/another-app/releases/download/v2/Sample.dmg".into();
        assert!(!apps[0].asset_belongs_to_app(&owned));
        let path = catalog_path(dir.path());
        save(&path, &apps).expect("save catalog");
        let restored = load(&path).expect("load catalog");
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].project_directory, dir.path());
        let migrated = dir.path().join("migration/catalog.sqlite3");
        std::fs::create_dir_all(migrated.parent().unwrap()).unwrap();
        std::fs::write(
            migrated.with_file_name("catalog.json"),
            serde_json::to_vec(&apps).unwrap(),
        )
        .unwrap();
        assert_eq!(load(&migrated).unwrap().len(), 2);
        assert!(migrated.is_file());
    }

    #[test]
    fn artwork_and_bundle_names_cannot_escape_repository() {
        let dir = tempfile::tempdir().expect("temp directory");
        std::fs::write(dir.path().join("icon.png"), b"png").expect("write icon");
        assert!(local_artwork(dir.path(), "icon.png").is_some());
        assert!(local_artwork(dir.path(), "../icon.png").is_none());
        assert!(safe_bundle_name("Sample.app"));
        assert!(!safe_bundle_name("../Sample.app"));
    }

    #[test]
    fn release_notes_remain_visible_without_a_platform_installer() {
        let dir = tempfile::tempdir().expect("temp directory");
        let project = WorkspaceProject {
            id: 3,
            name: "Docs".into(),
            directory: dir.path().to_path_buf(),
            provider: ProviderChoice::GitHub,
            account: "owner".into(),
            custom_host: None,
            credential_ref: None,
            disk_layout: DiskLayout::Flat,
            sources: Vec::new(),
            repository_list_mode: RepositoryListMode::Grouped,
        };
        let repos = vec![RepositoryReleases {
            provider: "github".into(),
            owner: "owner".into(),
            repository: "docs".into(),
            result: Ok(vec![WorkspaceRelease {
                tag_name: "v1".into(),
                description: Some("New guide".into()),
                is_draft: false,
                is_prerelease: Some(false),
                published_at: None,
                assets: Vec::new(),
            }]),
        }];
        let apps = apps_for_project(&project, &repos);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].latest().expect("release").notes, "New guide");
        assert!(apps[0].platform_asset().is_none());
    }

    #[test]
    fn repository_manifest_supplies_real_store_artwork_and_metadata() {
        let dir = tempfile::tempdir().expect("temp directory");
        let repo_dir = dir.path().join("sample");
        std::fs::create_dir_all(repo_dir.join("assets")).expect("assets directory");
        image::RgbaImage::new(512, 512)
            .save(repo_dir.join("assets/icon.png"))
            .expect("icon");
        std::fs::write(repo_dir.join("assets/screen.png"), b"image").expect("screenshot");
        std::fs::write(
            repo_dir.join("release-desk-store.json"),
            br#"{
            "name":"Sample Studio", "description":"Edits samples", "category":"Creative",
            "icon":"assets/icon.png", "images":["assets/screen.png", "../outside.png"],
            "bundle_name":"Sample Studio.app"
        }"#,
        )
        .expect("manifest");
        let project = WorkspaceProject {
            id: 5,
            name: "Creative apps".into(),
            directory: dir.path().to_path_buf(),
            provider: ProviderChoice::GitHub,
            account: "owner".into(),
            custom_host: None,
            credential_ref: None,
            disk_layout: DiskLayout::Flat,
            sources: Vec::new(),
            repository_list_mode: RepositoryListMode::Grouped,
        };
        let repos = vec![RepositoryReleases {
            provider: "github".into(),
            owner: "owner".into(),
            repository: "sample".into(),
            result: Ok(vec![WorkspaceRelease {
                tag_name: "v1".into(),
                description: Some("First release".into()),
                is_draft: false,
                is_prerelease: Some(false),
                published_at: None,
                assets: Vec::new(),
            }]),
        }];
        let apps = apps_for_project(&project, &repos);
        assert_eq!(apps[0].name, "Sample Studio");
        assert_eq!(apps[0].description, "Edits samples");
        assert_eq!(apps[0].category, "Creative");
        assert_eq!(apps[0].bundle_name, "Sample Studio.app");
        assert_eq!(
            apps[0].icon_path,
            Some(repo_dir.join("assets/icon.png").canonicalize().unwrap())
        );
        assert_eq!(
            apps[0].images,
            vec![repo_dir.join("assets/screen.png").canonicalize().unwrap()]
        );
    }

    #[test]
    fn screenshots_are_discovered_only_from_app_image_locations() {
        let dir = tempfile::tempdir().unwrap();
        let screenshots = dir.path().join("docs/images");
        std::fs::create_dir_all(&screenshots).unwrap();
        std::fs::write(screenshots.join("ui-layout.png"), b"image").unwrap();
        std::fs::write(screenshots.join("logo.png"), b"logo").unwrap();
        let images = discover_images(dir.path());
        assert_eq!(
            images,
            vec![screenshots.join("ui-layout.png").canonicalize().unwrap()]
        );
    }

    #[test]
    fn store_lists_only_verified_apps_for_the_running_platform() {
        let extension = match std::env::consts::OS {
            "macos" => "dmg",
            "windows" => "exe",
            _ => "AppImage",
        };
        let mut app = StoreApp {
            key: "1:github:owner/sample".into(),
            project_id: 1,
            project_name: "Project".into(),
            project_directory: PathBuf::from("/tmp"),
            provider: "github".into(),
            owner: "owner".into(),
            repository: "sample".into(),
            name: "sample".into(),
            description: "Sample app".into(),
            category: "Other".into(),
            icon_path: None,
            images: Vec::new(),
            bundle_name: "sample.app".into(),
            releases: vec![StoreRelease {
                tag: "v1".into(),
                notes: String::new(),
                published_at: None,
                assets: vec![StoreAsset {
                    name: format!("sample.{extension}"),
                    download_url: format!(
                        "https://github.com/owner/sample/releases/download/v1/sample.{extension}"
                    ),
                    size: None,
                    platform: std::env::consts::OS.into(),
                    architecture: None,
                    local_path: None,
                }],
            }],
            show_in_store: None,
        };
        assert!(app.is_app_listing());
        assert_eq!(app.display_name(), "Sample");
        app.show_in_store = Some(false);
        assert!(!app.is_app_listing());
        app.show_in_store = None;
        app.releases[0].assets[0].download_url =
            "https://github.com/owner/other/releases/download/v1/sample.dmg".into();
        assert!(!app.is_app_listing());
    }
}
