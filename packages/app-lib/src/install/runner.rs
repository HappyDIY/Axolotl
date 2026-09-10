use super::events::{InstallProgressReporter, emit_install_job};
use super::model::{
    InstallCleanup, InstallContinuationState, InstallErrorContext,
    InstallErrorView, InstallJavaStep, InstallJobDisplay, InstallJobEventKind,
    InstallJobSnapshot, InstallJobState, InstallJobStatus, InstallPauseReason,
    InstallPhaseDetails, InstallPhaseId, InstallPostInstallEdit,
    InstallProgress, InstallRequest, InstallRollbackState, InstallTarget,
    InstanceUpgradeCompatibilityWarning, InstanceUpgradeDisplayNames,
    InstanceUpgradeExecution, InstanceUpgradeExternalChange,
    InstanceUpgradeExternalChangeKind, InstanceUpgradeResult,
    InstanceUpgradeWatchBaseline, SharedUpgradeMode, initial_phase_for_request,
};
use super::{diagnostics, recovery, store};
use crate::ErrorKind;
use crate::api::pack::install_from::{
    CreatePackLocation, generate_pack_from_file,
    generate_pack_from_version_id_with_reporter, get_instance_from_pack,
};
use crate::api::pack::install_mrpack::{
    MrpackInstallOutcome, install_zipped_mrpack_files_with_reporter,
    related_file_paths,
};
use crate::event::InstancePayloadType;
use crate::event::emit::emit_instance;
use crate::state::{
    ContentProvider, ContentProviderRef, InstanceInstallStage, InstanceLink,
    InstanceUpgradeAction, InstanceUpgradeDependencyChangeKind,
    LoaderComponent, LoaderComponentKind, LoaderComponentRole, ModLoader,
    State,
};
use crate::util::fetch::DownloadReason;
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use uuid::Uuid;

mod lifecycle;
mod upgrade;
mod pack;

enum InstallExecutionOutcome<T> {
    Completed(T),
    WaitingForUser(InstallPauseReason),
}

pub async fn create_instance(
    name: String,
    game_version: String,
    loader: ModLoader,
    loader_version: Option<String>,
    icon_path: Option<String>,
    link: InstanceLink,
) -> crate::Result<InstallJobSnapshot> {
    create_instance_with_adjuncts(
        name,
        game_version,
        loader,
        loader_version,
        Vec::new(),
        icon_path,
        link,
        None,
    )
    .await
}

pub async fn create_instance_with_adjuncts(
    name: String,
    game_version: String,
    loader: ModLoader,
    loader_version: Option<String>,
    adjuncts: Vec<crate::state::LoaderComponent>,
    icon_path: Option<String>,
    link: InstanceLink,
    game_dir_override: Option<String>,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::CreateInstance {
        name,
        game_version,
        loader,
        loader_version,
        adjuncts,
        icon_path,
        link,
        game_dir_override,
    })
    .await
}

pub async fn create_modpack_instance(
    location: CreatePackLocation,
    post_install_edit: Option<InstallPostInstallEdit>,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::CreateModpackInstance {
        location,
        post_install_edit,
    })
    .await
}

pub async fn import_instance(
    launcher_type: crate::api::pack::import::ImportLauncherType,
    base_path: PathBuf,
    instance_folder: String,
    symlink: bool,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::ImportInstance {
        launcher_type,
        base_path,
        instance_folder,
        instance_path: None,
        symlink,
        game_version: None,
        loader: None,
        loader_version: None,
        game_dir_override: None,
    })
    .await
}

/// Like [`import_instance`] but with a pre-resolved filesystem path.
/// Used by the frontend when the path is already known from scanning,
/// avoiding redundant config/registry re-resolution.
pub async fn import_instance_with_path(
    launcher_type: crate::api::pack::import::ImportLauncherType,
    base_path: PathBuf,
    instance_folder: String,
    instance_path: Option<String>,
    symlink: bool,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::ImportInstance {
        launcher_type,
        base_path,
        instance_folder,
        instance_path,
        symlink,
        game_version: None,
        loader: None,
        loader_version: None,
        game_dir_override: None,
    })
    .await
}

pub async fn import_instance_with_plan(
    launcher_type: crate::api::pack::import::ImportLauncherType,
    base_path: PathBuf,
    instance_folder: String,
    instance_path: Option<String>,
    symlink: bool,
    game_version: Option<String>,
    loader: Option<crate::state::ModLoader>,
    loader_version: Option<String>,
    game_dir_override: Option<String>,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::ImportInstance {
        launcher_type,
        base_path,
        instance_folder,
        instance_path,
        symlink,
        game_version,
        loader,
        loader_version,
        game_dir_override,
    })
    .await
}

pub async fn duplicate_instance(
    source_instance_id: String,
) -> crate::Result<InstallJobSnapshot> {
    // Directly associated instances own no files to copy: duplicating one
    // would clone the linked launcher's `.minecraft` into Axolotl.
    let state = State::get().await?;
    if let Some(metadata) =
        crate::state::get_instance(&source_instance_id, &state.pool).await?
        && metadata.instance.is_direct_linked()
    {
        return Err(crate::ErrorKind::InputError(format!(
            "\"{}\" is directly associated with an external launcher and \
             cannot be duplicated; its files are managed by that launcher",
            metadata.instance.name
        ))
        .into());
    }
    drop(state);

    start(InstallRequest::DuplicateInstance { source_instance_id }).await
}

pub async fn install_existing_instance(
    instance_id: String,
    force: bool,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::InstallExistingInstance { instance_id, force }).await
}

pub async fn upgrade_unmanaged_instance(
    instance_id: String,
    plan_id: String,
    execution: InstanceUpgradeExecution,
    create_full_backup: bool,
    shared_upgrade_mode: SharedUpgradeMode,
    display_names: InstanceUpgradeDisplayNames,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::UpgradeUnmanagedInstance {
        instance_id,
        plan_id,
        execution,
        create_full_backup,
        shared_upgrade_mode,
        display_names,
    })
    .await
}

pub async fn install_content(
    instance_id: String,
    project_id: String,
    version_id: Option<String>,
    content_type: modrinth_content_management::ContentType,
    selected: modrinth_content_management::ResolutionPreferences,
    excluded_project_ids: Vec<String>,
    display_title: String,
    display_icon: Option<String>,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::InstallContent {
        instance_id,
        project_id,
        version_id,
        content_type,
        selected,
        excluded_project_ids,
        display_title,
        display_icon,
    })
    .await
}

pub async fn install_curseforge_content(
    request: crate::api::curseforge::CurseForgeInstallRequest,
    display_title: String,
    display_icon: Option<String>,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::InstallCurseForgeContent {
        request,
        display_title,
        display_icon,
    })
    .await
}

pub async fn install_curseforge_world(
    request: crate::api::curseforge::CurseForgeWorldInstallRequest,
    display_title: String,
    display_icon: Option<String>,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::InstallCurseForgeWorld {
        request,
        display_title,
        display_icon,
    })
    .await
}

pub async fn download_java(
    vendor: String,
    version: u32,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::DownloadJava { vendor, version }).await
}

pub async fn install_pack_to_existing_instance(
    instance_id: String,
    location: CreatePackLocation,
    post_install_edit: Option<InstallPostInstallEdit>,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::InstallPackToExistingInstance {
        instance_id,
        location,
        post_install_edit,
    })
    .await
}

pub async fn update_managed_curseforge_modpack(
    instance_id: String,
    file_id: u32,
) -> crate::Result<InstallJobSnapshot> {
    start(InstallRequest::UpdateManagedCurseForgeModpack {
        instance_id,
        file_id,
    })
    .await
}

pub async fn list_jobs(
    include_finished: bool,
) -> crate::Result<Vec<InstallJobSnapshot>> {
    let state = State::get().await?;
    Ok(store::list(include_finished, &state)
        .await?
        .into_iter()
        .map(|job| job.snapshot())
        .collect())
}

pub async fn get_job(job_id: Uuid) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    Ok(store::get_required(job_id, &state).await?.snapshot())
}

pub async fn job_support_details(job_id: Uuid) -> crate::Result<String> {
    let state = State::get().await?;
    let job = store::get_required(job_id, &state).await?;
    diagnostics::build_job_support_details(&job, &state).await
}

pub async fn retry_job(job_id: Uuid) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    let mut job = store::get_required(job_id, &state).await?;

    if !matches!(
        job.status,
        InstallJobStatus::Failed | InstallJobStatus::Interrupted
    ) {
        return Err(crate::ErrorKind::InputError(
            "Only failed or interrupted install jobs can be retried"
                .to_string(),
        )
        .into());
    }

    job.state.target = job.state.request.target();
    job.state.cleanup = job.state.request.cleanup();
    job.state.rollback = None;
    job.state.error = None;
    job.state.rollback_error = None;
    job.state.pause_reason = None;
    job.state.continuation = None;
    job.state.context = None;
    job.state.progress.phase = initial_phase_for_request(&job.state.request);
    job.state.progress.progress = None;
    job.state.progress.details = InstallPhaseDetails::Empty;
    job.state.progress.parallel = None;
    prepare_initial_instance(&mut job.state, &state).await?;
    job.state.record_event(InstallJobEventKind::JobQueued {
        kind: job.state.request.kind(),
    });

    let record = store::update_status(
        job_id,
        InstallJobStatus::Queued,
        &job.state,
        &state,
    )
    .await?;
    emit_install_job(&record.snapshot()).await?;
    lifecycle::spawn_job(job_id);

    // The spawned job may already have progressed (or finished) by the time
    // the command returns; hand the caller the freshest stored state.
    Ok(store::get_required(job_id, &state).await?.snapshot())
}

pub async fn repair_cache_and_retry_job(
    job_id: Uuid,
) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    let initial_job = store::get_required(job_id, &state).await?;
    let _ = validated_cache_repair_types(&initial_job)?;

    let operation_lock = state
        .install_job_operation_locks
        .entry(job_id)
        .or_default()
        .clone();
    let mut operation = operation_lock.lock().await;
    if operation.cache_repair_started {
        return Ok(store::get_required(job_id, &state).await?.snapshot());
    }

    let job = store::get_required(job_id, &state).await?;
    let cache_types = validated_cache_repair_types(&job)?;
    operation.cache_repair_started = true;

    if let Err(error) =
        crate::state::CachedEntry::purge_cache_types(&cache_types, &state.pool)
            .await
    {
        operation.cache_repair_started = false;
        return Err(crate::ErrorKind::OtherError(format!(
            "Project cache cleanup failed; retry was not started: {error}"
        ))
        .into());
    }

    retry_job(job_id).await.map_err(|error| {
        crate::ErrorKind::OtherError(format!(
            "Project cache was cleared, but retry could not be started: {error}"
        ))
        .into()
    })
}

fn validated_cache_repair_types(
    job: &store::InstallJobRecord,
) -> crate::Result<Vec<crate::state::CacheValueType>> {
    validated_cache_repair_types_for(job.status, job.state.error.as_ref())
}

fn validated_cache_repair_types_for(
    status: InstallJobStatus,
    error: Option<&InstallErrorView>,
) -> crate::Result<Vec<crate::state::CacheValueType>> {
    if !matches!(
        status,
        InstallJobStatus::Failed | InstallJobStatus::Interrupted
    ) {
        return Err(crate::ErrorKind::InputError(
            "Only failed or interrupted install jobs can repair cache"
                .to_string(),
        )
        .into());
    }
    let error = error.ok_or_else(|| {
        crate::ErrorKind::InputError(
            "Install job has no cache repair error".to_string(),
        )
    })?;
    if error.code != "cache_repair_required" {
        return Err(crate::ErrorKind::InputError(
            "Install job does not require cache repair".to_string(),
        )
        .into());
    }
    let cache_types = error
        .context
        .as_ref()
        .map(|context| context.cache_types.as_slice())
        .unwrap_or_default();
    if cache_types.is_empty() {
        return Err(crate::ErrorKind::InputError(
            "Install job has no repairable cache types".to_string(),
        )
        .into());
    }

    let mut validated = Vec::new();
    for cache_type in cache_types {
        let cache_type =
            crate::state::CacheValueType::from_repairable_str(cache_type)
                .ok_or_else(|| {
                    crate::ErrorKind::InputError(format!(
                        "Cache type is not repairable: {cache_type}"
                    ))
                })?;
        if !validated.contains(&cache_type) {
            validated.push(cache_type);
        }
    }
    Ok(validated)
}

pub async fn resume_job(job_id: Uuid) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    let job = store::get_required(job_id, &state).await?;
    if job.status != InstallJobStatus::WaitingForUser {
        return Err(crate::ErrorKind::InputError(
            "Only install jobs waiting for user action can be resumed"
                .to_string(),
        )
        .into());
    }

    queue_waiting_job(job_id, job.state, &state).await
}

pub async fn skip_missing_content_and_resume_job(
    job_id: Uuid,
) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    let job = store::get_required(job_id, &state).await?;
    if job.status != InstallJobStatus::WaitingForUser {
        return Err(crate::ErrorKind::InputError(
            "Only install jobs waiting for user action can skip missing content"
                .to_string(),
        )
        .into());
    }
    if matches!(
        job.state.request,
        InstallRequest::UpdateManagedCurseForgeModpack { .. }
    ) {
        return Err(crate::ErrorKind::InputError(
            "CurseForge modpack version updates cannot skip required manual downloads"
                .to_string(),
        )
        .into());
    }

    let mut current_missing_paths = job
        .snapshot()
        .items
        .into_iter()
        .filter(|item| {
            item.status == super::model::DownloadItemStatus::Failed
                || (item.status == super::model::DownloadItemStatus::Skipped
                    && item.manual_url.is_some())
        })
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let mut job_state = job.state;
    let InstallPauseReason::MissingRequiredContent { paths, .. } =
        job_state.pause_reason.as_ref().ok_or_else(|| {
            crate::ErrorKind::InputError(
                "Install job has no missing content to skip".to_string(),
            )
        })?;
    if current_missing_paths.is_empty() {
        current_missing_paths = paths.clone();
    }
    if current_missing_paths.is_empty() {
        return Err(crate::ErrorKind::InputError(
            "Install job has no missing content to skip".to_string(),
        )
        .into());
    }
    job_state
        .skipped_missing_content_paths
        .extend(current_missing_paths);
    job_state.skipped_missing_content_paths.sort_unstable();
    job_state.skipped_missing_content_paths.dedup();

    queue_waiting_job(job_id, job_state, &state).await
}

async fn queue_waiting_job(
    job_id: Uuid,
    mut job_state: InstallJobState,
    state: &State,
) -> crate::Result<InstallJobSnapshot> {
    prepare_resumed_job(&mut job_state);
    let Some(record) = store::update_status_if(
        job_id,
        InstallJobStatus::WaitingForUser,
        InstallJobStatus::Queued,
        &job_state,
        &state,
    )
    .await?
    else {
        return Err(crate::ErrorKind::InputError(
            "Install job is no longer waiting for user action".to_string(),
        )
        .into());
    };
    InstallProgressReporter::reset_job(job_id);
    emit_install_job(&record.snapshot()).await?;
    lifecycle::spawn_job(job_id);
    Ok(store::get_required(job_id, &state).await?.snapshot())
}

fn prepare_resumed_job(job_state: &mut InstallJobState) {
    job_state.pause_reason = None;
    job_state.error = None;
    job_state.rollback_error = None;
    job_state.context = None;
    job_state.active_downloads.clear();
    job_state.record_event(InstallJobEventKind::JobQueued {
        kind: job_state.request.kind(),
    });
}

pub async fn retry_job_as_new(
    job_id: Uuid,
) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    let job = store::get_required(job_id, &state).await?;
    if !matches!(
        job.status,
        InstallJobStatus::Failed
            | InstallJobStatus::Interrupted
            | InstallJobStatus::Canceled
    ) {
        return Err(crate::ErrorKind::InputError(
            "Only failed, interrupted, or canceled downloads can be retried"
                .to_string(),
        )
        .into());
    }
    let new_job = start(job.state.request).await?;
    // The spawned job may already have progressed (or finished) by the time
    // the command returns; hand the caller the freshest stored state.
    Ok(store::get_required(new_job.job_id, &state)
        .await?
        .snapshot())
}

pub async fn cancel_job(job_id: Uuid) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    let mut job = loop {
        let mut job = store::get_required(job_id, &state).await?;
        match job.status {
            InstallJobStatus::Running => {
                let Some(record) = store::update_status_if(
                    job_id,
                    InstallJobStatus::Running,
                    InstallJobStatus::Canceling,
                    &job.state,
                    &state,
                )
                .await?
                else {
                    continue;
                };
                if let Some(token) =
                    state.install_job_cancellations.get(&job_id)
                {
                    token.cancel();
                }
                emit_install_job(&record.snapshot()).await?;
                return Ok(record.snapshot());
            }
            InstallJobStatus::Canceling => return Ok(job.snapshot()),
            InstallJobStatus::Queued | InstallJobStatus::WaitingForUser => {
                let expected = job.status;
                begin_canceling_job(&mut job.state);
                let Some(record) = store::update_status_if(
                    job_id,
                    expected,
                    InstallJobStatus::Canceling,
                    &job.state,
                    &state,
                )
                .await?
                else {
                    continue;
                };
                emit_install_job(&record.snapshot()).await?;
                break record;
            }
            _ => {
                return Err(crate::ErrorKind::InputError(
                    "Only queued, running, or waiting install jobs can be canceled"
                        .to_string(),
                )
                .into());
            }
        }
    };

    let cleanup_succeeded =
        match recovery::apply_cleanup(&mut job.state, &state).await {
            Ok(()) => true,
            Err(error) => {
                job.state.rollback_error = Some(InstallErrorView::from_error(
                    "rollback_error",
                    InstallPhaseId::RollingBack,
                    &error,
                    None,
                ));
                job.state.record_event(InstallJobEventKind::RollbackFailed {
                    message: error.to_string(),
                });
                false
            }
        };
    recovery::finalize_rollback_state(&mut job.state, cleanup_succeeded);
    if cleanup_succeeded {
        clear_deleted_new_instance_id(&mut job.state);
    }
    let record = store::update_status(
        job_id,
        InstallJobStatus::Canceled,
        &job.state,
        &state,
    )
    .await?;
    emit_install_job(&record.snapshot()).await?;

    Ok(record.snapshot())
}

fn begin_canceling_job(job_state: &mut InstallJobState) {
    let canceled_phase = job_state.progress.phase;
    job_state.error = Some(InstallErrorView::from_message(
        "canceled",
        canceled_phase,
        "Install was canceled",
    ));
    job_state.pause_reason = None;
    job_state.record_event(InstallJobEventKind::JobCanceled {
        phase: canceled_phase,
    });
    job_state.progress.phase = InstallPhaseId::RollingBack;
    job_state.progress.progress = None;
    job_state.progress.details = InstallPhaseDetails::Empty;
    job_state.progress.parallel = None;
    job_state.record_event(InstallJobEventKind::RollbackStarted {
        cleanup: job_state.cleanup.clone(),
    });
}

pub async fn dismiss_job(job_id: Uuid) -> crate::Result<()> {
    let state = State::get().await?;
    store::dismiss(job_id, &state).await
}

pub async fn clear_job_history() -> crate::Result<u64> {
    let state = State::get().await?;
    store::clear_finished(&state).await
}

async fn start(request: InstallRequest) -> crate::Result<InstallJobSnapshot> {
    let state = State::get().await?;
    let id = Uuid::new_v4();
    let mut job_state = InstallJobState::new(request);
    prepare_initial_instance(&mut job_state, &state).await?;
    let record =
        match store::insert(id, &job_state, InstallJobStatus::Queued, &state)
            .await
        {
            Ok(record) => record,
            Err(error) => {
                return Err(cleanup_failed_initial_install(
                    &mut job_state,
                    &state,
                    error,
                )
                .await);
            }
        };
    emit_install_job(&record.snapshot()).await?;
    lifecycle::spawn_job(id);
    Ok(record.snapshot())
}

async fn cleanup_failed_initial_install(
    job_state: &mut InstallJobState,
    state: &State,
    error: crate::Error,
) -> crate::Error {
    match recovery::apply_cleanup(job_state, state).await {
        Ok(()) => error,
        Err(cleanup_error) => crate::ErrorKind::OtherError(format!(
            "Install initialization failed: {error}; cleanup also failed: {cleanup_error}"
        ))
        .into(),
    }
}

async fn prepare_initial_instance(
    job_state: &mut InstallJobState,
    state: &State,
) -> crate::Result<()> {
    match job_state.request.clone() {
        InstallRequest::CreateInstance {
            name,
            mut game_version,
            mut loader,
            mut loader_version,
            mut adjuncts,
            icon_path,
            link,
            game_dir_override,
        } => {
            if let InstanceLink::CurseForgeModpack {
                project_id,
                version_id,
            } = &link
            {
                let project_id = project_id.parse::<u32>().map_err(|_| {
                    ErrorKind::InputError(
                        "CurseForge project ID is invalid".to_string(),
                    )
                })?;
                let file_id = version_id.parse::<u32>().map_err(|_| {
                    ErrorKind::InputError(
                        "CurseForge file ID is invalid".to_string(),
                    )
                })?;
                let target = crate::api::curseforge::get_modpack_target(
                    project_id, file_id,
                )
                .await?;
                game_version = target.game_version;
                loader = target.loader;
                loader_version = target.loader_version;
                adjuncts.clear();
                job_state.request = InstallRequest::CreateInstance {
                    name: name.clone(),
                    game_version: game_version.clone(),
                    loader,
                    loader_version: loader_version.clone(),
                    adjuncts: Vec::new(),
                    icon_path: icon_path.clone(),
                    link: link.clone(),
                    game_dir_override: game_dir_override.clone(),
                };
            }
            resolve_required_adjuncts(
                &game_version,
                loader,
                &mut adjuncts,
                state,
            )
            .await?;
            job_state.request = InstallRequest::CreateInstance {
                name: name.clone(),
                game_version: game_version.clone(),
                loader,
                loader_version: loader_version.clone(),
                adjuncts: adjuncts.clone(),
                icon_path: icon_path.clone(),
                link: link.clone(),
                game_dir_override: game_dir_override.clone(),
            };
            let metadata = crate::api::instance::create(
                name,
                game_version,
                loader,
                loader_version,
                icon_path,
                link,
                None,
                game_dir_override,
            )
            .await?;
            if !adjuncts.is_empty() {
                let mut components = metadata.loader_components.clone();
                for adjunct in &mut adjuncts {
                    adjunct.instance_id = metadata.instance.id.clone();
                    adjunct.role = crate::state::LoaderComponentRole::Adjunct;
                }
                components.extend(adjuncts);
                validate_loader_components(&components)?;
                crate::state::instances::commands::replace_instance_loader_components(
					&metadata.instance.id,
					&components,
					&state.pool,
				)
				.await?;
            }
            set_display(
                job_state,
                metadata.instance.name,
                metadata.instance.icon_path,
            );
            set_instance_id(job_state, metadata.instance.id);
        }
        InstallRequest::CreateModpackInstance {
            location,
            post_install_edit,
        } => {
            let preview = get_instance_from_pack(location).await?;
            let name = post_install_edit
                .as_ref()
                .and_then(|edit| edit.name.clone())
                .unwrap_or_else(|| preview.name.clone());
            let icon_path = match post_install_edit
                .as_ref()
                .and_then(|edit| edit.icon_path.as_ref())
            {
                Some(icon_path) => icon_path.clone(),
                None => preview
                    .icon
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string())
                    .or_else(|| preview.icon_url.clone()),
            };
            let link = post_install_edit
                .as_ref()
                .and_then(|edit| edit.link.clone())
                .or_else(|| preview.link.clone())
                .unwrap_or(InstanceLink::Unmanaged);
            let metadata = crate::api::instance::create(
                name,
                preview.game_version,
                preview.modloader,
                preview.loader_version,
                icon_path,
                link,
                None,
                None,
            )
            .await?;
            set_display(
                job_state,
                metadata.instance.name,
                metadata.instance.icon_path,
            );
            set_instance_id(job_state, metadata.instance.id);
        }
        InstallRequest::ImportInstance {
            instance_folder,
            symlink: _,
            base_path: _,
            game_dir_override,
            ..
        } => {
            let metadata = crate::api::instance::create(
                instance_folder,
                "unknown".to_string(),
                ModLoader::Vanilla,
                None,
                None,
                InstanceLink::Unmanaged,
                None,
                game_dir_override,
            )
            .await?;
            set_display(
                job_state,
                metadata.instance.name,
                metadata.instance.icon_path,
            );
            set_instance_id(job_state, metadata.instance.id);
        }
        InstallRequest::DuplicateInstance { source_instance_id } => {
            let metadata =
                crate::state::get_instance(&source_instance_id, &state.pool)
                    .await?
                    .ok_or_else(|| {
                        crate::ErrorKind::InputError(
                            "Unknown instance".to_string(),
                        )
                    })?;
            let created = crate::api::instance::create(
                metadata.instance.name,
                metadata.applied_content_set.game_version,
                metadata.applied_content_set.loader,
                metadata.applied_content_set.loader_version,
                metadata.instance.icon_path,
                metadata.link,
                None,
                None,
            )
            .await?;
            set_display(
                job_state,
                created.instance.name,
                created.instance.icon_path,
            );
            set_instance_id(job_state, created.instance.id);
        }
        InstallRequest::UpgradeUnmanagedInstance {
            instance_id,
            shared_upgrade_mode,
            display_names,
            ..
        } => {
            let metadata =
                crate::state::get_instance(&instance_id, &state.pool)
                    .await?
                    .ok_or_else(|| {
                        crate::ErrorKind::InputError(
                            "Unknown upgrade source instance".to_string(),
                        )
                    })?;
            set_display(
                job_state,
                metadata.instance.name.clone(),
                metadata.instance.icon_path.clone(),
            );
            match shared_upgrade_mode {
                SharedUpgradeMode::Direct => {
                    prepare_existing_rollback(job_state, state, &instance_id)
                        .await?;
                }
                SharedUpgradeMode::CopyAndUpgrade => {
                    let created = crate::api::instance::create(
                        display_names.copy.unwrap_or_else(|| {
                            format!(
                                "{} (Upgraded Copy)",
                                metadata.instance.name
                            )
                        }),
                        metadata.applied_content_set.game_version.clone(),
                        metadata.applied_content_set.loader,
                        metadata.applied_content_set.loader_version.clone(),
                        metadata.instance.icon_path.clone(),
                        InstanceLink::Unmanaged,
                        None,
                        None,
                    )
                    .await?;
                    set_instance_id(job_state, created.instance.id.clone());
                    if let Err(error) = upgrade::clone_instance_loader_components(
                        &metadata.loader_components,
                        &created.instance.id,
                        state,
                    )
                    .await
                    {
                        return Err(cleanup_failed_initial_install(
                            job_state, state, error,
                        )
                        .await);
                    }
                }
            }
        }
        InstallRequest::InstallExistingInstance { instance_id, .. }
        | InstallRequest::InstallPackToExistingInstance {
            instance_id, ..
        }
        | InstallRequest::UpdateManagedCurseForgeModpack {
            instance_id, ..
        } => {
            prepare_existing_rollback(job_state, state, &instance_id).await?;
        }
        InstallRequest::InstallContent {
            instance_id,
            display_title,
            display_icon,
            ..
        } => {
            crate::state::get_instance(&instance_id, &state.pool)
                .await?
                .ok_or_else(|| {
                    crate::ErrorKind::InputError(format!(
                        "Unknown instance {instance_id}"
                    ))
                })?;
            set_display(job_state, display_title, display_icon);
        }
        InstallRequest::InstallCurseForgeContent {
            request,
            display_title,
            display_icon,
        } => {
            crate::state::get_instance(&request.instance_id, &state.pool)
                .await?
                .ok_or_else(|| {
                    crate::ErrorKind::InputError(format!(
                        "Unknown instance {}",
                        request.instance_id
                    ))
                })?;
            set_display(job_state, display_title, display_icon);
        }
        InstallRequest::InstallCurseForgeWorld {
            request,
            display_title,
            display_icon,
        } => {
            crate::state::get_instance(&request.instance_id, &state.pool)
                .await?
                .ok_or_else(|| {
                    crate::ErrorKind::InputError(format!(
                        "Unknown instance {}",
                        request.instance_id
                    ))
                })?;
            set_display(job_state, display_title, display_icon);
        }
        InstallRequest::DownloadJava { vendor, version } => {
            set_display(job_state, format!("Java {version} ({vendor})"), None);
        }
    }

    Ok(())
}


async fn run_request(
    job_id: Uuid,
    job_state: &mut InstallJobState,
    state: &State,
) -> crate::Result<InstallExecutionOutcome<Option<String>>> {
    match job_state.request.clone() {
        InstallRequest::CreateInstance {
            name,
            game_version,
            loader,
            loader_version: _,
            adjuncts,
            icon_path: _,
            link,
            game_dir_override: _,
        } => {
            let Some(instance_id) = current_instance_id(job_state) else {
                return Err(crate::ErrorKind::InputError(
                    "Install job is missing its instance id".to_string(),
                )
                .into());
            };
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::PreparingInstance,
                InstallPhaseDetails::Instance { name: name.clone() },
            )
            .await?;
            let reporter =
                InstallProgressReporter::new(job_id, job_state.clone());
            let mut parallel_minecraft_install = None;
            if let InstanceLink::CurseForgeModpack {
                project_id,
                version_id,
            } = link
            {
                let project_id = project_id.parse::<u32>().map_err(|_| {
                    ErrorKind::InputError(
                        "CurseForge project ID is invalid".to_string(),
                    )
                })?;
                let file_id = version_id.parse::<u32>().map_err(|_| {
                    ErrorKind::InputError(
                        "CurseForge file ID is invalid".to_string(),
                    )
                })?;
                crate::state::instances::commands::set_instance_install_stage(
                    &instance_id,
                    InstanceInstallStage::PackInstalling,
                    &state.pool,
                )
                .await?;
                emit_instance(&instance_id, InstancePayloadType::Edited)
                    .await?;
                parallel_minecraft_install = Some(
                    crate::api::pack::parallel_minecraft_install::ParallelMinecraftInstall::start(
                        instance_id.clone(),
                        reporter.clone(),
                    ),
                );
                let result = crate::api::curseforge::install_modpack_with_reporter(
                    crate::api::curseforge::CurseForgeModpackInstallRequest {
                        instance_id: instance_id.clone(),
                        project_id,
                        file_id,
                        install_optional: false,
                        allow_target_change: false,
                    },
                    Some(reporter.clone()),
                )
                .await?;
                if let Some(reason) = pack::curseforge_manual_download_pause(
                    &result,
                    &job_state.skipped_missing_content_paths,
                ) {
                    if let Some(minecraft_install) =
                        parallel_minecraft_install.take()
                    {
                        minecraft_install.abort().await;
                    }
                    return Ok(InstallExecutionOutcome::WaitingForUser(reason));
                }
            }
            if let Some(minecraft_install) = parallel_minecraft_install {
                minecraft_install.join().await?;
            } else {
                reporter
                    .update(
                        InstallPhaseId::DownloadingMinecraft,
                        None,
                        InstallPhaseDetails::Minecraft {
                            game_version: game_version.clone(),
                            loader,
                        },
                    )
                    .await?;
                let context =
                    crate::state::instances::commands::get_instance_launch_context(
                        &instance_id,
                        &state.pool,
                    )
                    .await?
                    .ok_or_else(|| {
                        crate::ErrorKind::InputError("Unknown instance".to_string())
                    })?;
                crate::launcher::install_minecraft_with_reporter(
                    &context,
                    false,
                    Some(reporter.clone()),
                    crate::launcher::InstanceCompletionPolicy::DeferToInstallJob,
                )
                .await?;
            }
            install_adjunct_components(
                state,
                &instance_id,
                &adjuncts,
                &game_version,
                loader,
                reporter.cancellation_token(),
            )
            .await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::CreateModpackInstance {
            location,
            post_install_edit,
        } => {
            let Some(instance_id) = current_instance_id(job_state) else {
                return Err(crate::ErrorKind::InputError(
                    "Install job is missing its instance id".to_string(),
                )
                .into());
            };
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::ResolvingPack,
                modpack_details(&location),
            )
            .await?;
            if let InstallExecutionOutcome::WaitingForUser(reason) =
                pack::install_pack(
                    job_id,
                    job_state,
                    location,
                    instance_id.clone(),
                    DownloadReason::Modpack,
                )
                .await?
            {
                return Ok(InstallExecutionOutcome::WaitingForUser(reason));
            }
            apply_post_install_edit(&instance_id, post_install_edit).await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::ImportInstance {
            launcher_type,
            base_path,
            instance_folder,
            instance_path,
            symlink,
            game_version,
            loader,
            loader_version,
            game_dir_override: _,
        } => {
            tracing::debug!(
                "InstallRequest::ImportInstance: launcher_type={launcher_type} base_path={} instance_folder={instance_folder} symlink={symlink}",
                base_path.display()
            );
            let Some(instance_id) = current_instance_id(job_state) else {
                return Err(crate::ErrorKind::InputError(
                    "Install job is missing its instance id".to_string(),
                )
                .into());
            };
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::PreparingInstance,
                InstallPhaseDetails::Import {
                    launcher_type,
                    instance_folder: instance_folder.clone(),
                },
            )
            .await?;
            crate::api::pack::import::import_instance_with_reporter(
                &instance_id,
                launcher_type,
                base_path,
                instance_folder,
                instance_path,
                crate::api::pack::import::ImportOverrides {
                    game_version,
                    loader,
                    loader_version,
                },
                // TODO(B2): apply overrides to launcher-specific importers
                // (MultiMC/Prism/ATLauncher/GDLauncher/Curseforge/ModrinthApp);
                // generic/PCL/HMCL/Axolotl paths already consume them.
                InstallProgressReporter::new(job_id, job_state.clone()),
                symlink,
            )
            .await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::DuplicateInstance { source_instance_id } => {
            let Some(instance_id) = current_instance_id(job_state) else {
                return Err(crate::ErrorKind::InputError(
                    "Install job is missing its instance id".to_string(),
                )
                .into());
            };
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::PreparingInstance,
                InstallPhaseDetails::Empty,
            )
            .await?;
            let state = State::get().await?;
            crate::api::pack::import::copy_dotminecraft_with_reporter(
                &instance_id,
                crate::api::instance::get_full_path(&source_instance_id)
                    .await?,
                &state.io_semaphore,
                InstallProgressReporter::new(job_id, job_state.clone()),
                InstallPhaseDetails::Empty,
            )
            .await?;
            let context =
                crate::state::instances::commands::get_instance_launch_context(
                    &instance_id,
                    &state.pool,
                )
                .await?
                .ok_or_else(|| {
                    crate::ErrorKind::InputError("Unknown instance".to_string())
                })?;
            crate::launcher::install_minecraft_with_reporter(
                &context,
                false,
                Some(InstallProgressReporter::new(job_id, job_state.clone())),
                crate::launcher::InstanceCompletionPolicy::DeferToInstallJob,
            )
            .await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::UpgradeUnmanagedInstance {
            instance_id: source_instance_id,
            plan_id,
            execution,
            create_full_backup,
            shared_upgrade_mode,
            display_names,
        } => {
            let target_instance_id = current_instance_id(job_state)
                .ok_or_else(|| {
                    crate::ErrorKind::InputError(
                        "Upgrade job is missing its target instance id"
                            .to_string(),
                    )
                })?;
            upgrade::run_instance_upgrade(
                job_id,
                job_state,
                state,
                &source_instance_id,
                &target_instance_id,
                &plan_id,
                execution,
                create_full_backup,
                shared_upgrade_mode,
                display_names,
            )
            .await?;
            Ok(InstallExecutionOutcome::Completed(Some(target_instance_id)))
        }
        InstallRequest::InstallExistingInstance { instance_id, force } => {
            prepare_existing_rollback(job_state, state, &instance_id).await?;
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::DownloadingMinecraft,
                InstallPhaseDetails::Empty,
            )
            .await?;
            let context =
                crate::state::instances::commands::get_instance_launch_context(
                    &instance_id,
                    &state.pool,
                )
                .await?
                .ok_or_else(|| {
                    crate::ErrorKind::InputError("Unknown instance".to_string())
                })?;
            crate::launcher::install_minecraft_with_reporter(
                &context,
                force,
                Some(InstallProgressReporter::new(job_id, job_state.clone())),
                crate::launcher::InstanceCompletionPolicy::DeferToInstallJob,
            )
            .await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::InstallPackToExistingInstance {
            instance_id,
            location,
            post_install_edit,
        } => {
            prepare_existing_rollback(job_state, state, &instance_id).await?;
            let disabled_project_ids = match job_state.continuation.clone() {
                Some(InstallContinuationState::InstallingPackToExistingInstance {
                    disabled_project_ids,
                }) => disabled_project_ids.into_iter().collect(),
                None => {
                    let disabled_project_ids = remove_existing_pack_content(
                        job_id,
                        job_state,
                        state,
                        &instance_id,
                    )
                    .await?;
                    let mut persisted_ids = disabled_project_ids
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    persisted_ids.sort_unstable();
                    let continuation = InstallContinuationState::InstallingPackToExistingInstance {
                        disabled_project_ids: persisted_ids,
                    };
                    job_state.continuation = Some(continuation.clone());
                    InstallProgressReporter::new(job_id, job_state.clone())
                        .set_continuation(Some(continuation))
                        .await?;
                    disabled_project_ids
                }
            };
            if let InstallExecutionOutcome::WaitingForUser(reason) =
                pack::install_pack(
                    job_id,
                    job_state,
                    location,
                    instance_id.clone(),
                    DownloadReason::Modpack,
                )
                .await?
            {
                return Ok(InstallExecutionOutcome::WaitingForUser(reason));
            }
            restore_disabled_projects(
                &instance_id,
                disabled_project_ids,
                state,
            )
            .await?;
            job_state.continuation = None;
            InstallProgressReporter::new(job_id, job_state.clone())
                .set_continuation(None)
                .await?;
            apply_post_install_edit(&instance_id, post_install_edit).await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::InstallContent {
            instance_id,
            project_id,
            version_id,
            content_type,
            selected,
            excluded_project_ids,
            display_title: _,
            display_icon: _,
        } => {
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::DownloadingContent,
                InstallPhaseDetails::Empty,
            )
            .await?;
            let plan = crate::state::instances::commands::resolve_install_plan(
                &instance_id,
                crate::state::instances::commands::InstanceInstallProjectRequest {
                    project_id: project_id.clone(),
                    version_id,
                    content_type,
                    selected,
                    excluded_project_ids,
                    force_project_ids: Vec::new(),
                },
                state,
            )
            .await?;
            let total = (plan.dependencies.len() + 1) as u64;
            let reporter =
                InstallProgressReporter::new(job_id, job_state.clone());
            reporter
                .update(
                    InstallPhaseId::DownloadingContent,
                    Some(InstallProgress {
                        current: 0,
                        total,
                        secondary: None,
                    }),
                    InstallPhaseDetails::Empty,
                )
                .await?;
            crate::state::instances::commands::install_resolved_content_plan_with_reporter(
                &instance_id,
                &plan,
                Some(reporter.clone()),
                state,
            )
            .await?;
            reporter
                .update(
                    InstallPhaseId::DownloadingContent,
                    Some(InstallProgress {
                        current: total,
                        total,
                        secondary: None,
                    }),
                    InstallPhaseDetails::Empty,
                )
                .await?;
            crate::api::instance::emit_content_changed(&instance_id).await?;
            let dependency_project_ids = plan
                .dependencies
                .iter()
                .map(|dependency| dependency.project_id.clone())
                .collect::<Vec<_>>();
            emit_instance(
                &instance_id,
                InstancePayloadType::ContentInstallFinished {
                    project_ids: std::iter::once(project_id.clone())
                        .chain(dependency_project_ids.iter().cloned())
                        .collect(),
                    dependency_project_ids,
                },
            )
            .await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::InstallCurseForgeContent {
            request,
            display_title: _,
            display_icon: _,
        } => {
            let instance_id = request.instance_id.clone();
            let primary_project_id = request.project_id;
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::DownloadingContent,
                InstallPhaseDetails::Empty,
            )
            .await?;
            let reporter =
                InstallProgressReporter::new(job_id, job_state.clone());
            let result = crate::api::curseforge::install_file_with_reporter(
                request, reporter,
            )
            .await?;
            crate::api::instance::emit_content_changed(&instance_id).await?;
            let dependency_project_ids = result
                .installed
                .iter()
                .filter(|installed| installed.dependency)
                .map(|installed| format!("curseforge:{}", installed.project_id))
                .collect::<Vec<_>>();
            emit_instance(
                &instance_id,
                InstancePayloadType::ContentInstallFinished {
                    project_ids: std::iter::once(format!(
                        "curseforge:{primary_project_id}"
                    ))
                    .chain(dependency_project_ids.iter().cloned())
                    .collect(),
                    dependency_project_ids,
                },
            )
            .await?;
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::InstallCurseForgeWorld {
            request,
            display_title: _,
            display_icon: _,
        } => {
            let instance_id = request.instance_id.clone();
            if pack::curseforge_world_was_imported_manually(job_state, &request) {
                return Ok(InstallExecutionOutcome::Completed(Some(
                    instance_id,
                )));
            }
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::DownloadingContent,
                InstallPhaseDetails::Empty,
            )
            .await?;
            let reporter =
                InstallProgressReporter::new(job_id, job_state.clone());
            let result = crate::api::curseforge::install_world_with_reporter(
                request.clone(),
                reporter.clone(),
            )
            .await?;
            if let Some(manual_download) = result.manual_download {
                let path = format!("saves/{}", manual_download.file_name);
                let manual_url = manual_download.website_url.clone().or_else(|| {
					Some(format!(
						"https://www.curseforge.com/minecraft/worlds/{}/download/{}",
						manual_download.project_slug, manual_download.file_id
					))
				});
                reporter
                    .record_events(vec![
                        InstallJobEventKind::ContentFileSkipped {
                            path: path.clone(),
                            reason: "CurseForge requires a manual download"
                                .to_string(),
                            project_id: Some(
                                manual_download.project_id.to_string(),
                            ),
                            version_id: Some(
                                manual_download.file_id.to_string(),
                            ),
                            manual_url,
                        },
                    ])
                    .await?;
                return Ok(InstallExecutionOutcome::WaitingForUser(
                    InstallPauseReason::MissingRequiredContent {
                        failed_files: 1,
                        paths: vec![path],
                    },
                ));
            }
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::UpdateManagedCurseForgeModpack {
            instance_id,
            file_id,
        } => {
            prepare_existing_rollback(job_state, state, &instance_id).await?;
            crate::state::instances::commands::set_instance_install_stage(
                &instance_id,
                InstanceInstallStage::PackInstalling,
                &state.pool,
            )
            .await?;
            emit_instance(&instance_id, InstancePayloadType::Edited).await?;
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::DownloadingContent,
                InstallPhaseDetails::Empty,
            )
            .await?;
            let reporter =
                InstallProgressReporter::new(job_id, job_state.clone());
            let result =
                crate::api::curseforge::update_managed_modpack_with_reporter(
                    &instance_id,
                    file_id,
                    Some(reporter.clone()),
                )
                .await?;
            if !result.content.failed_downloads.is_empty() {
                return Err(ErrorKind::NetworkError(format!(
                    "{} CurseForge files could not be downloaded automatically",
                    result.content.failed_downloads.len()
                ))
                .into());
            }
            if let Some(reason) = pack::curseforge_manual_download_pause(
                &result,
                &job_state.skipped_missing_content_paths,
            ) {
                return Ok(InstallExecutionOutcome::WaitingForUser(reason));
            }
            Ok(InstallExecutionOutcome::Completed(Some(instance_id)))
        }
        InstallRequest::DownloadJava { vendor, version } => {
            update_progress(
                job_id,
                job_state,
                state,
                InstallPhaseId::PreparingJava,
                InstallPhaseDetails::Java {
                    major_version: version,
                    step: InstallJavaStep::FetchingMetadata,
                },
            )
            .await?;
            let reporter =
                InstallProgressReporter::new(job_id, job_state.clone());
            let path = crate::api::jre::download_java_from_feed_with_reporter(
                &vendor, version, reporter,
            )
            .await?;
            let _ = path;
            Ok(InstallExecutionOutcome::Completed(None))
        }
    }
}

async fn apply_post_install_edit(
    instance_id: &str,
    edit: Option<InstallPostInstallEdit>,
) -> crate::Result<()> {
    let Some(edit) = edit else {
        return Ok(());
    };

    if edit.name.is_none() && edit.icon_path.is_none() && edit.link.is_none() {
        return Ok(());
    }

    crate::api::instance::edit(
        instance_id,
        crate::state::instances::commands::EditInstance {
            name: edit.name,
            icon_path: edit.icon_path,
            link: edit.link,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

async fn remove_existing_pack_content(
    job_id: Uuid,
    job_state: &mut InstallJobState,
    state: &State,
    instance_id: &str,
) -> crate::Result<HashSet<String>> {
    let metadata = crate::state::instances::commands::get_instance_metadata(
        instance_id,
        &state.pool,
    )
    .await?
    .ok_or_else(|| {
        crate::ErrorKind::InputError("Unknown instance".to_string())
    })?;
    let (project_id, version_id) = match &metadata.link {
        InstanceLink::ModrinthModpack {
            project_id,
            version_id,
        } => (project_id.clone(), version_id.clone()),
        InstanceLink::ServerProjectModpack {
            content_project_id,
            content_version_id,
            ..
        } => (content_project_id.clone(), content_version_id.clone()),
        InstanceLink::ImportedModpack { .. } => {
            recovery::prepare_existing_content_rollback(
                job_id,
                job_state,
                state,
                Vec::new(),
            )
            .await?;
            return Ok(HashSet::new());
        }
        _ => return Ok(HashSet::new()),
    };

    let disabled_project_ids =
        crate::state::instances::commands::list_project_files(
            instance_id,
            state,
        )
        .await?
        .into_iter()
        .filter_map(|file| {
            (!file.enabled).then(|| {
                file.provider_refs
                    .iter()
                    .find_map(|provider| match provider {
                        ContentProviderRef::Modrinth { project_id, .. } => {
                            Some(project_id.to_string())
                        }
                        ContentProviderRef::CurseForge { .. } => None,
                        ContentProviderRef::McArchive { .. } => None,
                    })
            })?
        })
        .collect::<HashSet<_>>();
    let reporter = InstallProgressReporter::new(job_id, job_state.clone());
    let old_pack = generate_pack_from_version_id_with_reporter(
        project_id.clone(),
        version_id.clone(),
        metadata.instance.name.clone(),
        None,
        instance_id.to_string(),
        DownloadReason::Update,
        reporter,
    )
    .await?;

    let related_paths = related_file_paths(&old_pack.file).await?;
    recovery::prepare_existing_content_rollback(
        job_id,
        job_state,
        state,
        related_paths,
    )
    .await?;

    Ok(disabled_project_ids)
}

async fn restore_disabled_projects(
    instance_id: &str,
    disabled_project_ids: HashSet<String>,
    state: &State,
) -> crate::Result<()> {
    if disabled_project_ids.is_empty() {
        return Ok(());
    }

    for file in crate::state::instances::commands::list_project_files(
        instance_id,
        state,
    )
    .await?
    {
        let is_disabled_modrinth_project = file.provider_refs.iter().any(
            |provider| {
                matches!(
                    provider,
                    ContentProviderRef::Modrinth { project_id, .. }
                        if disabled_project_ids.contains(&project_id.to_string())
                )
            },
        );
        if file.enabled && is_disabled_modrinth_project {
            crate::state::instances::commands::toggle_disable_project(
                instance_id,
                &file.relative_path,
                Some(false),
                state,
            )
            .await?;
        }
    }

    Ok(())
}

async fn prepare_existing_rollback(
    job_state: &mut InstallJobState,
    state: &State,
    instance_id: &str,
) -> crate::Result<()> {
    if job_state.rollback.is_some() {
        return Ok(());
    }

    let instance = crate::state::get_instance(instance_id, &state.pool)
        .await?
        .ok_or_else(|| {
            crate::ErrorKind::InputError(format!(
                "Unknown instance {instance_id}"
            ))
        })?;
    let install_stage = instance.instance.install_stage;
    set_display(
        job_state,
        instance.instance.name.clone(),
        instance.instance.icon_path.clone(),
    );
    job_state.rollback = Some(InstallRollbackState {
        instance,
        install_stage,
        content: None,
    });
    job_state.cleanup = InstallCleanup::RestoreExistingInstance {
        instance_id: instance_id.to_string(),
    };

    crate::state::instances::commands::set_instance_install_stage(
        instance_id,
        InstanceInstallStage::MinecraftInstalling,
        &state.pool,
    )
    .await?;
    emit_instance(instance_id, InstancePayloadType::Edited).await?;

    Ok(())
}

async fn update_progress(
    job_id: Uuid,
    job_state: &mut InstallJobState,
    state: &State,
    phase: InstallPhaseId,
    details: InstallPhaseDetails,
) -> crate::Result<()> {
    job_state.set_progress(phase, None, details);
    let record = store::update_state(job_id, job_state, state).await?;
    emit_install_job(&record.snapshot()).await?;
    Ok(())
}

fn set_instance_id(job_state: &mut InstallJobState, instance_id: String) {
    job_state.target = match &job_state.target {
        InstallTarget::ExistingInstance { .. } => {
            InstallTarget::ExistingInstance {
                instance_id: instance_id.clone(),
            }
        }
        InstallTarget::NewInstance { .. } => InstallTarget::NewInstance {
            instance_id: Some(instance_id.clone()),
        },
    };
    job_state.cleanup = match &job_state.cleanup {
        InstallCleanup::RestoreExistingInstance { .. } => {
            InstallCleanup::RestoreExistingInstance { instance_id }
        }
        InstallCleanup::DeleteNewInstance { .. } => {
            InstallCleanup::DeleteNewInstance {
                instance_id: Some(instance_id),
            }
        }
        InstallCleanup::None => InstallCleanup::None,
    };
}

fn clear_deleted_new_instance_id(job_state: &mut InstallJobState) {
    if matches!(job_state.cleanup, InstallCleanup::DeleteNewInstance { .. }) {
        job_state.target = InstallTarget::NewInstance { instance_id: None };
        job_state.cleanup =
            InstallCleanup::DeleteNewInstance { instance_id: None };
    }
}

fn set_display(
    job_state: &mut InstallJobState,
    title: String,
    icon: Option<String>,
) {
    job_state.display = Some(InstallJobDisplay { title, icon });
}

fn install_error_view(
    phase: InstallPhaseId,
    error: &crate::Error,
    context: Option<InstallErrorContext>,
) -> InstallErrorView {
    let context = match error.raw.as_ref() {
        ErrorKind::CacheReadError {
            cache_type,
            sqlite_code,
            ..
        } => {
            let mut context = context.unwrap_or_else(|| {
                InstallErrorContext::new("read project metadata cache").build()
            });
            context.cache_types = vec![cache_type.clone()];
            context.sqlite_code = sqlite_code.clone();
            Some(context)
        }
        _ => context,
    };
    InstallErrorView::from_error(
        install_error_code(phase, error),
        phase,
        error,
        context,
    )
}

fn install_error_code(
    phase: InstallPhaseId,
    error: &crate::Error,
) -> &'static str {
    use InstallPhaseId::*;

    match error.raw.as_ref() {
        ErrorKind::CacheReadError { .. } => "cache_repair_required",
        ErrorKind::InputError(msg)
            if msg.starts_with("Unrecognized modpack format")
                && matches!(phase, ResolvingPack) =>
        {
            "unrecognized_format"
        }
        ErrorKind::InputError(_) => match phase {
            PreparingInstance | CreatingBackup | Finalizing | Completed => {
                "instance_error"
            }
            ResolvingPack | DownloadingPackFile | ReadingPackManifest => {
                "pack_error"
            }
            DownloadingContent | StagingContent | ApplyingContent => {
                "content_error"
            }
            ExtractingOverrides => "path_error",
            PreparingJava => "java_error",
            DownloadingMinecraft => "instance_error",
            RollingBack => "rollback_error",
            ResolvingMinecraft
            | ResolvingLoader
            | RunningLoaderProcessors
            | UpdatingLoader
            | Verifying => "launcher_error",
        },
        ErrorKind::LauncherError(_) => match phase {
            RunningLoaderProcessors => "processor_error",
            PreparingJava => "java_error",
            ResolvingLoader => "loader_error",
            _ => "launcher_error",
        },
        ErrorKind::JREError(_) => "java_error",
        ErrorKind::NoValueFor(_) | ErrorKind::MetadataError(_) => match phase {
            ResolvingLoader => "loader_error",
            PreparingJava => "java_error",
            _ => "metadata_error",
        },
        ErrorKind::FetchError(_)
        | ErrorKind::NetworkError(_)
        | ErrorKind::HttpError { .. }
        | ErrorKind::ApiIsDownError(_) => "network_error",
        ErrorKind::Any(_)
            if matches!(
                phase,
                DownloadingPackFile
                    | DownloadingContent
                    | ResolvingMinecraft
                    | ResolvingLoader
                    | PreparingJava
                    | DownloadingMinecraft
            ) =>
        {
            "network_error"
        }
        ErrorKind::LabrinthError(_) => "api_error",
        ErrorKind::HashError(_, _) => "hash_error",
        ErrorKind::ZipError(_) => "archive_error",
        ErrorKind::DeserializationError(_) | ErrorKind::StripPrefixError(_) => {
            "path_error"
        }
        ErrorKind::FSError(_)
        | ErrorKind::IOError(_)
        | ErrorKind::StdIOError(_)
        | ErrorKind::UTFError(_) => "filesystem_error",
        ErrorKind::INIError(_) | ErrorKind::JSONError(_) => "parse_error",
        ErrorKind::Sqlx(_) | ErrorKind::SqlxMigrate(_) => "database_error",
        ErrorKind::JoinError(_)
        | ErrorKind::RecvError(_)
        | ErrorKind::AcquireError(_)
        | ErrorKind::EventError(_) => "internal_error",
        ErrorKind::OtherError(_) | ErrorKind::Any(_) => "internal_error",
        _ => "unknown_error",
    }
}

fn current_instance_id(job_state: &InstallJobState) -> Option<String> {
    match &job_state.target {
        InstallTarget::NewInstance { instance_id } => instance_id.clone(),
        InstallTarget::ExistingInstance { instance_id } => {
            Some(instance_id.clone())
        }
    }
}

pub(crate) const OPTIFABRIC_CURSEFORGE_PROJECT_ID: u32 = 322_385;

async fn resolve_required_adjuncts(
    game_version: &str,
    loader: ModLoader,
    adjuncts: &mut Vec<LoaderComponent>,
    _state: &State,
) -> crate::Result<()> {
    for adjunct in adjuncts.iter() {
        match adjunct.kind {
            LoaderComponentKind::OptiFine
                if !matches!(
                    loader,
                    ModLoader::Forge
                        | ModLoader::NeoForge
                        | ModLoader::Fabric
                        | ModLoader::LegacyFabric
                ) =>
            {
                return Err(ErrorKind::InputError(format!(
                    "OptiFine is not supported with {}",
                    loader.as_str()
                ))
                .into());
            }
            LoaderComponentKind::LiteLoader if loader != ModLoader::Forge => {
                return Err(ErrorKind::InputError(format!(
                    "LiteLoader is not supported with {}",
                    loader.as_str()
                ))
                .into());
            }
            _ => {}
        }
    }
    for adjunct in adjuncts.iter_mut() {
        adjunct.role = LoaderComponentRole::Adjunct;
        adjunct.instance_id.clear();
        match adjunct.kind {
            LoaderComponentKind::OptiFine => {
                let resolved =
					crate::launcher::optifine::resolve_loader_version(
						game_version,
						adjunct.version.as_deref(),
					)
					.await?
					.ok_or_else(|| {
						ErrorKind::InputError(format!(
							"No OptiFine version supports Minecraft {game_version}"
						))
					})?;
                adjunct.version = Some(resolved.id);
            }
            LoaderComponentKind::LiteLoader => {
                let resolved =
					crate::launcher::get_loader_version_from_profile(
						game_version,
						ModLoader::LiteLoader,
						adjunct.version.as_deref(),
					)
					.await?
					.ok_or_else(|| {
						ErrorKind::InputError(format!(
							"No LiteLoader version supports Minecraft {game_version}"
						))
					})?;
                adjunct.version = Some(resolved.id);
            }
            _ => {}
        }
    }
    if adjuncts
        .iter()
        .any(|component| component.kind == LoaderComponentKind::OptiFine)
        && matches!(loader, ModLoader::Fabric | ModLoader::LegacyFabric)
        && !adjuncts
            .iter()
            .any(|component| component.kind == LoaderComponentKind::OptiFabric)
    {
        let version_id = resolve_optifabric_version(game_version).await?;
        adjuncts.push(LoaderComponent {
            instance_id: String::new(),
            kind: LoaderComponentKind::OptiFabric,
            version: Some(version_id),
            role: LoaderComponentRole::Adjunct,
            provider_metadata: Some(serde_json::json!({
                "projectId": OPTIFABRIC_CURSEFORGE_PROJECT_ID,
                "provider": "curseforge"
            })),
        });
    }
    let mut components =
        vec![LoaderComponent::new_primary(String::new(), loader, None)];
    components.extend(adjuncts.iter().cloned());
    validate_loader_components(&components)
}

pub(crate) fn validate_loader_components(
    components: &[LoaderComponent],
) -> crate::Result<()> {
    let primary = components
        .iter()
        .find(|component| component.role == LoaderComponentRole::Primary)
        .ok_or_else(|| {
            ErrorKind::InputError(
                "Loader selection has no primary loader".to_string(),
            )
        })?;
    let has = |kind| {
        components.iter().any(|component| {
            component.role == LoaderComponentRole::Adjunct
                && component.kind == kind
        })
    };
    if has(LoaderComponentKind::OptiFine) {
        match primary.kind {
            LoaderComponentKind::Vanilla => {}
            LoaderComponentKind::Forge | LoaderComponentKind::NeoForge => {}
            LoaderComponentKind::Fabric | LoaderComponentKind::LegacyFabric
                if has(LoaderComponentKind::OptiFabric) => {}
            _ => {
                return Err(ErrorKind::InputError(format!(
                    "OptiFine is not supported with {}",
                    primary.kind.as_str()
                ))
                .into());
            }
        }
    }
    if has(LoaderComponentKind::OptiFabric)
        && !has(LoaderComponentKind::OptiFine)
    {
        return Err(ErrorKind::InputError(
            "OptiFabric can only be installed with OptiFine".to_string(),
        )
        .into());
    }
    if has(LoaderComponentKind::OptiFabric)
        && !matches!(
            primary.kind,
            LoaderComponentKind::Fabric | LoaderComponentKind::LegacyFabric
        )
    {
        return Err(ErrorKind::InputError(format!(
            "OptiFabric is not supported with {}",
            primary.kind.as_str()
        ))
        .into());
    }
    if has(LoaderComponentKind::LiteLoader)
        && !matches!(
            primary.kind,
            LoaderComponentKind::Vanilla | LoaderComponentKind::Forge
        )
    {
        return Err(ErrorKind::InputError(format!(
            "LiteLoader is not supported with {}",
            primary.kind.as_str()
        ))
        .into());
    }
    if components.iter().any(|component| {
        component.role == LoaderComponentRole::Adjunct
            && !matches!(
                component.kind,
                LoaderComponentKind::OptiFine
                    | LoaderComponentKind::LiteLoader
                    | LoaderComponentKind::OptiFabric
            )
    }) {
        return Err(ErrorKind::InputError(
            "Only OptiFine, LiteLoader, and OptiFabric can be adjunct loaders"
                .to_string(),
        )
        .into());
    }
    Ok(())
}

pub(crate) async fn resolve_optifabric_version(
    game_version: &str,
) -> crate::Result<String> {
    let files = crate::api::curseforge::get_files(
        OPTIFABRIC_CURSEFORGE_PROJECT_ID,
        crate::api::curseforge::CurseForgeFilesRequest {
            game_version: None,
            mod_loader_type: None,
            game_version_type_id: None,
            index: 0,
            page_size: 50,
        },
    )
    .await?
    .files;
    select_optifabric_file_id(&files, game_version)
        .map(|file_id| file_id.to_string())
        .ok_or_else(|| {
            ErrorKind::InputError(format!(
                "OptiFine requires OptiFabric, but no OptiFabric version supports Minecraft {game_version}"
            ))
            .into()
        })
}

fn select_optifabric_file_id(
    files: &[crate::api::curseforge::CurseForgeFile],
    game_version: &str,
) -> Option<u32> {
    files
        .iter()
        .find(|file| {
            file.is_available
                && file
                    .game_versions
                    .iter()
                    .any(|version| version == game_version)
        })
        .map(|file| file.id)
}

pub(crate) async fn install_optifabric_file(
    instance_id: &str,
    game_version: &str,
    version: &str,
) -> crate::Result<String> {
    let file_id = version.parse::<u32>().map_err(|_| {
        ErrorKind::InputError(
            "OptiFabric CurseForge file ID is invalid".to_string(),
        )
    })?;
    let file = crate::api::curseforge::get_file(
        OPTIFABRIC_CURSEFORGE_PROJECT_ID,
        file_id,
    )
    .await?;
    if file.mod_id != OPTIFABRIC_CURSEFORGE_PROJECT_ID
        || !file.is_available
        || !file
            .game_versions
            .iter()
            .any(|version| version == game_version)
    {
        return Err(ErrorKind::InputError(format!(
            "OptiFabric file {file_id} does not support Minecraft {game_version}"
        ))
        .into());
    }

    let result = crate::api::curseforge::install_file(
        crate::api::curseforge::CurseForgeInstallRequest {
            instance_id: instance_id.to_string(),
            project_id: OPTIFABRIC_CURSEFORGE_PROJECT_ID,
            file_id,
            project_type: "mod".to_string(),
            ownership_kind: crate::state::instances::ContentOwnershipKind::UserAdded,
            manual_operation_kind:
                crate::state::instances::ManualDownloadOperationKind::ContentInstall,
            game_version: Some(game_version.to_string()),
            mod_loader_type: Some(4),
            world_name: None,
            install_dependencies: false,
            excluded_dependency_project_ids: Vec::new(),
            force_dependency_project_ids: Vec::new(),
            dependency_plan_id: None,
        },
    )
    .await?;
    if !result.manual_downloads.is_empty() {
        return Err(ErrorKind::InputError(
            "OptiFabric requires a manual CurseForge download".to_string(),
        )
        .into());
    }
    if let Some(failure) = result.failed_downloads.first() {
        return Err(ErrorKind::InputError(format!(
            "Failed to install OptiFabric: {}",
            failure.reason
        ))
        .into());
    }
    if !result.installed.iter().any(|installed| {
        !installed.dependency
            && installed.project_id == OPTIFABRIC_CURSEFORGE_PROJECT_ID
            && installed.file_id == file_id
    }) {
        return Err(ErrorKind::InputError(
            "OptiFabric was not installed".to_string(),
        )
        .into());
    }
    Ok(file_id.to_string())
}

async fn install_adjunct_components(
    state: &State,
    instance_id: &str,
    adjuncts: &[LoaderComponent],
    game_version: &str,
    loader: ModLoader,
    cancellation: tokio_util::sync::CancellationToken,
) -> crate::Result<()> {
    if adjuncts.is_empty() {
        return Ok(());
    }
    let metadata = crate::api::instance::get(instance_id)
        .await?
        .ok_or_else(|| ErrorKind::InputError("Unknown instance".to_string()))?;
    let instance_path = state.directories.instance_game_dir(&metadata.instance);
    let mut components = metadata.loader_components.clone();

    for adjunct in adjuncts {
        match adjunct.kind {
            LoaderComponentKind::OptiFine => {
                let version = crate::launcher::optifine::resolve_loader_version(
					game_version,
					adjunct.version.as_deref(),
				)
				.await?
				.ok_or_else(|| {
					ErrorKind::InputError(format!(
						"No OptiFine version supports Minecraft {game_version}"
					))
				})?;
                crate::api::pack::install_mcbbs::install_optifine_mod(
                    state,
                    instance_id,
                    cancellation.clone(),
                    game_version,
                    &version.id,
                    &instance_path,
                )
                .await?;
                set_component_version(
                    &mut components,
                    LoaderComponentKind::OptiFine,
                    version.id,
                );
            }
            LoaderComponentKind::OptiFabric => {
                let version_id = match &adjunct.version {
                    Some(version) => version.clone(),
                    None => resolve_optifabric_version(game_version).await?,
                };
                let version_id = install_optifabric_file(
                    instance_id,
                    game_version,
                    &version_id,
                )
                .await?;
                set_component_version(
                    &mut components,
                    LoaderComponentKind::OptiFabric,
                    version_id,
                );
            }
            LoaderComponentKind::LiteLoader => {
                let version = install_liteloader_adjunct(
                    state,
                    &metadata,
                    game_version,
                    loader,
                    adjunct.version.as_deref(),
                )
                .await?;
                set_component_version(
                    &mut components,
                    LoaderComponentKind::LiteLoader,
                    version,
                );
            }
            _ => {}
        }
    }
    crate::state::instances::commands::replace_instance_loader_components(
        instance_id,
        &components,
        &state.pool,
    )
    .await
}

fn set_component_version(
    components: &mut [LoaderComponent],
    kind: LoaderComponentKind,
    version: String,
) {
    if let Some(component) = components
        .iter_mut()
        .find(|component| component.kind == kind)
    {
        component.version = Some(version);
    }
}

pub(crate) async fn install_liteloader_adjunct(
    state: &State,
    metadata: &crate::state::InstanceMetadata,
    game_version: &str,
    primary_loader: ModLoader,
    requested_version: Option<&str>,
) -> crate::Result<String> {
    let version = crate::launcher::get_loader_version_from_profile(
        game_version,
        ModLoader::LiteLoader,
        requested_version,
    )
    .await?
    .ok_or_else(|| {
        ErrorKind::InputError(format!(
            "No LiteLoader version supports Minecraft {game_version}"
        ))
    })?;
    install_liteloader_adjunct_resolved(
        state,
        metadata,
        game_version,
        primary_loader,
        &version,
    )
    .await
}

pub(crate) async fn install_liteloader_adjunct_resolved(
    state: &State,
    metadata: &crate::state::InstanceMetadata,
    game_version: &str,
    primary_loader: ModLoader,
    version: &daedalus::modded::LoaderVersion,
) -> crate::Result<String> {
    let partial = crate::api::loader_metadata::resolve_loader_profile(
        state,
        game_version,
        version,
    )
    .await?;
    let primary_version = metadata
        .applied_content_set
        .loader_version
        .as_deref()
        .ok_or_else(|| {
            ErrorKind::InputError(format!(
                "{} adjunct installation requires a pinned primary version",
                primary_loader.as_str()
            ))
        })?;
    let version_id = format!("{game_version}-{primary_version}");
    let path = state
        .directories
        .version_dir(&version_id)
        .join(format!("{version_id}.json"));
    let bytes = crate::util::io::read(&path).await?;
    let primary: daedalus::minecraft::VersionInfo =
        serde_json::from_slice(&bytes)?;
    let mut merged = daedalus::modded::merge_partial_version(partial, primary);
    merged.id.clone_from(&version_id);
    crate::launcher::download::download_libraries(
        state,
        None,
        &merged.libraries,
        &version_id,
        None,
        0.0,
        std::env::consts::ARCH,
        false,
        false,
        None,
    )
    .await?;
    crate::util::io::write(&path, serde_json::to_vec(&merged)?).await?;
    Ok(version.id.clone())
}

pub(super) fn modpack_details(location: &CreatePackLocation) -> InstallPhaseDetails {
    match location {
        CreatePackLocation::FromVersionId {
            project_id,
            version_id,
            title,
            ..
        } => InstallPhaseDetails::Modpack {
            project_id: Some(project_id.clone()),
            version_id: Some(version_id.clone()),
            title: Some(title.clone()),
        },
        CreatePackLocation::FromFile { .. } => InstallPhaseDetails::Modpack {
            project_id: None,
            version_id: None,
            title: None,
        },
    }
}

#[cfg(test)]
mod tests;
