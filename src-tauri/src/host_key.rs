use crate::known_hosts::{key_info, CheckResult, KnownHostsStore};
use async_trait::async_trait;
use russh_keys::key::{self, PublicKey};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyDecision {
    Reject,
    Trust,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyPromptKind {
    Unknown,
    NewKeyType,
    Changed,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HostKeyPrompt {
    pub kind: HostKeyPromptKind,
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub existing_fingerprints: Vec<String>,
}

#[async_trait]
pub trait HostKeyPrompter: Send + Sync {
    async fn ask(&self, prompt: HostKeyPrompt) -> HostKeyDecision;
}

pub struct HostKeyVerifier {
    pub store: Arc<KnownHostsStore>,
    pub prompter: Arc<dyn HostKeyPrompter>,
    pub host: String,
    pub port: u16,
    failure_reason: std::sync::Mutex<Option<String>>,
}

impl HostKeyVerifier {
    pub fn new(
        store: Arc<KnownHostsStore>,
        prompter: Arc<dyn HostKeyPrompter>,
        host: String,
        port: u16,
    ) -> Self {
        Self {
            store,
            prompter,
            host,
            port,
            failure_reason: std::sync::Mutex::new(None),
        }
    }

    /// Returns the technical reason why verification failed (cleared when read).
    pub fn take_failure(&self) -> Option<String> {
        self.failure_reason.lock().unwrap().take()
    }

    /// Returns the preferred host key algorithms for this host.
    /// Puts algorithms already known for this host FIRST, followed by russh defaults without duplicates:
    /// - ssh-ed25519 -> ED25519
    /// - ssh-rsa -> RSA_SHA2_512, then RSA_SHA2_256
    /// - ecdsa-sha2-nistp256 -> ECDSA_SHA2_NISTP256
    /// - ecdsa-sha2-nistp521 -> ECDSA_SHA2_NISTP521
    pub fn preferred_algorithms(&self) -> Vec<key::Name> {
        let mut result = Vec::new();

        if let Ok(entries) = self.store.entries_for_host(&self.host, self.port) {
            for entry in entries {
                match entry.key_type.as_str() {
                    "ssh-ed25519" => {
                        if !result.contains(&key::ED25519) {
                            result.push(key::ED25519);
                        }
                    }
                    "ssh-rsa" => {
                        if !result.contains(&key::RSA_SHA2_512) {
                            result.push(key::RSA_SHA2_512);
                        }
                        if !result.contains(&key::RSA_SHA2_256) {
                            result.push(key::RSA_SHA2_256);
                        }
                    }
                    "ecdsa-sha2-nistp256" => {
                        if !result.contains(&key::ECDSA_SHA2_NISTP256) {
                            result.push(key::ECDSA_SHA2_NISTP256);
                        }
                    }
                    "ecdsa-sha2-nistp521" => {
                        if !result.contains(&key::ECDSA_SHA2_NISTP521) {
                            result.push(key::ECDSA_SHA2_NISTP521);
                        }
                    }
                    _ => {}
                }
            }
        }

        let defaults = [
            key::ED25519,
            key::ECDSA_SHA2_NISTP256,
            key::ECDSA_SHA2_NISTP521,
            key::RSA_SHA2_512,
            key::RSA_SHA2_256,
        ];

        for alg in defaults {
            if !result.contains(&alg) {
                result.push(alg);
            }
        }

        result
    }

    /// Verifies the server public key against the known_hosts store.
    /// Returns true if trusted, false on rejection or error (fail closed).
    pub async fn verify(&self, key: &PublicKey) -> bool {
        let check_res = match self.store.check(&self.host, self.port, key) {
            Ok(r) => r,
            Err(e) => {
                log::error!(
                    "Host key check failed for {}:{}: {}",
                    self.host,
                    self.port,
                    e
                );
                *self.failure_reason.lock().unwrap() = Some(e);
                return false;
            }
        };

        match check_res {
            CheckResult::Trusted => true,
            CheckResult::Unknown => {
                let (key_type, fingerprint, _) = match key_info(key) {
                    Ok(info) => info,
                    Err(e) => {
                        *self.failure_reason.lock().unwrap() = Some(e);
                        return false;
                    }
                };
                let prompt = HostKeyPrompt {
                    kind: HostKeyPromptKind::Unknown,
                    host: self.host.clone(),
                    port: self.port,
                    key_type,
                    fingerprint,
                    existing_fingerprints: Vec::new(),
                };
                let decision = self.prompter.ask(prompt).await;
                match decision {
                    HostKeyDecision::Trust => {
                        if let Err(e) = self.store.add(&self.host, self.port, key) {
                            log::error!("Failed to store trusted host key: {}", e);
                            *self.failure_reason.lock().unwrap() = Some(e);
                            false
                        } else {
                            true
                        }
                    }
                    HostKeyDecision::Reject => false,
                }
            }
            CheckResult::NewKeyType(existing) => {
                let (key_type, fingerprint, _) = match key_info(key) {
                    Ok(info) => info,
                    Err(e) => {
                        *self.failure_reason.lock().unwrap() = Some(e);
                        return false;
                    }
                };
                let prompt = HostKeyPrompt {
                    kind: HostKeyPromptKind::NewKeyType,
                    host: self.host.clone(),
                    port: self.port,
                    key_type,
                    fingerprint,
                    existing_fingerprints: existing.iter().map(|e| e.fingerprint.clone()).collect(),
                };
                let decision = self.prompter.ask(prompt).await;
                match decision {
                    HostKeyDecision::Trust => {
                        if let Err(e) = self.store.add(&self.host, self.port, key) {
                            log::error!("Failed to store new key type: {}", e);
                            *self.failure_reason.lock().unwrap() = Some(e);
                            false
                        } else {
                            true
                        }
                    }
                    HostKeyDecision::Reject => false,
                }
            }
            CheckResult::Changed(existing) => {
                let (key_type, fingerprint, _) = match key_info(key) {
                    Ok(info) => info,
                    Err(e) => {
                        *self.failure_reason.lock().unwrap() = Some(e);
                        return false;
                    }
                };
                let prompt = HostKeyPrompt {
                    kind: HostKeyPromptKind::Changed,
                    host: self.host.clone(),
                    port: self.port,
                    key_type,
                    fingerprint,
                    existing_fingerprints: existing.iter().map(|e| e.fingerprint.clone()).collect(),
                };
                let decision = self.prompter.ask(prompt).await;
                match decision {
                    HostKeyDecision::Trust => {
                        if let Err(e) = self.store.replace_same_type(&self.host, self.port, key) {
                            log::error!("Failed to replace changed host key: {}", e);
                            *self.failure_reason.lock().unwrap() = Some(e);
                            false
                        } else {
                            true
                        }
                    }
                    HostKeyDecision::Reject => false,
                }
            }
        }
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

    #[derive(Default)]
    pub struct ScriptedPrompter {
        pub answers: tokio::sync::Mutex<std::collections::VecDeque<HostKeyDecision>>,
        pub prompts: tokio::sync::Mutex<Vec<HostKeyPrompt>>,
    }

    impl ScriptedPrompter {
        pub fn new(answers: Vec<HostKeyDecision>) -> Self {
            Self {
                answers: tokio::sync::Mutex::new(answers.into()),
                prompts: tokio::sync::Mutex::new(Vec::new()),
            }
        }

        pub async fn recorded_prompts(&self) -> Vec<HostKeyPrompt> {
            self.prompts.lock().await.clone()
        }
    }

    #[async_trait]
    impl HostKeyPrompter for ScriptedPrompter {
        async fn ask(&self, prompt: HostKeyPrompt) -> HostKeyDecision {
            self.prompts.lock().await.push(prompt);
            self.answers
                .lock()
                .await
                .pop_front()
                .unwrap_or(HostKeyDecision::Reject)
        }
    }

    #[tokio::test]
    async fn test_verifier_scripted_prompter_flow() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = Arc::new(KnownHostsStore::new(&path));

        let ed_kp1 = gen_ed25519();
        let ed_pk1 = ed_kp1.clone_public_key().unwrap();

        let ed_kp2 = gen_ed25519();
        let ed_pk2 = ed_kp2.clone_public_key().unwrap();

        let rsa_kp = gen_rsa();
        let rsa_pk = rsa_kp.clone_public_key().unwrap();

        // 1. First connect prompts and saves (Trust)
        {
            let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Trust]));
            let verifier = HostKeyVerifier::new(
                store.clone(),
                prompter.clone(),
                "example.com".to_string(),
                22,
            );

            assert!(verifier.verify(&ed_pk1).await);
            let prompts = prompter.recorded_prompts().await;
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts[0].kind, HostKeyPromptKind::Unknown);
            assert_eq!(
                store.check("example.com", 22, &ed_pk1).unwrap(),
                CheckResult::Trusted
            );
        }

        // 2. Reject saves nothing
        {
            let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Reject]));
            let verifier =
                HostKeyVerifier::new(store.clone(), prompter.clone(), "other.com".to_string(), 22);

            let kp = gen_ed25519();
            let pk = kp.clone_public_key().unwrap();
            assert!(!verifier.verify(&pk).await);
            assert_eq!(
                store.check("other.com", 22, &pk).unwrap(),
                CheckResult::Unknown
            );
        }

        // 3. Changed key reject keeps old key
        {
            let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Reject]));
            let verifier = HostKeyVerifier::new(
                store.clone(),
                prompter.clone(),
                "example.com".to_string(),
                22,
            );

            assert!(!verifier.verify(&ed_pk2).await);
            let prompts = prompter.recorded_prompts().await;
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts[0].kind, HostKeyPromptKind::Changed);
            // Old key still trusted
            assert_eq!(
                store.check("example.com", 22, &ed_pk1).unwrap(),
                CheckResult::Trusted
            );
        }

        // 4. Changed key trust replaces it
        {
            let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Trust]));
            let verifier = HostKeyVerifier::new(
                store.clone(),
                prompter.clone(),
                "example.com".to_string(),
                22,
            );

            assert!(verifier.verify(&ed_pk2).await);
            let prompts = prompter.recorded_prompts().await;
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts[0].kind, HostKeyPromptKind::Changed);
            // ed_pk2 is now trusted
            assert_eq!(
                store.check("example.com", 22, &ed_pk2).unwrap(),
                CheckResult::Trusted
            );
        }

        // 5. New key type prompts with a warning kind
        {
            let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Trust]));
            let verifier = HostKeyVerifier::new(
                store.clone(),
                prompter.clone(),
                "example.com".to_string(),
                22,
            );

            assert!(verifier.verify(&rsa_pk).await);
            let prompts = prompter.recorded_prompts().await;
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts[0].kind, HostKeyPromptKind::NewKeyType);
            assert_eq!(prompts[0].existing_fingerprints.len(), 1);
            // Both ed_pk2 and rsa_pk are now trusted
            assert_eq!(
                store.check("example.com", 22, &ed_pk2).unwrap(),
                CheckResult::Trusted
            );
            assert_eq!(
                store.check("example.com", 22, &rsa_pk).unwrap(),
                CheckResult::Trusted
            );
        }

        // 6. Store error means no prompt and take_failure is set
        {
            let bad_path = dir.path().join("corrupted_known_hosts");
            std::fs::write(&bad_path, [0xFF, 0xFE, 0xFD]).unwrap();
            let bad_store = Arc::new(KnownHostsStore::new(&bad_path));
            let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Trust]));
            let verifier =
                HostKeyVerifier::new(bad_store, prompter.clone(), "example.com".to_string(), 22);

            assert!(!verifier.verify(&ed_pk1).await);
            assert_eq!(
                prompter.recorded_prompts().await.len(),
                0,
                "No prompt on store error"
            );
            assert!(
                verifier.take_failure().is_some(),
                "take_failure must return the error"
            );
            assert!(
                verifier.take_failure().is_none(),
                "take_failure is cleared after reading"
            );
        }
    }

    #[test]
    fn test_preferred_algorithms_ordering_and_no_duplicates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = Arc::new(KnownHostsStore::new(&path));

        let ed_pk = gen_ed25519().clone_public_key().unwrap();
        let rsa_pk = gen_rsa().clone_public_key().unwrap();

        // 1. Unknown host: returns defaults with no duplicates
        let verifier = HostKeyVerifier::new(
            store.clone(),
            Arc::new(ScriptedPrompter::new(vec![])),
            "unknown.com".to_string(),
            22,
        );
        let algs = verifier.preferred_algorithms();
        assert_eq!(algs[0], key::ED25519);
        let mut deduped = algs.clone();
        deduped.dedup();
        assert_eq!(algs, deduped, "Must contain no duplicates");

        // 2. Host known by RSA: puts RSA algorithms FIRST
        store.add("rsa-only.com", 22, &rsa_pk).unwrap();
        let rsa_verifier = HostKeyVerifier::new(
            store.clone(),
            Arc::new(ScriptedPrompter::new(vec![])),
            "rsa-only.com".to_string(),
            22,
        );
        let rsa_algs = rsa_verifier.preferred_algorithms();
        assert_eq!(rsa_algs[0], key::RSA_SHA2_512);
        assert_eq!(rsa_algs[1], key::RSA_SHA2_256);
        let mut deduped = rsa_algs.clone();
        deduped.dedup();
        assert_eq!(rsa_algs, deduped, "Must contain no duplicates");

        // 3. Host known by Ed25519: puts ED25519 first
        store.add("ed-only.com", 22, &ed_pk).unwrap();
        let ed_verifier = HostKeyVerifier::new(
            store.clone(),
            Arc::new(ScriptedPrompter::new(vec![])),
            "ed-only.com".to_string(),
            22,
        );
        let ed_algs = ed_verifier.preferred_algorithms();
        assert_eq!(ed_algs[0], key::ED25519);
        let mut deduped = ed_algs.clone();
        deduped.dedup();
        assert_eq!(ed_algs, deduped, "Must contain no duplicates");
    }

    // ── In-Process Duplex Stream Handshake Tests ──────────────────────────────

    struct TestServerHandler;

    impl russh::server::Handler for TestServerHandler {
        type Error = russh::Error;
    }

    struct TestClientHandler {
        verifier: Arc<HostKeyVerifier>,
    }

    #[async_trait]
    impl russh::client::Handler for TestClientHandler {
        type Error = russh::Error;

        async fn check_server_key(
            &mut self,
            server_public_key: &PublicKey,
        ) -> Result<bool, Self::Error> {
            let ok = self.verifier.verify(server_public_key).await;
            Ok(ok)
        }
    }

    async fn run_handshake(
        server_keys: Vec<russh_keys::key::KeyPair>,
        verifier: Arc<HostKeyVerifier>,
    ) -> Result<russh::client::Handle<TestClientHandler>, russh::Error> {
        let (client_stream, server_stream) = tokio::io::duplex(8192);

        let server_config = Arc::new(russh::server::Config {
            keys: server_keys,
            ..Default::default()
        });

        tokio::spawn(async move {
            let _ =
                russh::server::run_stream(server_config, server_stream, TestServerHandler).await;
        });

        let mut client_config = russh::client::Config::default();
        client_config.preferred.key = std::borrow::Cow::Owned(verifier.preferred_algorithms());
        let client_config = Arc::new(client_config);

        let handler = TestClientHandler { verifier };
        russh::client::connect_stream(client_config, client_stream, handler).await
    }

    #[tokio::test]
    async fn test_duplex_handshake_first_connect_prompts_reconnect_silent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = Arc::new(KnownHostsStore::new(&path));
        let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Trust]));
        let verifier = Arc::new(HostKeyVerifier::new(
            store.clone(),
            prompter.clone(),
            "in-process.local".to_string(),
            22,
        ));

        let server_kp = gen_ed25519();

        // 1. First connect prompts and succeeds
        let res1 = run_handshake(vec![server_kp.clone()], verifier.clone()).await;
        assert!(res1.is_ok(), "First handshake must succeed");
        assert_eq!(prompter.recorded_prompts().await.len(), 1);

        // 2. Reconnect is silent (0 additional prompts)
        let res2 = run_handshake(vec![server_kp.clone()], verifier.clone()).await;
        assert!(res2.is_ok(), "Reconnect must succeed");
        assert_eq!(
            prompter.recorded_prompts().await.len(),
            1,
            "Reconnect must be silent without prompting"
        );
    }

    #[tokio::test]
    async fn test_duplex_handshake_swapped_key_refused_on_reject() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = Arc::new(KnownHostsStore::new(&path));

        let server_kp1 = gen_ed25519();
        let server_kp2 = gen_ed25519(); // Swapped key!

        // Pre-trust key 1
        store
            .add(
                "in-process.local",
                22,
                &server_kp1.clone_public_key().unwrap(),
            )
            .unwrap();

        // User will REJECT changed key
        let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Reject]));
        let verifier = Arc::new(HostKeyVerifier::new(
            store.clone(),
            prompter.clone(),
            "in-process.local".to_string(),
            22,
        ));

        let res = run_handshake(vec![server_kp2.clone()], verifier.clone()).await;
        match res {
            Err(russh::Error::UnknownKey) => {
                // Exactly expected!
            }
            other => panic!(
                "Expected russh::Error::UnknownKey on rejection, got {:?}",
                other.err()
            ),
        }

        let prompts = prompter.recorded_prompts().await;
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].kind, HostKeyPromptKind::Changed);

        // Original key is still in store
        assert_eq!(
            store
                .check(
                    "in-process.local",
                    22,
                    &server_kp1.clone_public_key().unwrap()
                )
                .unwrap(),
            CheckResult::Trusted
        );
    }

    #[tokio::test]
    async fn test_duplex_handshake_changed_key_accepted_after_explicit_trust() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = Arc::new(KnownHostsStore::new(&path));

        let server_kp1 = gen_ed25519();
        let server_kp2 = gen_ed25519();

        store
            .add(
                "in-process.local",
                22,
                &server_kp1.clone_public_key().unwrap(),
            )
            .unwrap();

        // User explicitly TRUSTS the changed key
        let prompter = Arc::new(ScriptedPrompter::new(vec![HostKeyDecision::Trust]));
        let verifier = Arc::new(HostKeyVerifier::new(
            store.clone(),
            prompter.clone(),
            "in-process.local".to_string(),
            22,
        ));

        let res = run_handshake(vec![server_kp2.clone()], verifier.clone()).await;
        assert!(
            res.is_ok(),
            "Handshake with trusted changed key must succeed"
        );

        let prompts = prompter.recorded_prompts().await;
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].kind, HostKeyPromptKind::Changed);

        // Key 2 is now trusted
        assert_eq!(
            store
                .check(
                    "in-process.local",
                    22,
                    &server_kp2.clone_public_key().unwrap()
                )
                .unwrap(),
            CheckResult::Trusted
        );
    }

    #[tokio::test]
    async fn test_duplex_handshake_known_rsa_server_with_ed25519_no_prompt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let store = Arc::new(KnownHostsStore::new(&path));

        let rsa_kp = gen_rsa();
        let ed_kp = gen_ed25519();

        // Host is known by RSA
        store
            .add("in-process.local", 22, &rsa_kp.clone_public_key().unwrap())
            .unwrap();

        // Server presents BOTH ed25519 and rsa
        let server_keys = vec![ed_kp.clone(), rsa_kp.clone()];

        // Prompter has 0 answers scripted; if ask() is called, it would panic or reject
        let prompter = Arc::new(ScriptedPrompter::new(vec![]));
        let verifier = Arc::new(HostKeyVerifier::new(
            store.clone(),
            prompter.clone(),
            "in-process.local".to_string(),
            22,
        ));

        // Connect must negotiate RSA because of known-first ordering and succeed with 0 prompts!
        let res = run_handshake(server_keys, verifier.clone()).await;
        assert!(
            res.is_ok(),
            "Handshake with known RSA algorithm must succeed without prompting: {:?}",
            res.err()
        );
        assert_eq!(
            prompter.recorded_prompts().await.len(),
            0,
            "Must NOT prompt on reconnect for known RSA host"
        );
    }
}
