use axum::extract::State;
use conduwuit::{err, warn, Err};
use ruma::{
	UInt,
	api::client::backup::{
		add_backup_keys, add_backup_keys_for_room, add_backup_keys_for_session,
		create_backup_version, delete_backup_keys, delete_backup_keys_for_room,
		delete_backup_keys_for_session, delete_backup_version, get_backup_info, get_backup_keys,
		get_backup_keys_for_room, get_backup_keys_for_session, get_latest_backup_info,
		update_backup_version,
	},
};

use crate::{Result, Ruma};

/// # `POST /_matrix/client/r0/room_keys/version`
///
/// Creates a new backup.
pub(crate) async fn create_backup_version_route(
	State(services): State<crate::State>,
	body: Ruma<create_backup_version::v3::Request>,
) -> Result<create_backup_version::v3::Response> {
	let version = services
		.key_backups
		.create_backup(body.sender_user(), &body.algorithm)?;

	Ok(create_backup_version::v3::Response { version })
}

/// # `PUT /_matrix/client/r0/room_keys/version/{version}`
///
/// Update information about an existing backup. Only `auth_data` can be
/// modified.
pub(crate) async fn update_backup_version_route(
	State(services): State<crate::State>,
	body: Ruma<update_backup_version::v3::Request>,
) -> Result<update_backup_version::v3::Response> {
	services
		.key_backups
		.update_backup(body.sender_user(), &body.version, &body.algorithm)
		.await?;

	Ok(update_backup_version::v3::Response {})
}

/// # `GET /_matrix/client/r0/room_keys/version`
///
/// Get information about the latest backup version.
pub(crate) async fn get_latest_backup_info_route(
	State(services): State<crate::State>,
	body: Ruma<get_latest_backup_info::v3::Request>,
) -> Result<get_latest_backup_info::v3::Response> {
	let (version, algorithm) = services
		.key_backups
		.get_latest_backup(body.sender_user())
		.await
		.map_err(|_| err!(Request(NotFound("Key backup does not exist."))))?;

	Ok(get_latest_backup_info::v3::Response {
		algorithm,
		count: (UInt::try_from(
			services
				.key_backups
				.count_keys(body.sender_user(), &version)
				.await,
		)
		.expect("user backup keys count should not be that high")),
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &version)
			.await,
		version,
	})
}

/// # `GET /_matrix/client/v3/room_keys/version/{version}`
///
/// Get information about an existing backup.
pub(crate) async fn get_backup_info_route(
	State(services): State<crate::State>,
	body: Ruma<get_backup_info::v3::Request>,
) -> Result<get_backup_info::v3::Response> {
	let algorithm = services
		.key_backups
		.get_backup(body.sender_user(), &body.version)
		.await
		.map_err(|_| {
			err!(Request(NotFound("Key backup does not exist at version {:?}", body.version)))
		})?;

	Ok(get_backup_info::v3::Response {
		algorithm,
		count: services
			.key_backups
			.count_keys(body.sender_user(), &body.version)
			.await
			.try_into()?,
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &body.version)
			.await,
		version: body.version.clone(),
	})
}

/// # `DELETE /_matrix/client/r0/room_keys/version/{version}`
///
/// Delete an existing key backup.
///
/// - Deletes both information about the backup, as well as all key data related
///   to the backup
pub(crate) async fn delete_backup_version_route(
	State(services): State<crate::State>,
	body: Ruma<delete_backup_version::v3::Request>,
) -> Result<delete_backup_version::v3::Response> {
	services
		.key_backups
		.delete_backup(body.sender_user(), &body.version)
		.await;

	Ok(delete_backup_version::v3::Response {})
}

/// # `PUT /_matrix/client/r0/room_keys/keys`
///
/// Add the received backup keys to the database.
///
/// - Only manipulating the most recently created version of the backup is
///   allowed
/// - Adds the keys to the backup
/// - Returns the new number of keys in this backup and the etag
pub(crate) async fn add_backup_keys_route(
	State(services): State<crate::State>,
	body: Ruma<add_backup_keys::v3::Request>,
) -> Result<add_backup_keys::v3::Response> {
	if services
		.key_backups
		.get_latest_backup_version(body.sender_user())
		.await
		.is_ok_and(|version| version != body.version)
	{
		return Err!(Request(InvalidParam(
			"You may only manipulate the most recently created version of the backup."
		)));
	}

	for (room_id, room) in &body.rooms {
		for (session_id, key_data) in &room.sessions {
			services
				.key_backups
				.add_key(body.sender_user(), &body.version, room_id, session_id, key_data)
				.await?;
		}
	}

	Ok(add_backup_keys::v3::Response {
		count: services
			.key_backups
			.count_keys(body.sender_user(), &body.version)
			.await
			.try_into()?,
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &body.version)
			.await,
	})
}

/// # `PUT /_matrix/client/r0/room_keys/keys/{roomId}`
///
/// Add the received backup keys to the database.
///
/// - Only manipulating the most recently created version of the backup is
///   allowed
/// - Adds the keys to the backup
/// - Returns the new number of keys in this backup and the etag
pub(crate) async fn add_backup_keys_for_room_route(
	State(services): State<crate::State>,
	body: Ruma<add_backup_keys_for_room::v3::Request>,
) -> Result<add_backup_keys_for_room::v3::Response> {
	if services
		.key_backups
		.get_latest_backup_version(body.sender_user())
		.await
		.is_ok_and(|version| version != body.version)
	{
		return Err!(Request(InvalidParam(
			"You may only manipulate the most recently created version of the backup."
		)));
	}

	for (session_id, key_data) in &body.sessions {
		// Check if we already have a better key
		let new_key = key_data.deserialize()?;
		let current_key = services
			.key_backups
			.get_session(body.sender_user(), &body.version, &body.room_id, session_id)
			.await?
			.deserialize()?;

		// Prefer key that `is_verified`
		if current_key.is_verified != new_key.is_verified {
			if current_key.is_verified {
				warn!("rejected key because of `is_verified` current_key: {:?} new_key: {:?}", current_key, new_key);
				continue;
			}
		} else {
			// If both have same `is_verified`, prefer the one with lower
			// `first_message_index`
			if new_key.first_message_index > current_key.first_message_index {
				warn!("rejected key because of `first_message_index` current_key: {:?} new_key: {:?}", current_key, new_key);
				continue;
			} else if (new_key.first_message_index == current_key.first_message_index)
			// If both have same `first_message_index`, prefer the one with lower `forwarded_count`
			&& (new_key.forwarded_count > current_key.forwarded_count)
			{
				warn!("rejected key because of `forwarded_count` current_key: {:?} new_key: {:?}", current_key, new_key);
				continue;
			}
		};

		warn!("new key accepted. current_key: {:?} new_key: {:?}", current_key, new_key);
		services
			.key_backups
			.add_key(body.sender_user(), &body.version, &body.room_id, session_id, key_data)
			.await?;
	}

	Ok(add_backup_keys_for_room::v3::Response {
		count: services
			.key_backups
			.count_keys(body.sender_user(), &body.version)
			.await
			.try_into()?,
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &body.version)
			.await,
	})
}

/// # `PUT /_matrix/client/r0/room_keys/keys/{roomId}/{sessionId}`
///
/// Add the received backup key to the database.
///
/// - Only manipulating the most recently created version of the backup is
///   allowed
/// - Adds the keys to the backup
/// - Returns the new number of keys in this backup and the etag
pub(crate) async fn add_backup_keys_for_session_route(
	State(services): State<crate::State>,
	body: Ruma<add_backup_keys_for_session::v3::Request>,
) -> Result<add_backup_keys_for_session::v3::Response> {
	if services
		.key_backups
		.get_latest_backup_version(body.sender_user())
		.await
		.is_ok_and(|version| version != body.version)
	{
		return Err!(Request(InvalidParam(
			"You may only manipulate the most recently created version of the backup."
		)));
	}

	services
		.key_backups
		.add_key(
			body.sender_user(),
			&body.version,
			&body.room_id,
			&body.session_id,
			&body.session_data,
		)
		.await?;

	Ok(add_backup_keys_for_session::v3::Response {
		count: services
			.key_backups
			.count_keys(body.sender_user(), &body.version)
			.await
			.try_into()?,
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &body.version)
			.await,
	})
}

/// # `GET /_matrix/client/r0/room_keys/keys`
///
/// Retrieves all keys from the backup.
pub(crate) async fn get_backup_keys_route(
	State(services): State<crate::State>,
	body: Ruma<get_backup_keys::v3::Request>,
) -> Result<get_backup_keys::v3::Response> {
	let rooms = services
		.key_backups
		.get_all(body.sender_user(), &body.version)
		.await;

	Ok(get_backup_keys::v3::Response { rooms })
}

/// # `GET /_matrix/client/r0/room_keys/keys/{roomId}`
///
/// Retrieves all keys from the backup for a given room.
pub(crate) async fn get_backup_keys_for_room_route(
	State(services): State<crate::State>,
	body: Ruma<get_backup_keys_for_room::v3::Request>,
) -> Result<get_backup_keys_for_room::v3::Response> {
	let sessions = services
		.key_backups
		.get_room(body.sender_user(), &body.version, &body.room_id)
		.await;

	Ok(get_backup_keys_for_room::v3::Response { sessions })
}

/// # `GET /_matrix/client/r0/room_keys/keys/{roomId}/{sessionId}`
///
/// Retrieves a key from the backup.
pub(crate) async fn get_backup_keys_for_session_route(
	State(services): State<crate::State>,
	body: Ruma<get_backup_keys_for_session::v3::Request>,
) -> Result<get_backup_keys_for_session::v3::Response> {
	let key_data = services
		.key_backups
		.get_session(body.sender_user(), &body.version, &body.room_id, &body.session_id)
		.await
		.map_err(|_| {
			err!(Request(NotFound(debug_error!("Backup key not found for this user's session."))))
		})?;

	Ok(get_backup_keys_for_session::v3::Response { key_data })
}

/// # `DELETE /_matrix/client/r0/room_keys/keys`
///
/// Delete the keys from the backup.
pub(crate) async fn delete_backup_keys_route(
	State(services): State<crate::State>,
	body: Ruma<delete_backup_keys::v3::Request>,
) -> Result<delete_backup_keys::v3::Response> {
	services
		.key_backups
		.delete_all_keys(body.sender_user(), &body.version)
		.await;

	Ok(delete_backup_keys::v3::Response {
		count: services
			.key_backups
			.count_keys(body.sender_user(), &body.version)
			.await
			.try_into()?,
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &body.version)
			.await,
	})
}

/// # `DELETE /_matrix/client/r0/room_keys/keys/{roomId}`
///
/// Delete the keys from the backup for a given room.
pub(crate) async fn delete_backup_keys_for_room_route(
	State(services): State<crate::State>,
	body: Ruma<delete_backup_keys_for_room::v3::Request>,
) -> Result<delete_backup_keys_for_room::v3::Response> {
	services
		.key_backups
		.delete_room_keys(body.sender_user(), &body.version, &body.room_id)
		.await;

	Ok(delete_backup_keys_for_room::v3::Response {
		count: services
			.key_backups
			.count_keys(body.sender_user(), &body.version)
			.await
			.try_into()?,
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &body.version)
			.await,
	})
}

/// # `DELETE /_matrix/client/r0/room_keys/keys/{roomId}/{sessionId}`
///
/// Delete a key from the backup.
pub(crate) async fn delete_backup_keys_for_session_route(
	State(services): State<crate::State>,
	body: Ruma<delete_backup_keys_for_session::v3::Request>,
) -> Result<delete_backup_keys_for_session::v3::Response> {
	services
		.key_backups
		.delete_room_key(body.sender_user(), &body.version, &body.room_id, &body.session_id)
		.await;

	Ok(delete_backup_keys_for_session::v3::Response {
		count: services
			.key_backups
			.count_keys(body.sender_user(), &body.version)
			.await
			.try_into()?,
		etag: services
			.key_backups
			.get_etag(body.sender_user(), &body.version)
			.await,
	})
}
