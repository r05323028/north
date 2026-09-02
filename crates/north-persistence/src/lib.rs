//! PostgreSQL persistence for North server state.
//!
//! Database rows and their domain mappings live here. Hosts call this crate's
//! operations instead of hand-rolling SQL.

use north_domain::role::Role;
use rand::{rng, Rng};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
pub use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{error::Error, fmt};
use subtle::ConstantTimeEq;

mod clarification;
mod conversations;
mod daemon;
mod delivery;
mod readiness;
mod repositories;
mod requirements;

pub use clarification::{
    ClarificationActivity, ClarificationCommandResult, ClarificationError, ClarificationEvent,
    ClarificationEventError, ClarificationPhase, ClarificationRun, ClarificationStartInput,
    ClarificationStartResult, ClarificationStatus, ReadinessView, MAX_CONTEXT_BYTES,
    MAX_CONTEXT_MESSAGES,
};
pub use conversations::{
    ConversationError, ConversationPage, ConversationRecord, MessageKind, MessageRecord,
};
pub use daemon::{
    AuthenticatedDaemon, DaemonRegistration, DaemonSessionState, DaemonSetupClaim,
    DaemonSetupPreview, DaemonSetupRequest, DaemonSetupState, PinnedCommand,
    DAEMON_SETUP_CLEANUP_BATCH_SIZE, DAEMON_SETUP_RETENTION_SECONDS, DAEMON_SETUP_TTL_SECONDS,
};
pub use delivery::{
    canonical_payload_digest, EventReceipt, EventReceiptOutcome, EventReceiptRequest,
};
pub use readiness::{
    AssessmentOutcome, ReadinessAssessmentRecord, ReadinessAssessmentResult, ReadinessError,
};
pub use repositories::{repository_metadata, RepositoryRecord};
pub use requirements::{
    RequirementError, RequirementListQuery, RequirementRecord, RequirementSort,
    RequirementTransition,
};

pub use sqlx::postgres::PgPoolOptions as PoolOptions;
pub use sqlx::PgPool as DatabasePool;

/// Verification codes remain usable for ten minutes.
pub const VERIFICATION_CODE_TTL_SECONDS: i64 = 10 * 60;
/// Failed verification invalidates one issued code after this many attempts.
pub const VERIFICATION_CODE_MAX_ATTEMPTS: i32 = 5;
/// A single email cannot request more than one code per minute.
pub const CODE_REQUEST_COOLDOWN_SECONDS: i64 = 60;
/// Sessions remain valid for thirty days unless explicitly invalidated.
pub const SESSION_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

/// Compile-time embedded migrations applied by server startup.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub type MigrationError = sqlx::migrate::MigrateError;

pub async fn run_migrations(pool: &PgPool) -> Result<(), MigrationError> {
    MIGRATOR.run(pool).await
}

#[derive(Debug)]
pub enum PersistenceError {
    Database(sqlx::Error),
    InvalidCode,
    InvalidRole(String),
    RateLimited,
    InvalidDaemonCredential,
    RevokedDaemon,
    DaemonNotFound,
    SetupNotFound,
    SetupExpired,
    SetupAlreadyApproved,
    SetupAlreadyClaimed,
    InvalidSetup,
    NoEligibleDaemon,
    InvalidCapabilities,
    InvalidCommandPayload,
    InvalidSessionState,
    SessionRequirementMismatch,
    InvalidRepository(north_domain::repository::RepositoryError),
    RepositoryNotFound,
    RepositoryNameConflict,
    RepositoryUrlImmutable,
    ProtocolIntegrity(String),
    EventSequenceGap { expected: u64, received: u64 },
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::InvalidCode => f.write_str("invalid or expired verification code"),
            Self::InvalidRole(role) => write!(f, "unknown persisted role: {role}"),
            Self::RateLimited => f.write_str("verification code request rate limited"),
            Self::InvalidDaemonCredential => f.write_str("invalid daemon credential"),
            Self::RevokedDaemon => f.write_str("daemon credential revoked"),
            Self::DaemonNotFound => f.write_str("daemon not found"),
            Self::SetupNotFound => f.write_str("daemon setup request not found"),
            Self::SetupExpired => f.write_str("daemon setup request expired"),
            Self::SetupAlreadyApproved => f.write_str("daemon setup request already approved"),
            Self::SetupAlreadyClaimed => f.write_str("daemon setup request already claimed"),
            Self::InvalidSetup => f.write_str("invalid daemon setup request"),
            Self::NoEligibleDaemon => f.write_str("no eligible daemon connected"),
            Self::InvalidCapabilities => f.write_str("invalid daemon capabilities"),
            Self::InvalidCommandPayload => f.write_str("invalid durable command payload"),
            Self::InvalidSessionState => f.write_str("invalid durable session state"),
            Self::SessionRequirementMismatch => {
                f.write_str("session is bound to another requirement")
            }
            Self::InvalidRepository(error) => write!(f, "invalid repository: {error:?}"),
            Self::RepositoryNotFound => f.write_str("repository not found"),
            Self::RepositoryNameConflict => f.write_str("repository name already exists"),
            Self::RepositoryUrlImmutable => f.write_str("repository URL is immutable"),
            Self::ProtocolIntegrity(reason) => write!(f, "protocol integrity error: {reason}"),
            Self::EventSequenceGap { expected, received } => {
                write!(
                    f,
                    "event sequence gap: expected {expected}, received {received}"
                )
            }
        }
    }
}

impl Error for PersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::InvalidCode
            | Self::InvalidRole(_)
            | Self::RateLimited
            | Self::InvalidDaemonCredential
            | Self::RevokedDaemon
            | Self::DaemonNotFound
            | Self::SetupNotFound
            | Self::SetupExpired
            | Self::SetupAlreadyApproved
            | Self::SetupAlreadyClaimed
            | Self::InvalidSetup
            | Self::NoEligibleDaemon
            | Self::InvalidCapabilities
            | Self::InvalidCommandPayload
            | Self::InvalidSessionState
            | Self::SessionRequirementMismatch
            | Self::InvalidRepository(_)
            | Self::RepositoryNotFound
            | Self::RepositoryNameConflict
            | Self::RepositoryUrlImmutable
            | Self::ProtocolIntegrity(_)
            | Self::EventSequenceGap { .. } => None,
        }
    }
}

impl From<sqlx::Error> for PersistenceError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// User value returned after row-to-domain role conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    pub id: String,
    pub email: String,
    pub role: Role,
}

/// Raw session token is returned only to the HTTP adapter for its cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedSession {
    pub user: UserRecord,
    pub token: String,
}

#[derive(Clone)]
pub struct AuthStore {
    pool: PgPool,
}

impl AuthStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Supersede any active code and insert one hashed code in one transaction.
    pub async fn issue_code(&self, email: &str, code: &str) -> Result<(), PersistenceError> {
        let code_hash = hash_secret(code.as_bytes());
        let mut transaction = self.pool.begin().await?;

        // Serialize requests for the same email before checking the cooldown.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(email)
            .execute(&mut *transaction)
            .await?;

        let recently_requested: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1
                FROM verification_codes
                WHERE email = $1
                  AND created_at > CURRENT_TIMESTAMP
                      - ($2::double precision * INTERVAL '1 second')
            )",
        )
        .bind(email)
        .bind(CODE_REQUEST_COOLDOWN_SECONDS)
        .fetch_one(&mut *transaction)
        .await?;
        if recently_requested {
            return Err(PersistenceError::RateLimited);
        }

        sqlx::query(
            "UPDATE verification_codes
             SET used_at = CURRENT_TIMESTAMP
             WHERE email = $1 AND used_at IS NULL",
        )
        .bind(email)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO verification_codes (email, code_hash, expires_at)
             VALUES ($1, $2, CURRENT_TIMESTAMP
                 + ($3::double precision * INTERVAL '1 second'))",
        )
        .bind(email)
        .bind(code_hash)
        .bind(VERIFICATION_CODE_TTL_SECONDS)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    /// Consume a valid code and create its session in one transaction.
    pub async fn verify_code(
        &self,
        email: &str,
        code: &str,
    ) -> Result<AuthenticatedSession, PersistenceError> {
        let candidate_hash = hash_secret(code.as_bytes());
        let mut transaction = self.pool.begin().await?;

        let Some(code_row) = sqlx::query_as::<_, VerificationCodeRow>(
            "SELECT id, code_hash, failed_attempts
             FROM verification_codes
             WHERE email = $1
               AND used_at IS NULL
               AND expires_at > CURRENT_TIMESTAMP
             ORDER BY created_at DESC
             LIMIT 1
             FOR UPDATE",
        )
        .bind(email)
        .fetch_optional(&mut *transaction)
        .await?
        else {
            return Err(PersistenceError::InvalidCode);
        };

        let matches = code_row
            .code_hash
            .as_slice()
            .ct_eq(candidate_hash.as_slice())
            .unwrap_u8()
            == 1;
        if !matches {
            let failed_attempts = code_row.failed_attempts.saturating_add(1);
            sqlx::query(
                "UPDATE verification_codes
                 SET failed_attempts = $2,
                     used_at = CASE WHEN $2 >= $3 THEN CURRENT_TIMESTAMP ELSE used_at END
                 WHERE id = $1 AND used_at IS NULL",
            )
            .bind(code_row.id)
            .bind(failed_attempts)
            .bind(VERIFICATION_CODE_MAX_ATTEMPTS)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Err(PersistenceError::InvalidCode);
        }

        let consumed = sqlx::query(
            "UPDATE verification_codes
             SET used_at = CURRENT_TIMESTAMP
             WHERE id = $1 AND used_at IS NULL",
        )
        .bind(code_row.id)
        .execute(&mut *transaction)
        .await?;
        if consumed.rows_affected() != 1 {
            return Err(PersistenceError::InvalidCode);
        }

        let user_id = random_hex(16);
        sqlx::query(
            "INSERT INTO users (id, email)
             VALUES ($1, $2)
             ON CONFLICT (email) DO NOTHING",
        )
        .bind(&user_id)
        .bind(email)
        .execute(&mut *transaction)
        .await?;

        let user_row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, role FROM users WHERE email = $1 FOR UPDATE",
        )
        .bind(email)
        .fetch_one(&mut *transaction)
        .await?;
        let mut user = user_row.into_domain()?;

        let owner_claimed = sqlx::query(
            "UPDATE instance_settings
             SET owner_user_id = $1
             WHERE id = 1 AND owner_user_id IS NULL",
        )
        .bind(&user.id)
        .execute(&mut *transaction)
        .await?
        .rows_affected()
            == 1;
        if owner_claimed {
            sqlx::query("UPDATE users SET role = 'Owner' WHERE id = $1")
                .bind(&user.id)
                .execute(&mut *transaction)
                .await?;
            user.role = Role::Owner;
        }

        let token = random_hex(32);
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, expires_at)
             VALUES ($1, $2, $3, CURRENT_TIMESTAMP
                 + ($4::double precision * INTERVAL '1 second'))",
        )
        .bind(random_hex(16))
        .bind(&user.id)
        .bind(hash_secret(token.as_bytes()))
        .bind(SESSION_TTL_SECONDS)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(AuthenticatedSession { user, token })
    }

    pub async fn user_for_session(
        &self,
        token: &str,
    ) -> Result<Option<UserRecord>, PersistenceError> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT users.id, users.email, users.role
             FROM sessions
             INNER JOIN users ON users.id = sessions.user_id
             WHERE sessions.token_hash = $1
               AND sessions.invalidated_at IS NULL
               AND sessions.expires_at > CURRENT_TIMESTAMP",
        )
        .bind(hash_secret(token.as_bytes()))
        .fetch_optional(&self.pool)
        .await?;
        row.map(UserRow::into_domain).transpose()
    }

    pub async fn list_users(&self) -> Result<Vec<UserRecord>, PersistenceError> {
        let rows = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, role FROM users ORDER BY email ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(UserRow::into_domain).collect()
    }

    pub async fn user_by_id(&self, user_id: &str) -> Result<Option<UserRecord>, PersistenceError> {
        let row = sqlx::query_as::<_, UserRow>("SELECT id, email, role FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(UserRow::into_domain).transpose()
    }

    pub async fn update_user_role(
        &self,
        user_id: &str,
        role: Role,
    ) -> Result<Option<UserRecord>, PersistenceError> {
        let row = sqlx::query_as::<_, UserRow>(
            "UPDATE users
             SET role = $2
             WHERE id = $1
             RETURNING id, email, role",
        )
        .bind(user_id)
        .bind(persisted_role(role))
        .fetch_optional(&self.pool)
        .await?;
        row.map(UserRow::into_domain).transpose()
    }

    pub async fn invalidate_session(&self, token: &str) -> Result<(), PersistenceError> {
        sqlx::query(
            "UPDATE sessions
             SET invalidated_at = CURRENT_TIMESTAMP
             WHERE token_hash = $1 AND invalidated_at IS NULL",
        )
        .bind(hash_secret(token.as_bytes()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(Debug, FromRow)]
struct VerificationCodeRow {
    id: i64,
    code_hash: Vec<u8>,
    failed_attempts: i32,
}

#[derive(Debug, FromRow)]
struct UserRow {
    id: String,
    email: String,
    role: String,
}

fn persisted_role(role: Role) -> &'static str {
    match role {
        Role::Owner => "Owner",
        Role::Admin => "Admin",
        Role::RequirementManager => "RequirementManager",
        Role::Requester => "Requester",
    }
}

impl UserRow {
    fn into_domain(self) -> Result<UserRecord, PersistenceError> {
        let role = match self.role.as_str() {
            "Owner" => Role::Owner,
            "Admin" => Role::Admin,
            "RequirementManager" => Role::RequirementManager,
            "Requester" => Role::Requester,
            role => return Err(PersistenceError::InvalidRole(role.to_string())),
        };
        Ok(UserRecord {
            id: self.id,
            email: self.email,
            role,
        })
    }
}

fn hash_secret(secret: &[u8]) -> Vec<u8> {
    Sha256::digest(secret).to_vec()
}

pub(crate) fn payload_digest(payload: &str) -> String {
    use md5::{Digest, Md5};
    let digest = Md5::digest(payload.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn command_identity_digest(payload: &str) -> String {
    let mut value = serde_json::from_str::<serde_json::Value>(payload)
        .unwrap_or_else(|_| serde_json::Value::String(payload.to_owned()));
    if let Some(command_payload) = value
        .get_mut("payload")
        .and_then(serde_json::Value::as_object_mut)
    {
        command_payload.remove("sent_at");
    }
    let canonical = serde_json::to_vec(&value).unwrap_or_default();
    use md5::{Digest, Md5};
    let digest = Md5::digest(canonical);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn command_identity_digest_valid(payload: &str, stored: &str) -> bool {
    command_identity_digest(payload) == stored || payload_digest(payload) == stored
}

pub(crate) fn command_identity_matches(existing: &str, candidate: &str, stored: &str) -> bool {
    if command_identity_digest(candidate) == stored {
        return true;
    }
    if payload_digest(existing) != stored {
        return false;
    }
    let Ok(mut existing) = serde_json::from_str::<serde_json::Value>(existing) else {
        return false;
    };
    let Ok(mut candidate) = serde_json::from_str::<serde_json::Value>(candidate) else {
        return false;
    };
    for value in [&mut existing, &mut candidate] {
        if let Some(command_payload) = value
            .get_mut("payload")
            .and_then(serde_json::Value::as_object_mut)
        {
            command_payload.remove("sent_at");
        }
    }
    existing == candidate
}

fn random_hex(byte_count: usize) -> String {
    let mut bytes = vec![0_u8; byte_count];
    rng().fill(bytes.as_mut_slice());
    bytes
        .into_iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    static DATABASE_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    async fn database_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        DATABASE_TEST_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await
    }

    #[test]
    fn persisted_roles_map_only_inside_persistence() {
        for (stored, expected) in [
            ("Owner", Role::Owner),
            ("Admin", Role::Admin),
            ("RequirementManager", Role::RequirementManager),
            ("Requester", Role::Requester),
        ] {
            assert_eq!(
                UserRow {
                    id: "id".into(),
                    email: "user@example.com".into(),
                    role: stored.into(),
                }
                .into_domain()
                .expect("known role")
                .role,
                expected
            );
            assert_eq!(persisted_role(expected), stored);
        }
    }

    #[test]
    fn secret_hashes_have_fixed_length() {
        assert_eq!(hash_secret(b"123456").len(), 32);
        assert_ne!(hash_secret(b"123456"), b"123456");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_first_verifications_yield_one_owner() {
        let Ok(database_url) = std::env::var("NORTH_TEST_DATABASE_URL") else {
            return;
        };
        let _database_test_guard = database_test_lock().await;
        let pool = PoolOptions::new()
            .max_connections(8)
            .connect(&database_url)
            .await
            .expect("connect test database");
        run_migrations(&pool).await.expect("run migrations");

        // NORTH_TEST_DATABASE_URL must point at an isolated test database.
        sqlx::query("DELETE FROM instance_settings")
            .execute(&pool)
            .await
            .expect("clear instance settings");
        for table in [
            "server_message_command_map",
            "server_command_tombstones",
            "server_event_dedupe",
            "server_command_outbox",
            "execution_sessions",
            "daemon_setup_requests",
            "daemon_registrations",
        ] {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&pool)
                .await
                .expect("clear daemon runtime rows");
        }
        sqlx::query("DELETE FROM users")
            .execute(&pool)
            .await
            .expect("clear users");
        sqlx::query("INSERT INTO instance_settings (id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("restore singleton settings");

        let store = AuthStore::new(pool.clone());
        store
            .issue_code("owner-a@example.com", "111111")
            .await
            .expect("issue first code");
        store
            .issue_code("owner-b@example.com", "222222")
            .await
            .expect("issue second code");

        let (first, second) = tokio::join!(
            store.verify_code("owner-a@example.com", "111111"),
            store.verify_code("owner-b@example.com", "222222"),
        );
        let first = first.expect("first verification");
        let second = second.expect("second verification");
        assert_eq!(
            [first.user.role, second.user.role]
                .into_iter()
                .filter(|role| *role == Role::Owner)
                .count(),
            1
        );
        assert_eq!(
            [first.user.role, second.user.role]
                .into_iter()
                .filter(|role| *role == Role::Requester)
                .count(),
            1
        );

        let owner_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'Owner'")
                .fetch_one(&pool)
                .await
                .expect("count owners");
        assert_eq!(owner_count, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn verification_attempts_are_bounded_transactionally() {
        let Ok(database_url) = std::env::var("NORTH_TEST_DATABASE_URL") else {
            return;
        };
        let _database_test_guard = database_test_lock().await;
        let pool = PoolOptions::new()
            .max_connections(8)
            .connect(&database_url)
            .await
            .expect("connect test database");
        run_migrations(&pool).await.expect("run migrations");
        let store = AuthStore::new(pool.clone());
        let email = format!("verification-attempts-{}@example.com", random_hex(8));
        store
            .issue_code(&email, "123456")
            .await
            .expect("issue verification code");

        assert!(matches!(
            store.verify_code(&email, "000000").await,
            Err(PersistenceError::InvalidCode)
        ));
        let failed_attempts: i32 = sqlx::query_scalar(
            "SELECT failed_attempts FROM verification_codes
             WHERE email = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(&email)
        .fetch_one(&pool)
        .await
        .expect("read failed attempts");
        assert_eq!(failed_attempts, 1);

        let mut attempts = Vec::new();
        for _ in 1..VERIFICATION_CODE_MAX_ATTEMPTS {
            let store = store.clone();
            let email = email.clone();
            attempts.push(tokio::spawn(async move {
                store.verify_code(&email, "000000").await
            }));
        }
        for attempt in attempts {
            assert!(matches!(
                attempt.await.expect("verification task"),
                Err(PersistenceError::InvalidCode)
            ));
        }

        let (failed_attempts, consumed): (i32, bool) = sqlx::query_as(
            "SELECT failed_attempts, used_at IS NOT NULL
             FROM verification_codes
             WHERE email = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(&email)
        .fetch_one(&pool)
        .await
        .expect("read consumed code");
        assert_eq!(failed_attempts, VERIFICATION_CODE_MAX_ATTEMPTS);
        assert!(consumed);
        assert!(matches!(
            store.verify_code(&email, "123456").await,
            Err(PersistenceError::InvalidCode)
        ));

        sqlx::query(
            "UPDATE verification_codes
             SET created_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes'
             WHERE email = $1",
        )
        .bind(&email)
        .execute(&pool)
        .await
        .expect("age previous verification code");
        store
            .issue_code(&email, "654321")
            .await
            .expect("issue fresh verification code");
        let session = store
            .verify_code(&email, "654321")
            .await
            .expect("verify fresh code");
        assert_eq!(session.user.email, email);
        let fresh_attempts: i32 = sqlx::query_scalar(
            "SELECT failed_attempts FROM verification_codes
             WHERE email = $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(&email)
        .fetch_one(&pool)
        .await
        .expect("read fresh attempt budget");
        assert_eq!(fresh_attempts, 0);

        let superseded_email = format!("verification-superseded-{}@example.com", random_hex(8));
        store
            .issue_code(&superseded_email, "777777")
            .await
            .expect("issue superseded code");
        sqlx::query(
            "UPDATE verification_codes\n             SET created_at = CURRENT_TIMESTAMP - INTERVAL '2 minutes'\n             WHERE email = $1",
        )
        .bind(&superseded_email)
        .execute(&pool)
        .await
        .expect("age superseded code");
        store
            .issue_code(&superseded_email, "888888")
            .await
            .expect("issue replacement code");
        assert!(matches!(
            store.verify_code(&superseded_email, "777777").await,
            Err(PersistenceError::InvalidCode)
        ));
        store
            .verify_code(&superseded_email, "888888")
            .await
            .expect("verify replacement code");
    }
}
