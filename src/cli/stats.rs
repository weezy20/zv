use crate::app::config;
use crate::shell::path_utils::check_dir_in_path_for_shell;
use crate::tools::{ZvPaths, canonicalize};
use crate::{App, ResolvedZigVersion, Result, Shell};
use serde::Serialize;
use std::path::{Path, PathBuf};
use yansi::Paint;

// ─── data model ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum LayoutKind {
    Xdg,
    MacLibrary,
    WindowsHome,
    EnvOverride,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum EntryKind {
    File,
    Dir,
    Symlink { target: String, dangling: bool },
    Missing,
}

/// 0 = not a TTL file, 1 = expiring soon (within 3 days), 2 = expired
type StaleLevel = u8;

#[derive(Debug, Serialize)]
struct Entry {
    name: String,
    #[serde(serialize_with = "ser_path")]
    path: PathBuf,
    kind: EntryKind,
    size: u64,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation: Option<String>,
    #[serde(skip)]
    stale: StaleLevel,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<Entry>,
}

#[derive(Debug, Serialize)]
struct Group {
    title: &'static str,
    #[serde(serialize_with = "ser_path")]
    root: PathBuf,
    root_exists: bool,
    size: u64,
    entries: Vec<Entry>,
}

#[derive(Debug, Serialize)]
struct PathCheck {
    #[serde(serialize_with = "ser_path")]
    current_exe: PathBuf,
    #[serde(serialize_with = "ser_path")]
    resolved_dir: PathBuf,
    in_path: bool,
    matching_entry: Option<String>,
    #[serde(serialize_with = "ser_path")]
    expected_dir: PathBuf,
    expected_in_path: bool,
}

#[derive(Debug, Serialize)]
struct StatsReport {
    layout: LayoutKind,
    using_env_var: bool,
    zv_version: &'static str,
    active_zig: Option<String>,
    groups: Vec<Group>,
    path_check: PathCheck,
}

fn ser_path<S: serde::Serializer>(p: &Path, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.serialize_str(&p.to_string_lossy())
}

// ─── entry point ─────────────────────────────────────────────────────────────

pub async fn run(app: &App, verbose: bool, json: bool, no_color: bool) -> Result<()> {
    if no_color || json {
        yansi::disable();
    }
    let report = collect(app, verbose);
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        render_tree(&report);
    }
    Ok(())
}

// ─── collectors ──────────────────────────────────────────────────────────────

fn collect(app: &App, verbose: bool) -> StatsReport {
    let paths = &app.paths;
    let active_zig = app.get_active_version().map(|v| v.to_string());
    let layout = detect_layout(paths);
    let fold_config = paths.config_dir == paths.data_dir;
    let fold_cache = paths.cache_dir == paths.data_dir;
    let zls_cfg = config::load_zv_config(&paths.config_file).ok();

    let mut groups = Vec::new();
    groups.push(collect_data(
        app,
        fold_config,
        fold_cache,
        &zls_cfg,
        verbose,
    ));
    if !fold_config {
        groups.push(collect_config(paths, &zls_cfg));
    }
    if !fold_cache {
        groups.push(collect_cache(paths, verbose));
    }
    if let Some(ref pub_dir) = paths.public_bin_dir {
        groups.push(collect_public_bin(pub_dir));
    }

    StatsReport {
        layout,
        using_env_var: paths.using_env_var,
        zv_version: env!("CARGO_PKG_VERSION"),
        active_zig,
        groups,
        path_check: build_path_check(paths),
    }
}

fn detect_layout(paths: &ZvPaths) -> LayoutKind {
    if paths.using_env_var {
        return LayoutKind::EnvOverride;
    }
    if cfg!(windows) {
        return LayoutKind::WindowsHome;
    }
    #[cfg(target_os = "macos")]
    if paths.tier == 2 {
        return LayoutKind::MacLibrary;
    }
    LayoutKind::Xdg
}

fn collect_data(
    app: &App,
    fold_config: bool,
    fold_cache: bool,
    zls_cfg: &Option<config::ZvConfig>,
    verbose: bool,
) -> Group {
    let paths = &app.paths;
    let mut entries = Vec::new();

    // bin/
    let bin_children = bin_entries(&paths.bin_dir);
    let bin_size: u64 = bin_children.iter().map(|e| e.size).sum();
    entries.push(Entry {
        name: "bin/".into(),
        path: paths.bin_dir.clone(),
        kind: EntryKind::Dir,
        size: bin_size,
        active: false,
        annotation: None,
        stale: 0,
        children: bin_children,
    });

    // versions/
    let (ver_children, ver_size) = version_entries(app);
    let ver_count = ver_children.len();
    entries.push(Entry {
        name: "versions/".into(),
        path: paths.versions_dir.clone(),
        kind: EntryKind::Dir,
        size: ver_size,
        active: false,
        annotation: Some(format!("{} installed", ver_count)),
        stale: 0,
        children: ver_children,
    });

    // zls/
    let zls_dir = paths.zls_dir();
    let (zls_children, zls_size) = zls_build_entries(&zls_dir, zls_cfg);
    let zls_count = zls_children.len();
    entries.push(Entry {
        name: "zls/".into(),
        path: zls_dir,
        kind: EntryKind::Dir,
        size: zls_size,
        active: false,
        annotation: Some(format!(
            "{} build{}",
            zls_count,
            if zls_count == 1 { "" } else { "s" }
        )),
        stale: 0,
        children: zls_children,
    });

    // shell env file
    let env_path = app.env_path().clone();
    if env_path.exists() {
        let size = std::fs::metadata(&env_path).map(|m| m.len()).unwrap_or(0);
        let name = env_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        entries.push(Entry {
            name,
            path: env_path,
            kind: EntryKind::File,
            size,
            active: false,
            annotation: Some("shell env file".into()),
            stale: 0,
            children: vec![],
        });
    }

    if fold_config {
        entries.push(toml_entry(&paths.config_file, zls_cfg));
    }
    if fold_cache {
        entries.extend(cache_file_entries(paths, verbose));
    }

    let size = entries.iter().map(|e| e.size).sum();
    Group {
        title: "Data",
        root: paths.data_dir.clone(),
        root_exists: paths.data_dir.is_dir(),
        size,
        entries,
    }
}

fn collect_config(paths: &ZvPaths, zls_cfg: &Option<config::ZvConfig>) -> Group {
    let entry = toml_entry(&paths.config_file, zls_cfg);
    let size = entry.size;
    Group {
        title: "Config",
        root: paths.config_dir.clone(),
        root_exists: paths.config_dir.is_dir(),
        size,
        entries: vec![entry],
    }
}

fn toml_entry(config_file: &Path, zls_cfg: &Option<config::ZvConfig>) -> Entry {
    let size = std::fs::metadata(config_file).map(|m| m.len()).unwrap_or(0);
    let annotation = zls_cfg.as_ref().map(|c| {
        let active = c
            .active_zig
            .as_ref()
            .map(|a| a.version.as_str())
            .unwrap_or("none");
        let n = c.zls.as_ref().map(|z| z.mappings.len()).unwrap_or(0);
        format!(
            "active={active}, {n} zls mapping{}",
            if n == 1 { "" } else { "s" }
        )
    });
    Entry {
        name: "zv.toml".into(),
        kind: if config_file.exists() {
            EntryKind::File
        } else {
            EntryKind::Missing
        },
        path: config_file.to_path_buf(),
        size,
        active: false,
        annotation,
        stale: 0,
        children: vec![],
    }
}

fn collect_cache(paths: &ZvPaths, verbose: bool) -> Group {
    let entries = cache_file_entries(paths, verbose);
    let size = entries.iter().map(|e| e.size).sum();
    Group {
        title: "Cache",
        root: paths.cache_dir.clone(),
        root_exists: paths.cache_dir.is_dir(),
        size,
        entries,
    }
}

fn cache_file_entries(paths: &ZvPaths, verbose: bool) -> Vec<Entry> {
    let mut out = Vec::new();
    let ttl_index = *crate::app::INDEX_TTL_DAYS;
    let ttl_mirrors = *crate::app::MIRRORS_TTL_DAYS;

    // downloads/
    let dl = &paths.downloads_dir;
    let dl_size = if dl.is_dir() { dir_size(dl) } else { 0 };
    let dl_items = if dl.is_dir() {
        std::fs::read_dir(dl).map(|r| r.count()).unwrap_or(0)
    } else {
        0
    };
    let dl_children = if verbose && dl.is_dir() {
        flat_dir_entries(dl)
    } else {
        vec![]
    };
    out.push(Entry {
        name: "downloads/".into(),
        path: dl.clone(),
        kind: EntryKind::Dir,
        size: dl_size,
        active: false,
        annotation: Some(if dl_items == 0 {
            "empty".into()
        } else {
            format!("{dl_items} item{}", if dl_items == 1 { "" } else { "s" })
        }),
        stale: 0,
        children: dl_children,
    });

    // zls-src/
    let src = paths.zls_src_dir();
    let src_size = if src.is_dir() { dir_size(&src) } else { 0 };
    let git_ref = if src.is_dir() {
        read_git_head(&src)
    } else {
        None
    };
    out.push(Entry {
        name: "zls-src/".into(),
        path: src,
        kind: EntryKind::Dir,
        size: src_size,
        active: false,
        annotation: git_ref.map(|r| format!("ref: {r}")),
        stale: 0,
        children: vec![],
    });

    // index.toml
    out.push(ttl_entry(&paths.index_file, "index.toml", ttl_index));
    // mirrors.toml
    out.push(ttl_entry(&paths.mirrors_file, "mirrors.toml", ttl_mirrors));

    // master
    let mf = &paths.master_file;
    let master_val = std::fs::read_to_string(mf)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    out.push(Entry {
        name: "master".into(),
        path: mf.clone(),
        kind: if mf.exists() {
            EntryKind::File
        } else {
            EntryKind::Missing
        },
        size: std::fs::metadata(mf).map(|m| m.len()).unwrap_or(0),
        active: false,
        annotation: master_val,
        stale: 0,
        children: vec![],
    });

    out
}

fn ttl_entry(path: &Path, name: &str, ttl_days: i64) -> Entry {
    let exists = path.exists();
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let (annotation, stale) = if exists {
        let age = file_age_days(path).unwrap_or(0);
        let stale = if age > ttl_days {
            2
        } else if ttl_days - age <= 3 {
            1
        } else {
            0
        };
        (
            Some(format!("{age} day{} old", if age == 1 { "" } else { "s" })),
            stale,
        )
    } else {
        (None, 0)
    };
    Entry {
        name: name.into(),
        path: path.to_path_buf(),
        kind: if exists {
            EntryKind::File
        } else {
            EntryKind::Missing
        },
        size,
        active: false,
        annotation,
        stale,
        children: vec![],
    }
}

fn collect_public_bin(pub_dir: &Path) -> Group {
    // Only surface the three zv-managed shims, not the entire ~/.local/bin.
    let shim_names = [
        crate::Shim::Zv.executable_name(),
        crate::Shim::Zig.executable_name(),
        crate::Shim::Zls.executable_name(),
    ];
    let mut entries = Vec::new();
    for name in shim_names {
        let path = pub_dir.join(name);
        let meta = std::fs::symlink_metadata(&path);
        let (kind, size) = match meta {
            Err(_) => (EntryKind::Missing, 0),
            Ok(m) if m.file_type().is_symlink() => {
                let dangling = !path.exists();
                let target = std::fs::read_link(&path)
                    .ok()
                    .map(|t| {
                        t.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| t.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| "?".into());
                (EntryKind::Symlink { target, dangling }, m.len())
            }
            Ok(m) => (EntryKind::File, m.len()),
        };
        entries.push(Entry {
            name: name.to_string(),
            path,
            kind,
            size,
            active: false,
            annotation: None,
            stale: 0,
            children: vec![],
        });
    }
    let size = entries.iter().map(|e| e.size).sum();
    Group {
        title: "Public bin",
        root: pub_dir.to_path_buf(),
        root_exists: pub_dir.is_dir(),
        size,
        entries,
    }
}

fn bin_entries(dir: &Path) -> Vec<Entry> {
    if !dir.is_dir() {
        return vec![];
    }
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    names.sort_by_key(|e| e.file_name());
    names
        .into_iter()
        .map(|de| {
            let path = de.path();
            let name = de.file_name().to_string_lossy().into_owned();
            let meta = std::fs::symlink_metadata(&path);
            let (kind, size) = match meta {
                Err(_) => (EntryKind::Missing, 0),
                Ok(m) if m.file_type().is_symlink() => {
                    let dangling = !path.exists();
                    let target = std::fs::read_link(&path)
                        .ok()
                        .map(|t| {
                            t.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| t.to_string_lossy().into_owned())
                        })
                        .unwrap_or_else(|| "?".into());
                    (EntryKind::Symlink { target, dangling }, m.len())
                }
                Ok(m) => (EntryKind::File, m.len()),
            };
            Entry {
                name,
                path,
                kind,
                size,
                active: false,
                annotation: None,
                stale: 0,
                children: vec![],
            }
        })
        .collect()
}

fn version_entries(app: &App) -> (Vec<Entry>, u64) {
    let versions_dir = &app.paths.versions_dir;
    let mut installs = app.toolchain_manager.list_installations();
    installs.sort_by(|a, b| b.0.cmp(&a.0));

    let mut entries = Vec::new();
    let mut total = 0u64;

    for (version, is_active, is_master) in &installs {
        let rzv = if *is_master {
            ResolvedZigVersion::Master(version.clone())
        } else {
            ResolvedZigVersion::Semver(version.clone())
        };
        let dir = app
            .toolchain_manager
            .is_version_installed(&rzv)
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| versions_dir.join(version.to_string()));

        let size = if dir.is_dir() { dir_size(&dir) } else { 0 };
        total += size;

        entries.push(Entry {
            name: version.to_string(),
            path: dir,
            kind: EntryKind::Dir,
            size,
            active: *is_active,
            annotation: if *is_master {
                Some("master".into())
            } else {
                None
            },
            stale: 0,
            children: vec![],
        });
    }
    (entries, total)
}

fn zls_build_entries(zls_dir: &Path, zls_cfg: &Option<config::ZvConfig>) -> (Vec<Entry>, u64) {
    if !zls_dir.is_dir() {
        return (vec![], 0);
    }
    let reverse: std::collections::HashMap<String, String> = zls_cfg
        .as_ref()
        .and_then(|c| c.zls.as_ref())
        .map(|z| {
            z.mappings
                .iter()
                .map(|(zig, zls)| (zls.clone(), zig.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut names: Vec<_> = std::fs::read_dir(zls_dir)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    names.sort_by_key(|e| e.file_name());

    let mut entries = Vec::new();
    let mut total = 0u64;
    for de in names {
        let path = de.path();
        if !path.is_dir() {
            continue;
        }
        let name = de.file_name().to_string_lossy().into_owned();
        let size = dir_size(&path);
        total += size;
        let annotation = reverse.get(&name).map(|zig| format!("for Zig {zig}"));
        entries.push(Entry {
            name,
            path,
            kind: EntryKind::Dir,
            size,
            active: false,
            annotation,
            stale: 0,
            children: vec![],
        });
    }
    (entries, total)
}

fn flat_dir_entries(dir: &Path) -> Vec<Entry> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    names.sort_by_key(|e| e.file_name());
    names
        .into_iter()
        .map(|de| {
            let path = de.path();
            let name = de.file_name().to_string_lossy().into_owned();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            Entry {
                name,
                path,
                kind: EntryKind::File,
                size,
                active: false,
                annotation: None,
                stale: 0,
                children: vec![],
            }
        })
        .collect()
}

// ─── PATH check ──────────────────────────────────────────────────────────────

fn build_path_check(paths: &ZvPaths) -> PathCheck {
    let shell = Shell::detect();
    let current_exe = std::env::current_exe()
        .ok()
        .and_then(|p| canonicalize(&p).ok())
        .unwrap_or_default();
    let resolved_dir = current_exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let expected_dir = paths
        .public_bin_dir
        .as_ref()
        .unwrap_or(&paths.bin_dir)
        .clone();
    let expected_in_path =
        expected_dir.is_dir() && check_dir_in_path_for_shell(&shell, &expected_dir);

    let path_var = std::env::var("PATH").unwrap_or_default();
    let sep = shell.get_path_separator();
    let (in_path, matching_entry) = find_in_path(&path_var, sep, &resolved_dir);

    PathCheck {
        current_exe,
        resolved_dir,
        in_path,
        matching_entry,
        expected_dir,
        expected_in_path,
    }
}

fn find_in_path(path_var: &str, sep: char, target: &Path) -> (bool, Option<String>) {
    if target == Path::new("") {
        return (false, None);
    }
    for raw in path_var.split(sep) {
        if raw.is_empty() {
            continue;
        }
        let p = Path::new(raw);
        if !p.is_dir() {
            continue;
        }
        if let Ok(c) = canonicalize(p)
            && c == target
        {
            return (true, Some(raw.to_string()));
        }
    }
    (false, None)
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn file_age_days(path: &Path) -> Option<i64> {
    let elapsed = std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .elapsed()
        .ok()?;
    Some(elapsed.as_secs() as i64 / 86400)
}

fn read_git_head(dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(dir.join(".git/HEAD")).ok()?;
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        Some(branch.to_string())
    } else if head.len() >= 7 {
        Some(head[..7].to_string())
    } else {
        Some(head.to_string())
    }
}

// ─── renderer ────────────────────────────────────────────────────────────────

fn render_tree(report: &StatsReport) {
    let layout_str = match report.layout {
        LayoutKind::Xdg => "XDG",
        LayoutKind::MacLibrary => "macOS Library",
        LayoutKind::WindowsHome => "Windows",
        LayoutKind::EnvOverride => "ZV_DIR override",
    };
    let env_badge = if report.using_env_var {
        format!("  {}", Paint::yellow("(ZV_DIR)").italic())
    } else {
        String::new()
    };
    println!();
    println!(
        "zv {}  —  layout: {}{}",
        report.zv_version.yellow(),
        Paint::cyan(layout_str).bold(),
        env_badge
    );

    match &report.active_zig {
        Some(v) => println!("active zig: {}", Paint::green(v).bold()),
        None => println!("active zig: {}", Paint::new("none").dim()),
    }
    println!();

    for group in &report.groups {
        render_group(group);
        println!();
    }

    render_path_check(&report.path_check);
    println!();
}

fn render_group(group: &Group) {
    let root = group.root.display().to_string();
    let size_str = human_size(group.size);

    if group.root_exists {
        println!(
            "{:<12}  {}  ({})",
            Paint::cyan(group.title).bold(),
            Paint::yellow(&root),
            Paint::new(&size_str).dim()
        );
    } else {
        println!(
            "{:<12}  {}  {}",
            Paint::cyan(group.title).bold(),
            Paint::yellow(&root),
            Paint::red("(does not exist)")
        );
    }

    let n = group.entries.len();
    for (i, entry) in group.entries.iter().enumerate() {
        render_entry(entry, "", i == n - 1);
    }
}

fn render_entry(entry: &Entry, prefix: &str, is_last: bool) {
    let connector = if is_last { "└─ " } else { "├─ " };
    let child_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });

    // name portion — active gets ★ + green bold, annotation "master" gets yellow suffix
    let name_part: String = if entry.active {
        format!("{} {}", "★".green(), Paint::green(&entry.name).bold())
    } else {
        let master = entry.annotation.as_deref() == Some("master");
        if master {
            format!("{}  {}", entry.name, Paint::yellow("(master)").italic())
        } else {
            entry.name.clone()
        }
    };

    // symlink / missing suffix
    let kind_suffix = match &entry.kind {
        EntryKind::Symlink { target, dangling } => {
            if *dangling {
                format!("  →  {}  {}", target, Paint::red("(dangling)"))
            } else {
                format!("  →  {}", Paint::new(target).dim())
            }
        }
        EntryKind::Missing => format!("  {}", Paint::red("(missing)")),
        _ => String::new(),
    };

    // trailing (size[, annotation]) — skip symlinks; annotation already handled for "master"
    let trail = match &entry.kind {
        EntryKind::Missing => String::new(),
        EntryKind::Symlink { .. } => String::new(),
        _ => {
            let sz = Paint::new(human_size(entry.size)).dim().to_string();
            let ann = match entry.annotation.as_deref() {
                None | Some("master") => String::new(),
                Some(a) => {
                    let age_colored = if entry.stale == 2 {
                        Paint::red(a).to_string()
                    } else if entry.stale == 1 {
                        Paint::yellow(a).to_string()
                    } else {
                        Paint::new(a).dim().to_string()
                    };
                    format!(", {age_colored}")
                }
            };
            format!("  ({sz}{ann})")
        }
    };

    println!(
        "{}{}{}{}{}",
        prefix, connector, name_part, kind_suffix, trail
    );

    let cn = entry.children.len();
    for (i, child) in entry.children.iter().enumerate() {
        render_entry(child, &child_prefix, i == cn - 1);
    }
}

fn render_path_check(pc: &PathCheck) {
    println!("{}", Paint::cyan("PATH").bold());

    let exe = pc.current_exe.display().to_string();
    println!("├─ invoked from: {}", Paint::yellow(&exe));

    if pc.in_path {
        let resolved_fallback = pc.resolved_dir.display().to_string();
        let entry = pc.matching_entry.as_deref().unwrap_or(&resolved_fallback);
        println!("├─ $PATH hit: {}  {}", Paint::yellow(entry), "✔".green());
    } else {
        let dir = pc.resolved_dir.display().to_string();
        println!(
            "├─ $PATH hit: {}  {}",
            Paint::yellow(&dir),
            "✘ not in PATH".red()
        );
    }

    let expected = pc.expected_dir.display().to_string();
    if pc.expected_in_path {
        println!(
            "└─ expected bin: {}  {}",
            Paint::yellow(&expected),
            "✔ in PATH".green()
        );
    } else {
        println!(
            "└─ expected bin: {}  {}  — run {}",
            Paint::yellow(&expected),
            "✘ not in PATH".red(),
            crate::tools::format_cmd("zv setup")
        );
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_boundaries() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn find_in_path_matching() {
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_string_lossy();
        let path_var = format!("/nonexistent:{tmp_str}:/other");
        let canonical = canonicalize(&tmp).unwrap_or(tmp.clone());
        let (found, entry) = find_in_path(&path_var, ':', &canonical);
        assert!(found);
        assert_eq!(entry.as_deref(), Some(tmp_str.as_ref()));
    }

    #[test]
    fn find_in_path_skips_empty() {
        let (found, _) = find_in_path(":/nonexistent:", ':', Path::new("/some/dir"));
        assert!(!found);
    }
}
