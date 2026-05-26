use std::path::Path;

const BLAKE3_DIGEST_PREFIX: &str = "blake3:";

pub(super) fn normalize_relative_path(path: &Path) -> String {
    let mut normalized = String::with_capacity(path.as_os_str().len());
    for component in path.components() {
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(&component.as_os_str().to_string_lossy());
    }
    normalized
}

pub(super) fn blake3_digest(bytes: &[u8]) -> String {
    blake3_prefixed_digest(blake3::hash(bytes))
}

pub(super) fn blake3_prefixed_digest(hash: blake3::Hash) -> String {
    let hex = hash.to_hex();
    let mut digest = String::with_capacity(BLAKE3_DIGEST_PREFIX.len() + hex.len());
    digest.push_str(BLAKE3_DIGEST_PREFIX);
    digest.push_str(hex.as_str());
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake3_digest_preserves_prefixed_hex_shape() {
        let expected = format!("blake3:{}", blake3::hash(b"openspec"));
        assert_eq!(blake3_digest(b"openspec"), expected);
    }
}
