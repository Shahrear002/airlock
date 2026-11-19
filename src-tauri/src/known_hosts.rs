use base64::Engine;
use russh_keys::PublicKeyBase64;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KnownHostEntry {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub public_key_base64: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    Trusted,
    Unknown,
    NewKeyType(Vec<KnownHostEntry>),
    Changed(Vec<KnownHostEntry>),
}

/// Refuses host names containing whitespace, control characters, or forbidden symbols.
pub fn validate_host(host: &str) -> Result<(), String> {
    if host.is_empty() {
        return Err("Host name cannot be empty".to_string());
    }
    for c in host.chars() {
        if c.is_whitespace() || c.is_control() || matches!(c, ',' | '*' | '?' | '[' | ']') {
            return Err(format!("Host name contains forbidden character: {:?}", c));
        }
    }
    Ok(())
}

/// Formats a host pattern for known_hosts (port 22 is bare host, other ports are [host]:port).
/// Hosts are canonicalized to lowercase.
pub fn format_host_pattern(host: &str, port: u16) -> Result<String, String> {
    validate_host(host)?;
    let h = host.to_ascii_lowercase();
    if port == 22 {
        Ok(h)
    } else {
        Ok(format!("[{}]:{}", h, port))
    }
}

/// Parses a host pattern into (lowercase host, port).
pub fn parse_host_pattern(pattern: &str) -> Option<(String, u16)> {
    if pattern.starts_with('[') {
        if let Some(idx) = pattern.find("]:") {
            let h = pattern[1..idx].to_ascii_lowercase();
            let pt = pattern[idx + 2..].parse::<u16>().ok()?;
            Some((h, pt))
        } else {
            None
        }
    } else {
        Some((pattern.to_ascii_lowercase(), 22))
    }
}

/// Extracts the canonical key type identifier from the first string in an OpenSSH public key blob.
/// (e.g. "ssh-rsa", "ssh-ed25519", "ecdsa-sha2-nistp256").
pub fn extract_key_type_from_blob(blob: &[u8]) -> Result<String, String> {
    if blob.len() < 4 {
        return Err("Key blob is too short".to_string());
    }
    let len = u32::from_be_bytes([blob[0], blob[1], blob[2], blob[3]]) as usize;
    if blob.len() < 4 + len {
        return Err("Truncated key blob header".to_string());
    }
    let key_type_str = std::str::from_utf8(&blob[4..4 + len])
        .map_err(|e| format!("Invalid UTF-8 in key type: {}", e))?;
    Ok(key_type_str.to_string())
}

/// Extracts (canonical_key_type, fingerprint, base64_representation) from a PublicKey.
pub fn key_info(key: &russh_keys::key::PublicKey) -> Result<(String, String, String), String> {
    let b64 = key.public_key_base64();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&b64)
        .map_err(|e| format!("Failed to base64 decode key: {}", e))?;
    let key_type = extract_key_type_from_blob(&decoded)?;
    let fingerprint = format!("SHA256:{}", key.fingerprint());
    Ok((key_type, fingerprint, b64))
}

/// Attempts to parse a managed entry from a known_hosts line.
/// Returns None for unmanaged lines (comments, blank lines, hashed lines |1|...,
/// @cert-authority/@revoked markers, comma-separated host lists, or unparseable lines).
pub fn parse_line_entry(line: &str) -> Option<(KnownHostEntry, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with('@')
        || trimmed.starts_with('|')
    {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let host_part = parts.next()?;
    if host_part.contains(',') || host_part.contains('*') || host_part.contains('?') {
        return None;
    }
    let (host, port) = parse_host_pattern(host_part)?;
    let _stated_type = parts.next()?;
    let b64_key = parts.next()?;

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64_key)
        .ok()?;
    let key_type = extract_key_type_from_blob(&decoded).ok()?;
    let pubkey = russh_keys::parse_public_key_base64(b64_key).ok()?;
    let fingerprint = format!("SHA256:{}", pubkey.fingerprint());

    Some((
        KnownHostEntry {
            host,
            port,
            key_type,
            fingerprint,
            public_key_base64: b64_key.to_string(),
        },
        host_part,
    ))
}

#[derive(Debug, Clone)]
pub struct KnownHostsStore {
    path: PathBuf,
}

impl KnownHostsStore {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads all lines from the known_hosts file.
    /// If the file does not exist, returns Ok(empty vec).
    /// If the file exists but is unreadable or non-UTF-8, returns Err (fail-closed).
    pub fn read_lines(&self) -> Result<Vec<String>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&self.path).map_err(|e| {
            format!(
                "Failed to read known_hosts at '{}': {}",
                self.path.display(),
                e
            )
        })?;
        Ok(content.lines().map(|s| s.to_string()).collect())
    }

    /// Writes lines atomically via a temporary file in the same directory, then renames.
    /// On Unix, sets permissions to 0600.
    pub fn write_lines(&self, lines: &[String]) -> Result<(), String> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "Failed to create parent directory for '{}': {}",
                self.path.display(),
                e
            )
        })?;

        let mut temp_file = tempfile::Builder::new()
            .prefix("known_hosts.tmp.")
            .tempfile_in(parent)
            .map_err(|e| {
                format!(
                    "Failed to create temp file in '{}': {}",
                    parent.display(),
                    e
                )
            })?;

        use std::io::Write;
        for line in lines {
            writeln!(temp_file, "{}", line)
                .map_err(|e| format!("Failed to write line to temp file: {}", e))?;
        }
        temp_file
            .flush()
            .map_err(|e| format!("Failed to flush temp file: {}", e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(temp_file.path(), perms);
        }

        temp_file.persist(&self.path).map_err(|e| {
            format!(
                "Failed to persist temp file to '{}': {}",
                self.path.display(),
                e
            )
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&self.path, perms);
        }

        Ok(())
    }

    /// Checks a host key against known_hosts.
    /// Returns:
    /// - Trusted: key exactly matches an existing entry for this host+port
    /// - Changed: an existing entry of the SAME key type has a different key
    /// - NewKeyType: host is known, but only under different key types
    /// - Unknown: host is not known at all
    pub fn check(
        &self,
        host: &str,
        port: u16,
        key: &russh_keys::key::PublicKey,
    ) -> Result<CheckResult, String> {
        validate_host(host)?;
        let (incoming_type, _incoming_fp, incoming_b64) = key_info(key)?;
        let target_pat = format_host_pattern(host, port)?;
        let lines = self.read_lines()?;

        let mut matching_entries = Vec::new();
        for line in &lines {
            if let Some((entry, _)) = parse_line_entry(line) {
                if let Ok(entry_pat) = format_host_pattern(&entry.host, entry.port) {
                    if entry_pat == target_pat {
                        matching_entries.push(entry);
                    }
                }
            }
        }

        if matching_entries.is_empty() {
            return Ok(CheckResult::Unknown);
        }

        // Exact match -> Trusted
        for entry in &matching_entries {
            if entry.public_key_base64 == incoming_b64 {
                return Ok(CheckResult::Trusted);
            }
        }

        // Same key type exists with a different key -> Changed
        let same_type_entries: Vec<KnownHostEntry> = matching_entries
            .iter()
            .filter(|e| e.key_type == incoming_type)
            .cloned()
            .collect();
        if !same_type_entries.is_empty() {
            return Ok(CheckResult::Changed(same_type_entries));
        }

        // Host is known under other key types -> NewKeyType
        Ok(CheckResult::NewKeyType(matching_entries))
    }

    /// Adds a host key to known_hosts idempotently.
    /// Preserves all unmanaged lines verbatim.
    pub fn add(
        &self,
        host: &str,
        port: u16,
        key: &russh_keys::key::PublicKey,
    ) -> Result<(), String> {
        validate_host(host)?;
        let (key_type, _fp, b64) = key_info(key)?;
        let target_pat = format_host_pattern(host, port)?;
        let lines = self.read_lines()?;

        for line in &lines {
            if let Some((entry, _)) = parse_line_entry(line) {
                if let Ok(entry_pat) = format_host_pattern(&entry.host, entry.port) {
                    if entry_pat == target_pat && entry.public_key_base64 == b64 {
                        return Ok(()); // Already present, idempotent
                    }
                }
            }
        }

        let new_line = format!("{} {} {}", target_pat, key_type, b64);
        let mut new_lines = lines;
        new_lines.push(new_line);
        self.write_lines(&new_lines)
    }

    /// Replaces ONLY the entry of matching host + port + key_type.
    /// Preserves all other entries (including other key types for this host) and unmanaged lines verbatim.
    pub fn replace_same_type(
        &self,
        host: &str,
        port: u16,
        key: &russh_keys::key::PublicKey,
    ) -> Result<(), String> {
        validate_host(host)?;
        let (key_type, _fp, b64) = key_info(key)?;
        let target_pat = format_host_pattern(host, port)?;
        let lines = self.read_lines()?;

        let new_line = format!("{} {} {}", target_pat, key_type, b64);
        let mut new_lines = Vec::with_capacity(lines.len() + 1);
        let mut replaced = false;

        for line in lines {
            if let Some((entry, _)) = parse_line_entry(&line) {
                if let Ok(entry_pat) = format_host_pattern(&entry.host, entry.port) {
                    if entry_pat == target_pat && entry.key_type == key_type {
                        if !replaced {
                            new_lines.push(new_line.clone());
                            replaced = true;
                        }
                        continue;
                    }
                }
            }
            new_lines.push(line);
        }

        if !replaced {
            new_lines.push(new_line);
        }

        self.write_lines(&new_lines)
    }

    /// Removes a single host key entry identified by (host, port, fingerprint).
    /// Preserves all other entries for this host, other hosts, and unmanaged lines verbatim.
    pub fn remove(&self, host: &str, port: u16, fingerprint: &str) -> Result<(), String> {
        validate_host(host)?;
        let target_pat = format_host_pattern(host, port)?;
        let lines = self.read_lines()?;

        let mut new_lines = Vec::with_capacity(lines.len());
        for line in lines {
            if let Some((entry, _)) = parse_line_entry(&line) {
                if let Ok(entry_pat) = format_host_pattern(&entry.host, entry.port) {
                    if entry_pat == target_pat && entry.fingerprint == fingerprint {
                        continue; // Remove this one key
                    }
                }
            }
            new_lines.push(line);
        }

        self.write_lines(&new_lines)
    }

    /// Lists all managed entries in the known_hosts file.
    pub fn list(&self) -> Result<Vec<KnownHostEntry>, String> {
        let lines = self.read_lines()?;
        let mut entries = Vec::new();
        for line in lines {
            if let Some((entry, _)) = parse_line_entry(&line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Returns all managed entries for a specific host and port.
    pub fn entries_for_host(&self, host: &str, port: u16) -> Result<Vec<KnownHostEntry>, String> {
        validate_host(host)?;
        let target_pat = format_host_pattern(host, port)?;
        let all = self.list()?;
        Ok(all
            .into_iter()
            .filter(|e| format_host_pattern(&e.host, e.port).unwrap_or_default() == target_pat)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn gen_ed25519() -> russh_keys::key::KeyPair {
        russh_keys::key::KeyPair::generate_ed25519().unwrap()
    }

    fn gen_rsa() -> russh_keys::key::KeyPair {
        russh_keys::key::KeyPair::generate_rsa(2048, russh_keys::key::SignatureHash::SHA2_256)
            .unwrap()
    }

    #[test]
    fn test_unknown_add_trusted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        // 1. Initial check on non-existent file -> Unknown
        assert_eq!(
            store.check("example.com", 22, &pk).unwrap(),
            CheckResult::Unknown
        );

        // 2. Add key
        store.add("example.com", 22, &pk).unwrap();

        // 3. Check again -> Trusted
        assert_eq!(
            store.check("example.com", 22, &pk).unwrap(),
            CheckResult::Trusted
        );
    }

    #[test]
    fn test_changed_same_type() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp1 = gen_ed25519();
        let pk1 = kp1.clone_public_key().unwrap();

        let kp2 = gen_ed25519();
        let pk2 = kp2.clone_public_key().unwrap();

        store.add("example.com", 22, &pk1).unwrap();

        // Checking pk2 (same type ssh-ed25519, different key) -> Changed
        match store.check("example.com", 22, &pk2).unwrap() {
            CheckResult::Changed(existing) => {
                assert_eq!(existing.len(), 1);
                assert_eq!(existing[0].key_type, "ssh-ed25519");
                assert_eq!(
                    existing[0].fingerprint,
                    format!("SHA256:{}", pk1.fingerprint())
                );
            }
            other => panic!("Expected Changed, got {:?}", other),
        }
    }

    #[test]
    fn test_new_key_type_is_not_called_changed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let rsa_kp = gen_rsa();
        let rsa_pk = rsa_kp.clone_public_key().unwrap();

        let ed_kp = gen_ed25519();
        let ed_pk = ed_kp.clone_public_key().unwrap();

        // Known via RSA
        store.add("example.com", 22, &rsa_pk).unwrap();

        // Check incoming Ed25519 key -> NewKeyType, NEVER Changed!
        match store.check("example.com", 22, &ed_pk).unwrap() {
            CheckResult::NewKeyType(existing) => {
                assert_eq!(existing.len(), 1);
                assert_eq!(existing[0].key_type, "ssh-rsa");
                assert_eq!(
                    existing[0].fingerprint,
                    format!("SHA256:{}", rsa_pk.fingerprint())
                );
            }
            other => panic!("Expected NewKeyType, got {:?}", other),
        }
    }

    #[test]
    fn test_port_isolation() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        // Port 22
        store.add("example.com", 22, &pk).unwrap();

        // Port 2222 with same key is distinct -> Unknown
        assert_eq!(
            store.check("example.com", 2222, &pk).unwrap(),
            CheckResult::Unknown
        );

        // Add port 2222
        store.add("example.com", 2222, &pk).unwrap();
        assert_eq!(
            store.check("example.com", 2222, &pk).unwrap(),
            CheckResult::Trusted
        );
        assert_eq!(
            store.check("example.com", 22, &pk).unwrap(),
            CheckResult::Trusted
        );
    }

    #[test]
    fn test_case_insensitive_host() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        store.add("Example.COM", 22, &pk).unwrap();

        assert_eq!(
            store.check("example.com", 22, &pk).unwrap(),
            CheckResult::Trusted
        );
        assert_eq!(
            store.check("EXAMPLE.COM", 22, &pk).unwrap(),
            CheckResult::Trusted
        );
    }

    #[test]
    fn test_file_lines_in_openssh_format() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        store.add("example.com", 22, &pk).unwrap();
        store.add("example.com", 2222, &pk).unwrap();

        let lines = store.read_lines().unwrap();
        assert_eq!(lines.len(), 2);

        // Port 22 must be bare host
        assert!(lines[0].starts_with("example.com ssh-ed25519 "));

        // Port 2222 must be [host]:port
        assert!(lines[1].starts_with("[example.com]:2222 ssh-ed25519 "));
    }

    #[test]
    fn test_add_twice_is_noop() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        store.add("example.com", 22, &pk).unwrap();
        let count1 = store.read_lines().unwrap().len();

        store.add("example.com", 22, &pk).unwrap();
        let count2 = store.read_lines().unwrap().len();

        assert_eq!(count1, count2, "add must be idempotent");
    }

    #[test]
    fn test_replace_same_type_keeps_other_types() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let ed1 = gen_ed25519();
        let ed1_pk = ed1.clone_public_key().unwrap();

        let rsa = gen_rsa();
        let rsa_pk = rsa.clone_public_key().unwrap();

        store.add("example.com", 22, &ed1_pk).unwrap();
        store.add("example.com", 22, &rsa_pk).unwrap();

        let ed2 = gen_ed25519();
        let ed2_pk = ed2.clone_public_key().unwrap();

        // Replace ed1 with ed2
        store.replace_same_type("example.com", 22, &ed2_pk).unwrap();

        // Now ed2 is trusted, ed1 is not trusted, and RSA is still trusted
        assert_eq!(
            store.check("example.com", 22, &ed2_pk).unwrap(),
            CheckResult::Trusted
        );
        assert_eq!(
            store.check("example.com", 22, &rsa_pk).unwrap(),
            CheckResult::Trusted
        );
        match store.check("example.com", 22, &ed1_pk).unwrap() {
            CheckResult::Changed(_) => {}
            other => panic!("Expected Changed for old key, got {:?}", other),
        }
    }

    #[test]
    fn test_remove_by_fingerprint_keeps_unmanaged_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let ed = gen_ed25519();
        let ed_pk = ed.clone_public_key().unwrap();
        let ed_fp = format!("SHA256:{}", ed_pk.fingerprint());

        let rsa = gen_rsa();
        let rsa_pk = rsa.clone_public_key().unwrap();

        // Pre-populate with comments, hashed lines, @cert-authority, comma lines
        let unmanaged = vec![
            "# Managed by OpenSSH admin".to_string(),
            "@cert-authority *.example.com ssh-rsa AAAAB3...".to_string(),
            "|1|hashedhost|salt= ssh-ed25519 AAAAC3...".to_string(),
            "host1,host2 ssh-rsa AAAAB3...".to_string(),
            "malformed line without enough fields".to_string(),
        ];
        store.write_lines(&unmanaged).unwrap();

        store.add("example.com", 22, &ed_pk).unwrap();
        store.add("example.com", 22, &rsa_pk).unwrap();

        // Remove only the ed key by fingerprint
        store.remove("example.com", 22, &ed_fp).unwrap();

        let lines = store.read_lines().unwrap();
        // Verify all unmanaged lines preserved
        for u in &unmanaged {
            assert!(
                lines.contains(u),
                "Unmanaged line '{}' must be preserved verbatim",
                u
            );
        }

        // Verify RSA is preserved and Ed is gone
        assert_eq!(
            store.check("example.com", 22, &rsa_pk).unwrap(),
            CheckResult::Trusted
        );
        match store.check("example.com", 22, &ed_pk).unwrap() {
            CheckResult::NewKeyType(_) => {}
            other => panic!("Expected NewKeyType after Ed removal, got {:?}", other),
        }
    }

    #[test]
    fn test_hostile_host_names_refused_and_nothing_written() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        let hostile = [
            "foo bar",
            "foo\tbar",
            "foo\nbar",
            "foo\x00bar",
            "host,alias",
            "*.example.com",
            "?example.com",
            "[bracketed]",
            "",
        ];

        for h in hostile {
            assert!(
                store.check(h, 22, &pk).is_err(),
                "Hostile host {:?} must be refused on check",
                h
            );
            assert!(
                store.add(h, 22, &pk).is_err(),
                "Hostile host {:?} must be refused on add",
                h
            );
            assert!(
                store.replace_same_type(h, 22, &pk).is_err(),
                "Hostile host {:?} must be refused on replace",
                h
            );
            assert!(
                store.remove(h, 22, "SHA256:abc").is_err(),
                "Hostile host {:?} must be refused on remove",
                h
            );
        }

        // Nothing was written
        assert!(
            !path.exists() || store.read_lines().unwrap().is_empty(),
            "Refused operations must write nothing"
        );
    }

    #[test]
    fn test_missing_file_means_no_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent_known_hosts");
        let store = KnownHostsStore::new(&path);

        assert_eq!(store.list().unwrap(), Vec::new());
        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();
        assert_eq!(
            store.check("example.com", 22, &pk).unwrap(),
            CheckResult::Unknown
        );
    }

    #[test]
    fn test_unreadable_file_fails_closed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad_utf8_known_hosts");

        // Write invalid UTF-8 bytes
        std::fs::write(&path, [0xFF, 0xFE, 0xFD]).unwrap();
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        assert!(
            store.read_lines().is_err(),
            "Unreadable/non-UTF8 file must return Err"
        );
        assert!(
            store.check("example.com", 22, &pk).is_err(),
            "Unreadable file must return Err (fail closed), not Unknown"
        );
        assert!(
            store.list().is_err(),
            "Unreadable file must fail closed on list"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_unix_file_mode_0600() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = KnownHostsStore::new(&path);

        let kp = gen_ed25519();
        let pk = kp.clone_public_key().unwrap();

        store.add("example.com", 22, &pk).unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "known_hosts file mode must be 0600 on Unix");
    }
}
