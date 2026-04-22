use crate::app::network::zls as zls_api;
use crate::app::zls_download::download_zls_prebuilt;
use crate::app::zls_source::build_zls_from_source;
use crate::{App, Shim, suggest};
use color_eyre::eyre::eyre;
use std::path::Path;

fn active_zig_semver_string(active: &crate::ZigVersion) -> Option<String> {
    active.version().map(|v| v.to_string())
}

pub(crate) async fn provision_zls(
    app: &mut App,
    download: bool,
    force: bool,
    update: bool,
) -> crate::Result<()> {
    if !app.is_initialized() {
        crate::tools::error(
            "zv is not initialized. Run 'zv sync' first to set up directories and the zv binary.",
        );
        std::process::exit(1);
    }

    let active_zig = app.get_active_version().ok_or_else(|| {
        crate::tools::error("No active Zig version found");
        suggest!("Set one with {}", cmd = "zv use <version>");
        eyre!("No active Zig version")
    })?;

    let active_zig_exe = app
        .toolchain_manager
        .get_active_install()
        .map(|zi| zi.path.join(Shim::Zig.executable_name()))
        .ok_or_else(|| eyre!("No active Zig installation path found"))?;

    provision_zls_for(
        app,
        &active_zig,
        &active_zig_exe,
        download,
        force,
        update,
        true,
    )
    .await
}

pub(crate) async fn provision_zls_for(
    app: &mut App,
    zig_version: &crate::ZigVersion,
    zig_exe: &Path,
    download: bool,
    force: bool,
    update: bool,
    ensure_shim: bool,
) -> crate::Result<()> {
    let zig_version_string = active_zig_semver_string(zig_version).ok_or_else(|| {
        eyre!(
            "Unable to determine active Zig semantic version for '{}'.",
            zig_version
        )
    })?;

    if !force
        && !update
        && let Some((zls_version, zls_path)) = app.get_zls_for_zig(zig_version)
    {
        if ensure_shim && let Some(active_install) = app.toolchain_manager.get_active_install() {
            app.toolchain_manager
                .deploy_shims(active_install, false, true)
                .await?;
        }
        println!(
            "Compatible ZLS already provisioned: {} ({})",
            zls_version,
            zls_path.display()
        );
        return Ok(());
    }

    let release = zls_api::select_version(&zig_version_string)
        .await
        .map_err(|e| {
            eyre!(
                "Failed to query compatible ZLS for Zig '{}': {}",
                zig_version_string,
                e
            )
        })?;
    let host_target = crate::app::utils::host_target()
        .ok_or_else(|| eyre!("Could not determine host target for current machine"))?;

    let zls_dest_dir = app.paths.zls_dir().join(&release.version);
    let zls_binary = if download {
        download_zls_prebuilt(app, &release, &host_target, &zls_dest_dir).await?
    } else {
        let zls_src_dir = app.paths.zls_src_dir();
        build_zls_from_source(&release.version, zig_exe, &zls_src_dir, &zls_dest_dir).await?
    };

    app.record_zls_mapping(zig_version, &release.version)?;

    if ensure_shim && let Some(active_install) = app.toolchain_manager.get_active_install() {
        app.toolchain_manager
            .deploy_shims(active_install, false, true)
            .await?;
    }

    println!(
        "Provisioned ZLS {} (release date {}) for Zig {} at {}",
        release.version,
        release.date,
        zig_version_string,
        zls_binary.display()
    );

    Ok(())
}
