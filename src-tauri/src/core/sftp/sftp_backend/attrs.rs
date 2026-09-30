//! Internal pieces of the SFTP backend moved out of `sftp_backend.rs`.

use super::*;

impl SftpBackend {
    pub(super) async fn resolve_uid_names(&self, uids: HashSet<u32>) -> HashMap<u32, String> {
        if !self.shell_available {
            return HashMap::new();
        }
        let missing: Vec<u32> = {
            let cache = self.identity_cache.read().await;
            uids.iter()
                .copied()
                .filter(|uid| !cache.users_by_uid.contains_key(uid))
                .collect()
        };
        if missing.is_empty() {
            let cache = self.identity_cache.read().await;
            return uids
                .into_iter()
                .filter_map(|uid| cache.users_by_uid.get(&uid).map(|name| (uid, name.clone())))
                .collect();
        }

        let id_list = missing
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!(
            "ids={}; for id in $ids; do name=$(getent passwd \"$id\" 2>/dev/null | cut -d: -f1); if [ -z \"$name\" ] && [ -r /etc/passwd ]; then name=$(awk -F: -v id=\"$id\" '$3==id {{print $1; exit}}' /etc/passwd 2>/dev/null); fi; if [ -n \"$name\" ]; then printf '%s:%s\\n' \"$id\" \"$name\"; fi; done",
            sh_quote(&id_list)
        );

        let mut resolved = HashMap::new();
        if let Ok(output) = self.exec_ok(&command).await {
            for line in String::from_utf8_lossy(&output).lines() {
                if let Some((id, name)) = line.split_once(':') {
                    if let Ok(uid) = id.parse::<u32>() {
                        let trimmed = name.trim();
                        if !trimmed.is_empty() {
                            resolved.insert(uid, trimmed.to_string());
                        }
                    }
                }
            }
        }

        let mut cache = self.identity_cache.write().await;
        for (uid, name) in &resolved {
            cache.users_by_uid.insert(*uid, name.clone());
            cache.uids_by_user.insert(name.clone(), *uid);
        }
        uids.into_iter()
            .filter_map(|uid| cache.users_by_uid.get(&uid).map(|name| (uid, name.clone())))
            .collect()
    }

    pub(super) async fn resolve_gid_names(&self, gids: HashSet<u32>) -> HashMap<u32, String> {
        if !self.shell_available {
            return HashMap::new();
        }
        let missing: Vec<u32> = {
            let cache = self.identity_cache.read().await;
            gids.iter()
                .copied()
                .filter(|gid| !cache.groups_by_gid.contains_key(gid))
                .collect()
        };
        if missing.is_empty() {
            let cache = self.identity_cache.read().await;
            return gids
                .into_iter()
                .filter_map(|gid| {
                    cache
                        .groups_by_gid
                        .get(&gid)
                        .map(|name| (gid, name.clone()))
                })
                .collect();
        }

        let id_list = missing
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!(
            "ids={}; for id in $ids; do name=$(getent group \"$id\" 2>/dev/null | cut -d: -f1); if [ -z \"$name\" ] && [ -r /etc/group ]; then name=$(awk -F: -v id=\"$id\" '$3==id {{print $1; exit}}' /etc/group 2>/dev/null); fi; if [ -n \"$name\" ]; then printf '%s:%s\\n' \"$id\" \"$name\"; fi; done",
            sh_quote(&id_list)
        );

        let mut resolved = HashMap::new();
        if let Ok(output) = self.exec_ok(&command).await {
            for line in String::from_utf8_lossy(&output).lines() {
                if let Some((id, name)) = line.split_once(':') {
                    if let Ok(gid) = id.parse::<u32>() {
                        let trimmed = name.trim();
                        if !trimmed.is_empty() {
                            resolved.insert(gid, trimmed.to_string());
                        }
                    }
                }
            }
        }

        let mut cache = self.identity_cache.write().await;
        for (gid, name) in &resolved {
            cache.groups_by_gid.insert(*gid, name.clone());
            cache.gids_by_group.insert(name.clone(), *gid);
        }
        gids.into_iter()
            .filter_map(|gid| {
                cache
                    .groups_by_gid
                    .get(&gid)
                    .map(|name| (gid, name.clone()))
            })
            .collect()
    }

    pub(super) async fn resolve_user_to_uid(&self, owner: &str) -> AppResult<u32> {
        if let Ok(uid) = owner.parse::<u32>() {
            return Ok(uid);
        }
        if let Some(uid) = self
            .identity_cache
            .read()
            .await
            .uids_by_user
            .get(owner)
            .copied()
        {
            return Ok(uid);
        }
        let command = format!(
            "name={}; id=$(getent passwd \"$name\" 2>/dev/null | cut -d: -f3); if [ -z \"$id\" ] && [ -r /etc/passwd ]; then id=$(awk -F: -v name=\"$name\" '$1==name {{print $3; exit}}' /etc/passwd 2>/dev/null); fi; [ -n \"$id\" ] && printf '%s\\n' \"$id\"",
            sh_quote(owner)
        );
        let output = self.exec_ok(&command).await?;
        let text = String::from_utf8_lossy(&output);
        let uid = text
            .trim()
            .parse::<u32>()
            .map_err(|_| AppError::Channel(format!("Failed to resolve remote user '{}'", owner)))?;
        let mut cache = self.identity_cache.write().await;
        cache.uids_by_user.insert(owner.to_string(), uid);
        cache.users_by_uid.insert(uid, owner.to_string());
        Ok(uid)
    }

    pub(super) async fn resolve_group_to_gid(&self, group: &str) -> AppResult<u32> {
        if let Ok(gid) = group.parse::<u32>() {
            return Ok(gid);
        }
        if let Some(gid) = self
            .identity_cache
            .read()
            .await
            .gids_by_group
            .get(group)
            .copied()
        {
            return Ok(gid);
        }
        let command = format!(
            "name={}; id=$(getent group \"$name\" 2>/dev/null | cut -d: -f3); if [ -z \"$id\" ] && [ -r /etc/group ]; then id=$(awk -F: -v name=\"$name\" '$1==name {{print $3; exit}}' /etc/group 2>/dev/null); fi; [ -n \"$id\" ] && printf '%s\\n' \"$id\"",
            sh_quote(group)
        );
        let output = self.exec_ok(&command).await?;
        let text = String::from_utf8_lossy(&output);
        let gid = text.trim().parse::<u32>().map_err(|_| {
            AppError::Channel(format!("Failed to resolve remote group '{}'", group))
        })?;
        let mut cache = self.identity_cache.write().await;
        cache.gids_by_group.insert(group.to_string(), gid);
        cache.groups_by_gid.insert(gid, group.to_string());
        Ok(gid)
    }
}

pub(super) fn sftp_attrs_is_dir(attrs: &FileAttributes) -> bool {
    attrs.permissions.map_or(false, |permissions| {
        (permissions & SFTP_FILE_TYPE_MASK) == 0o040000
    })
}

pub(super) fn sftp_attrs_is_symlink(attrs: &FileAttributes) -> bool {
    attrs.permissions.map_or(false, |permissions| {
        (permissions & SFTP_FILE_TYPE_MASK) == 0o120000
    })
}

pub(super) fn permissions_to_preserve_after_write(permissions: Option<u32>) -> Option<u32> {
    permissions.map(|permissions| permissions & POSIX_MODE_MASK)
}

pub(super) fn normalize_remote_dir_path(path: &str) -> &str {
    if path == "/" {
        "/"
    } else {
        path.trim_end_matches('/')
    }
}
pub(super) async fn apply_remote_mode_bytes(
    sftp: &SftpSession,
    display_path: &str,
    path_bytes: Vec<u8>,
    requested_mode: u32,
) -> AppResult<()> {
    let original_attrs = sftp.metadata_bytes(path_bytes.clone()).await?;
    let original_permissions = original_attrs.permissions;
    let requested_permissions = requested_mode & POSIX_MODE_MASK;

    let mut attrs = FileAttributes::empty();
    attrs.permissions = Some(requested_permissions);
    sftp.set_metadata_bytes(path_bytes.clone(), attrs)
        .await
        .map_err(|error| {
            tracing::warn!(
                remote_path = display_path,
                original_permissions = %describe_permissions(original_permissions),
                requested_permissions = format!("{requested_permissions:#06o}"),
                error = %error,
                "Failed to update remote permissions with a permissions-only SETSTAT payload"
            );
            AppError::from(error)
        })?;

    let actual_permissions = sftp
        .metadata_bytes(path_bytes)
        .await
        .ok()
        .and_then(|attrs| attrs.permissions);
    tracing::debug!(
        target: "user_action",
        action = "chmod",
        remote_path = display_path,
        original_permissions = %describe_permissions(original_permissions),
        requested_permissions = format!("{requested_permissions:#06o}"),
        actual_permissions = %describe_permissions(actual_permissions),
        "Applied remote permissions"
    );

    Ok(())
}

pub(super) async fn apply_remote_attrs_bytes(
    sftp: &SftpSession,
    display_path: &str,
    path_bytes: Vec<u8>,
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
) -> AppResult<()> {
    let original_attrs = sftp.symlink_metadata_bytes(path_bytes.clone()).await?;
    let mut attrs = FileAttributes::empty();
    if let Some(mode) = mode {
        let type_bits = original_attrs.permissions.unwrap_or(0) & SFTP_FILE_TYPE_MASK;
        attrs.permissions = Some(type_bits | (mode & POSIX_MODE_MASK));
    }
    if uid.is_some() || gid.is_some() {
        let effective_uid = uid.or(original_attrs.uid);
        let effective_gid = gid.or(original_attrs.gid);
        match (effective_uid, effective_gid) {
            (Some(effective_uid), Some(effective_gid)) => {
                attrs.uid = Some(effective_uid);
                attrs.gid = Some(effective_gid);
            }
            _ => {
                return Err(AppError::Channel(
                    "Cannot update SFTP ownership because the server did not provide the current UID/GID; set both owner and group."
                        .to_string(),
                ));
            }
        }
    }
    if attrs.permissions.is_none() && attrs.uid.is_none() && attrs.gid.is_none() {
        return Ok(());
    }

    sftp.set_metadata_bytes(path_bytes, attrs)
        .await
        .map_err(|error| {
            tracing::warn!(
                remote_path = display_path,
                requested_mode = ?mode,
                requested_uid = ?uid,
                requested_gid = ?gid,
                error = %error,
                "Failed to update remote file attributes"
            );
            AppError::from(error)
        })?;

    Ok(())
}

pub(super) async fn apply_remote_attrs_recursive_bytes(
    sftp: &SftpSession,
    display_path: &str,
    path_bytes: Vec<u8>,
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
) -> AppResult<()> {
    let path_bytes = normalize_remote_dir_path_bytes(&path_bytes);
    let meta = sftp.symlink_metadata_bytes(path_bytes.clone()).await?;
    let is_dir = sftp_attrs_is_dir(&meta);
    let is_symlink = sftp_attrs_is_symlink(&meta);

    apply_remote_attrs_bytes(sftp, display_path, path_bytes.clone(), mode, uid, gid).await?;

    if !is_dir || is_symlink {
        return Ok(());
    }

    let dir = sftp.read_dir_bytes(path_bytes.clone()).await?;
    let mut errors: Vec<String> = Vec::new();
    for entry in dir {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let child_bytes = join_remote_child_bytes(&path_bytes, entry.file_name_bytes());
        let child_display = join_remote_child(display_path, &name);
        let attrs = entry.metadata();
        if sftp_attrs_is_dir(&attrs) && !sftp_attrs_is_symlink(&attrs) {
            if let Err(error) = Box::pin(apply_remote_attrs_recursive_bytes(
                sftp,
                &child_display,
                child_bytes,
                mode,
                uid,
                gid,
            ))
            .await
            {
                errors.push(error.to_string());
            }
        } else if let Err(error) =
            apply_remote_attrs_bytes(sftp, &child_display, child_bytes, mode, uid, gid).await
        {
            errors.push(format!("'{}': {}", child_display, error));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::Channel(format!(
            "{} item(s) could not be updated:\n{}",
            errors.len(),
            errors.join("\n")
        )))
    }
}

pub(super) async fn apply_remote_mode_after_create_bytes(
    sftp: &SftpSession,
    display_path: &str,
    path_bytes: Vec<u8>,
    mode: &str,
    item_kind: &str,
) -> AppResult<()> {
    let requested_mode = parse_octal_mode(mode)?;

    match apply_remote_mode_bytes(sftp, display_path, path_bytes.clone(), requested_mode).await {
        Ok(()) => Ok(()),
        Err(error) => {
            if sftp.metadata_bytes(path_bytes).await.is_ok() {
                tracing::warn!(
                    remote_path = display_path,
                    requested_mode = mode,
                    item_kind = %item_kind,
                    error = %error,
                    "Remote item created, but failed to apply requested permissions"
                );
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}
