//! D49: password recovery keys for an offline app (no e-mail reset link is possible).
//!
//! A key is `TCRK-` + 20 characters of Crockford base32 (100 random bits) in groups of four,
//! e.g. `TCRK-7H3F-Q9XM-2LPA-8VDN-K4TR`. It is shown once; only the SHA-256 of its normalised
//! form is stored, one key per user. Resetting a password with it consumes it and issues a new
//! one in the same transaction, so a key works exactly once.

use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

pub const PREFIX: &str = "TCRK";
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHARS: usize = 20;

/// A new random key, formatted for people.
pub fn generate() -> AppResult<String> {
    let bytes = app_core::secrets::random_bytes(CHARS).map_err(AppError::internal)?;
    let body: String = bytes.iter().map(|b| ALPHABET[usize::from(b & 31)] as char).collect();
    let groups: Vec<&str> = (0..CHARS).step_by(4).map(|i| &body[i..i + 4]).collect();
    Ok(format!("{PREFIX}-{}", groups.join("-")))
}

/// What a person typed, reduced to the 20 key characters: case, spaces, dashes and the `TCRK`
/// prefix don't matter, and the letters people confuse with digits (O, I, L) are read as 0 / 1.
/// `None` when it can't be a key.
pub fn normalise(input: &str) -> Option<String> {
    let mut s: String = input.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_uppercase()).collect();
    if s.len() == CHARS + PREFIX.len() && s.starts_with(PREFIX) {
        s.drain(..PREFIX.len());
    }
    let s: String = s
        .chars()
        .map(|c| match c {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect();
    (s.len() == CHARS && s.bytes().all(|b| ALPHABET.contains(&b))).then_some(s)
}

fn hash(normalised: &str) -> String {
    Sha256::digest(normalised.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Stores a new key for the user (replacing any earlier one) and returns it.
pub async fn issue_in(conn: &mut SqliteConnection, user_id: &str) -> AppResult<String> {
    let key = generate()?;
    let normalised = normalise(&key).ok_or_else(|| AppError::internal("generated recovery key does not parse"))?;
    sqlx::query(
        r#"INSERT INTO "recovery_keys" ("userId","keyHash","createdAt") VALUES (?1,?2,?3)
           ON CONFLICT ("userId") DO UPDATE SET "keyHash" = excluded."keyHash", "createdAt" = excluded."createdAt""#,
    )
    .bind(user_id)
    .bind(hash(&normalised))
    .bind(now_iso())
    .execute(conn)
    .await?;
    Ok(key)
}

pub async fn issue(db: &Db, user_id: &str) -> AppResult<String> {
    let mut tx = db.begin().await?;
    let key = issue_in(&mut tx, user_id).await?;
    tx.commit().await?;
    Ok(key)
}

/// Cancels the user's key, if any (an administrator's password reset, or switching the user off).
pub async fn revoke_in(conn: &mut SqliteConnection, user_id: &str) -> AppResult<()> {
    sqlx::query(r#"DELETE FROM "recovery_keys" WHERE "userId" = ?1"#).bind(user_id).execute(conn).await?;
    Ok(())
}

/// When the user's current key was made, if they have one.
pub async fn created_at(db: &Db, user_id: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query_scalar(r#"SELECT "createdAt" FROM "recovery_keys" WHERE "userId" = ?1"#)
        .bind(user_id)
        .fetch_optional(db.read())
        .await?)
}

/// Whether `input` is the user's current key. Constant-time comparison of the hashes.
pub async fn matches_in(conn: &mut SqliteConnection, user_id: &str, input: &str) -> AppResult<bool> {
    let Some(normalised) = normalise(input) else { return Ok(false) };
    let stored: Option<String> = sqlx::query_scalar(r#"SELECT "keyHash" FROM "recovery_keys" WHERE "userId" = ?1"#)
        .bind(user_id)
        .fetch_optional(conn)
        .await?;
    let Some(stored) = stored else { return Ok(false) };
    let given = hash(&normalised);
    Ok(stored.len() == given.len() && stored.bytes().zip(given.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0)
}

/// Sets `username`'s password with their recovery key. On success the key is used up and a
/// new one is returned with the user; `None` (nothing changed) when the user, the key or the
/// match is missing — the caller answers all three the same way.
pub async fn reset_with_key(
    db: &Db,
    username: &str,
    key: &str,
    new_password: &str,
) -> AppResult<Option<(crate::User, String)>> {
    // Hash first, whatever happens next, so a wrong username takes as long as a wrong key.
    let hash = crate::users::hash_password(new_password).await?;
    let mut tx = db.begin().await?;
    let Some(user) = crate::users::find_by_username(&mut tx, username).await? else { return Ok(None) };
    if !matches_in(&mut tx, &user.id, key).await? {
        return Ok(None);
    }
    crate::users::set_password_hash_in(&mut tx, &user.id, &hash).await?;
    let next = issue_in(&mut tx, &user.id).await?;
    let user = crate::users::require(&mut tx, &user.id).await?;
    tx.commit().await?;
    Ok(Some((user, next)))
}

/// Sets `username`'s password after the person at the main PC proved they are the Windows user
/// that runs TallyCheck (the caller checks that). Issues a new recovery key, since the old one
/// was evidently lost. `None` when there is no such user.
pub async fn reset_for_windows_owner(
    db: &Db,
    username: &str,
    new_password: &str,
) -> AppResult<Option<(crate::User, String)>> {
    let hash = crate::users::hash_password(new_password).await?;
    let mut tx = db.begin().await?;
    let Some(user) = crate::users::find_by_username(&mut tx, username).await? else { return Ok(None) };
    crate::users::set_password_hash_in(&mut tx, &user.id, &hash).await?;
    let next = issue_in(&mut tx, &user.id).await?;
    let user = crate::users::require(&mut tx, &user.id).await?;
    tx.commit().await?;
    Ok(Some((user, next)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_have_the_documented_shape_and_parse() {
        for _ in 0..200 {
            let k = generate().expect("test value");
            assert_eq!(k.len(), 4 + 5 * 5, "{k}");
            assert!(k.starts_with("TCRK-"));
            assert_eq!(k.split('-').count(), 6);
            assert_eq!(normalise(&k).expect("test value").len(), 20);
        }
        assert_ne!(generate().expect("test value"), generate().expect("test value"));
    }

    #[test]
    fn typing_mistakes_people_make_still_match() {
        let n = normalise("TCRK-7H3F-Q9XM-2PA0-8VDN-K4TR").expect("test value");
        assert_eq!(normalise("tcrk 7h3f q9xm 2pa0 8vdn k4tr").expect("test value"), n);
        assert_eq!(normalise("7H3FQ9XM2PA08VDNK4TR").expect("test value"), n);
        assert_eq!(normalise("TCRK-7H3F-Q9XM-2PAO-8VDN-K4TR").expect("test value"), n, "O read as 0");
        assert_eq!(normalise("1111-1111-1111-1111-1111"), normalise("IiLl-1111-1111-1111-1111"));
    }

    #[test]
    fn wrong_lengths_and_letters_are_not_keys() {
        assert_eq!(normalise(""), None);
        assert_eq!(normalise("TCRK-7H3F-Q9XM-2PA0-8VD"), None);
        assert_eq!(normalise("7H3F-Q9XM-2PA0-8VDN"), None);
        assert_eq!(normalise("TCRK-7H3F-Q9XM-2PA0-8VDN-K4TR-AAAA"), None);
        assert_eq!(normalise("TCRK-7H3F-Q9XM-2PA0-8VDN-K4TU"), None, "U is not in the alphabet");
    }

    async fn fresh() -> AppResult<(tempfile::TempDir, Db, crate::User)> {
        let dir = tempfile::tempdir().map_err(AppError::internal)?;
        let db = Db::open(&dir.path().join("t.db")).await?;
        let user = crate::users::create_first_admin(&db, "owner", "old-password").await?.expect("test value");
        Ok((dir, db, user))
    }

    async fn password_is(db: &Db, password: &str) -> AppResult<bool> {
        let u = crate::users::get_by_username(db, "owner").await?.expect("test value");
        Ok(matches!(
            crate::users::verify_password(password, &u.password_hash).await?,
            crate::password::Verified::Match { .. }
        ))
    }

    #[tokio::test]
    async fn a_key_resets_the_password_once_and_is_replaced() -> AppResult<()> {
        let (_d, db, user) = fresh().await?;
        assert_eq!(created_at(&db, &user.id).await?, None);
        let key = issue(&db, &user.id).await?;
        assert!(created_at(&db, &user.id).await?.is_some());

        let (u, next) = reset_with_key(&db, "owner", &key.to_lowercase(), "new-password-1").await?.expect("test value");
        assert_eq!(u.id, user.id);
        assert_ne!(next, key);
        assert!(password_is(&db, "new-password-1").await?);

        // The used key no longer works; the new one does.
        assert!(reset_with_key(&db, "owner", &key, "new-password-2").await?.is_none());
        assert!(password_is(&db, "new-password-1").await?);
        assert!(reset_with_key(&db, "owner", &next, "new-password-2").await?.is_some());
        assert!(password_is(&db, "new-password-2").await?);
        Ok(())
    }

    #[tokio::test]
    async fn wrong_user_wrong_key_or_no_key_change_nothing() -> AppResult<()> {
        let (_d, db, user) = fresh().await?;
        let other = crate::users::create_user(
            &db,
            crate::users::CreateUser { username: "clerk".into(), password: "clerk-pass".into(), role_id: None },
        )
        .await?;
        assert!(reset_with_key(&db, "owner", &generate()?, "x-password").await?.is_none(), "no key issued yet");
        let key = issue(&db, &user.id).await?;
        let clerk_key = issue(&db, &other.id).await?;
        assert!(reset_with_key(&db, "nobody", &key, "x-password").await?.is_none());
        assert!(reset_with_key(&db, "owner", &clerk_key, "x-password").await?.is_none(), "another user's key");
        assert!(reset_with_key(&db, "owner", "not a key", "x-password").await?.is_none());
        assert!(password_is(&db, "old-password").await?);
        // Issuing again replaces the key.
        let again = issue(&db, &user.id).await?;
        assert!(reset_with_key(&db, "owner", &key, "x-password").await?.is_none());
        assert!(reset_with_key(&db, "owner", &again, "x-password").await?.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn an_admin_reset_or_switching_the_user_off_cancels_the_key() -> AppResult<()> {
        let (_d, db, owner) = fresh().await?;
        let clerk = crate::users::create_user(
            &db,
            crate::users::CreateUser { username: "clerk".into(), password: "clerk-pass".into(), role_id: None },
        )
        .await?;
        let key = issue(&db, &clerk.id).await?;
        crate::users::reset_password(&db, &clerk.id, "set-by-admin").await?;
        assert_eq!(created_at(&db, &clerk.id).await?, None);
        assert!(reset_with_key(&db, "clerk", &key, "x-password").await?.is_none(), "key cancelled by the reset");

        let key = issue(&db, &clerk.id).await?;
        let off = crate::users::UpdateUser { username: None, role_id: None, is_active: Some(false) };
        crate::users::update_user(&db, &clerk.id, off).await?;
        let on = crate::users::UpdateUser { username: None, role_id: None, is_active: Some(true) };
        crate::users::update_user(&db, &clerk.id, on).await?;
        assert!(reset_with_key(&db, "clerk", &key, "x-password").await?.is_none(), "key cancelled when switched off");
        assert!(created_at(&db, &owner.id).await?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn windows_owner_reset_sets_the_password_and_issues_a_key() -> AppResult<()> {
        let (_d, db, user) = fresh().await?;
        let old = issue(&db, &user.id).await?;
        assert!(reset_for_windows_owner(&db, "nobody", "new-password-1").await?.is_none());
        let (_, key) = reset_for_windows_owner(&db, "owner", "new-password-1").await?.expect("test value");
        assert!(password_is(&db, "new-password-1").await?);
        assert!(reset_with_key(&db, "owner", &old, "x-password").await?.is_none(), "the old key is replaced");
        assert!(reset_with_key(&db, "owner", &key, "x-password").await?.is_some());
        Ok(())
    }
}
