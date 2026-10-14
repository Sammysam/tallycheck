//! Identifiers. Primary keys are random v4 UUIDs, lower-case, like `uuid_generate_v4()`.

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// class-validator `IsUUID()` as shipped with the reference (validator 13.15, version "all"):
/// version nibble 1-8 and variant 8/9/a/b, or the nil / max UUID. Case-insensitive.
pub fn is_uuid(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower == "00000000-0000-0000-0000-000000000000" || lower == "ffffffff-ffff-ffff-ffff-ffffffffffff" {
        return true;
    }
    let b = lower.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
        && (b'1'..=b'8').contains(&b[14])
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_uuids() {
        let id = new_id();
        assert!(is_uuid(&id));
        assert_eq!(id, id.to_lowercase());
    }

    #[test]
    fn matches_reference_validator() {
        // Expected values were produced by the reference's validator/lib/isUUID.
        assert!(is_uuid("7C9E6679-7425-40DE-944B-E07FC1F90AE7"));
        assert!(is_uuid("7c9e6679-7425-40de-944b-e07fc1f90ae7"));
        assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
        assert!(is_uuid("ffffffff-ffff-ffff-ffff-ffffffffffff"));
        assert!(!is_uuid("11111111-1111-1111-1111-111111111111"));
        assert!(!is_uuid("7c9e6679-7425-00de-944b-e07fc1f90ae7"));
        assert!(!is_uuid("7c9e6679-7425-40de-c44b-e07fc1f90ae7"));
        assert!(!is_uuid("7c9e6679742540de944be07fc1f90ae7"));
        assert!(!is_uuid("not-a-uuid"));
    }
}
