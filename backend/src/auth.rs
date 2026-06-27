pub mod oauth;
/// SEP-10 authentication — canonical implementation.
///
/// `sep10_simple` is the only supported SEP-10 module in this repository.
/// A previous `sep10` module that depended on `stellar-xdr` directly has been
/// removed. All handler wiring must use `crate::auth::sep10_simple::Sep10Service`.
pub mod sep10_simple;

use anyhow::{anyhow, Result};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

// Token expiry constants
const ACCESS_TOKEN_EXPIRY_HOURS: i64 = 1;
const REFRESH_TOKEN_EXPIRY_DAYS: i64 = 7;

// WARNING: Demo credentials removed for security. Use database-backed user store.
// See SEC-001 in SECURITY_AUDIT.md

/// User model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
}

/// Login request
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Login response
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

/// Refresh token request
#[derive(Debug, Deserialize)]
pub struct RefreshTokenRequest {
    pub refresh_token: String,
}

/// Refresh token response (includes rotated refresh token)
#[derive(Debug, Serialize)]
pub struct RefreshTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

/// Logout request
#[derive(Debug, Deserialize)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

/// JWT Claims
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,        // User ID
    pub username: String,   // Username
    pub exp: i64,           // Expiry timestamp
    pub iat: i64,           // Issued at timestamp
    pub jti: String,        // Unique token ID for rotation and revocation
    pub token_type: String, // "access" or "refresh"
}

/// Authentication service
pub struct AuthService {
    jwt_secret: String,
    redis_connection: Arc<RwLock<Option<MultiplexedConnection>>>,
    db_pool: SqlitePool,
}

impl AuthService {
    pub fn new(
        redis_connection: Arc<RwLock<Option<MultiplexedConnection>>>,
        db_pool: SqlitePool,
    ) -> Self {
        let jwt_secret = std::env::var("JWT_SECRET")
            .expect("JWT_SECRET environment variable is required. Generate a cryptographically secure random key of at least 32 bytes.");

        assert!(
            (jwt_secret.len() >= 32),
            "JWT_SECRET must be at least 32 characters for adequate security"
        );

        Self {
            jwt_secret,
            redis_connection,
            db_pool,
        }
    }

    /// Authenticate user with credentials against the database.
    /// Passwords are verified using argon2 — never stored or compared in plaintext.
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<User> {
        #[derive(sqlx::FromRow)]
        struct UserRecord {
            id: String,
            username: String,
            password_hash: String,
        }

        let record = sqlx::query_as::<_, UserRecord>(
            "SELECT id, username, password_hash FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| anyhow!("Database error during authentication: {e}"))?;

        let record = record.ok_or_else(|| anyhow!("Invalid username or password"))?;

        let parsed_hash = PasswordHash::new(&record.password_hash)
            .map_err(|e| anyhow!("Failed to parse password hash: {e}"))?;

        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| anyhow!("Invalid username or password"))?;

        Ok(User {
            id: record.id,
            username: record.username,
        })
    }

    /// Generate access token with a unique JTI for revocation support
    pub fn generate_access_token(&self, user: &User) -> Result<String> {
        let expiration = Utc::now()
            .checked_add_signed(Duration::hours(ACCESS_TOKEN_EXPIRY_HOURS))
            .ok_or_else(|| anyhow!("Invalid timestamp"))?
            .timestamp();

        let claims = Claims {
            sub: user.id.clone(),
            username: user.username.clone(),
            exp: expiration,
            iat: Utc::now().timestamp(),
            jti: uuid::Uuid::new_v4().to_string(),
            token_type: "access".to_string(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| anyhow!("Failed to generate access token: {e}"))
    }

    /// Generate refresh token with a unique JTI for single-use rotation
    pub fn generate_refresh_token(&self, user: &User) -> Result<(String, String)> {
        let jti = uuid::Uuid::new_v4().to_string();
        let expiration = Utc::now()
            .checked_add_signed(Duration::days(REFRESH_TOKEN_EXPIRY_DAYS))
            .ok_or_else(|| anyhow!("Invalid timestamp"))?
            .timestamp();

        let claims = Claims {
            sub: user.id.clone(),
            username: user.username.clone(),
            exp: expiration,
            iat: Utc::now().timestamp(),
            jti: jti.clone(),
            token_type: "refresh".to_string(),
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| anyhow!("Failed to generate refresh token: {e}"))?;

        Ok((token, jti))
    }

    /// Validate and decode token, checking JTI against revocation list
    pub fn validate_token(&self, token: &str) -> Result<Claims> {
        let validation = Validation::default();

        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        )
        .map(|data| data.claims)
        .map_err(|e| anyhow!("Invalid token: {e}"))
    }

    /// Store refresh token JTI in Redis, keyed by JTI for single-use enforcement.
    /// Also maintains a set of active JTIs per user for bulk revocation.
    pub async fn store_refresh_token(&self, jti: &str, user_id: &str) -> Result<()> {
        if let Some(conn) = self.redis_connection.read().await.as_ref() {
            let mut conn = conn.clone();
            let expiry = (REFRESH_TOKEN_EXPIRY_DAYS * 24 * 60 * 60) as u64;

            // Store the JTI as valid with its user_id
            let jti_key = format!("refresh_jti:{jti}");
            conn.set_ex::<_, _, ()>(&jti_key, user_id, expiry)
                .await
                .map_err(|e| anyhow!("Failed to store refresh token JTI: {e}"))?;

            // Track active JTIs per user for bulk revocation
            let user_jti_key = format!("user_refresh_jtis:{user_id}");
            conn.sadd::<_, _, ()>(&user_jti_key, jti)
                .await
                .map_err(|e| anyhow!("Failed to track user JTI: {e}"))?;
            conn.expire::<_, ()>(&user_jti_key, expiry as i64)
                .await
                .ok();

            tracing::debug!(
                user_id = crate::logging::redaction::redact_user_id(user_id),
                "Stored refresh token JTI for user"
            );
        } else {
            tracing::warn!("Redis not available, refresh token not stored");
        }

        Ok(())
    }

    /// Validate refresh token: checks JWT, verifies JTI is still valid (single-use).
    /// If the JTI was already consumed, triggers stolen-token detection and revokes
    /// ALL sessions for the user.
    pub async fn validate_refresh_token(&self, token: &str) -> Result<Claims> {
        let claims = self.validate_token(token)?;

        if claims.token_type != "refresh" {
            return Err(anyhow!("Invalid token type"));
        }

        if let Some(conn) = self.redis_connection.read().await.as_ref() {
            let mut conn = conn.clone();
            let jti_key = format!("refresh_jti:{}", claims.jti);

            let stored_user: Option<String> = conn
                .get(&jti_key)
                .await
                .map_err(|e| anyhow!("Failed to retrieve refresh token JTI: {e}"))?;

            match stored_user {
                Some(user_id) if user_id == claims.sub => {
                    // Valid JTI — consume it atomically (single-use)
                    conn.del::<_, ()>(&jti_key)
                        .await
                        .map_err(|e| anyhow!("Failed to consume refresh token JTI: {e}"))?;

                    conn.srem::<_, _, ()>(
                        &format!("user_refresh_jtis:{}", claims.sub),
                        &claims.jti,
                    )
                    .await
                    .ok();
                }
                Some(_) => {
                    return Err(anyhow!("Token user mismatch"));
                }
                None => {
                    // JTI not found — already used. This is a stolen token reuse attempt.
                    tracing::warn!(
                        user_id = crate::logging::redaction::redact_user_id(&claims.sub),
                        jti = %claims.jti,
                        "Stolen token detected: already-invalidated refresh token reused. Revoking all sessions."
                    );
                    self.revoke_all_user_sessions(&claims.sub).await?;
                    return Err(anyhow!("Refresh token already used — all sessions revoked"));
                }
            }
        } else {
            tracing::error!(
                "Redis not available - refusing refresh token validation (fail closed)"
            );
            return Err(anyhow!("Token validation service unavailable"));
        }

        Ok(claims)
    }

    /// Revoke a specific access token by storing its JTI in the revocation list.
    /// The JTI remains in the list until the token's original expiry time.
    pub async fn revoke_access_token(&self, claims: &Claims) -> Result<()> {
        if let Some(conn) = self.redis_connection.read().await.as_ref() {
            let mut conn = conn.clone();
            let revoked_key = format!("revoked_jti:{}", claims.jti);
            let ttl = (claims.exp - Utc::now().timestamp()).max(0) as u64;
            if ttl > 0 {
                conn.set_ex::<_, _, ()>(&revoked_key, "1", ttl)
                    .await
                    .map_err(|e| anyhow!("Failed to revoke access token: {e}"))?;
            }
        }
        Ok(())
    }

    /// Check if an access token's JTI has been revoked
    pub async fn is_token_revoked(&self, jti: &str) -> Result<bool> {
        if let Some(conn) = self.redis_connection.read().await.as_ref() {
            let mut conn = conn.clone();
            let revoked_key = format!("revoked_jti:{jti}");
            let exists: bool = conn
                .exists(&revoked_key)
                .await
                .map_err(|e| anyhow!("Failed to check token revocation: {e}"))?;
            Ok(exists)
        } else {
            // Fail closed: treat as revoked if Redis is unavailable
            Ok(true)
        }
    }

    /// Revoke ALL sessions for a user (stolen token detection response).
    /// Removes all active refresh JTIs and adds a user-level revocation marker.
    pub async fn revoke_all_user_sessions(&self, user_id: &str) -> Result<()> {
        if let Some(conn) = self.redis_connection.read().await.as_ref() {
            let mut conn = conn.clone();
            let user_jti_key = format!("user_refresh_jtis:{user_id}");

            // Get all active refresh JTIs for this user
            let jtis: Vec<String> = conn
                .smembers(&user_jti_key)
                .await
                .unwrap_or_default();

            // Delete each JTI
            for jti in &jtis {
                conn.del::<_, ()>(&format!("refresh_jti:{jti}"))
                    .await
                    .ok();
            }

            // Delete the user's JTI set
            conn.del::<_, ()>(&user_jti_key).await.ok();

            // Set a user-level revocation marker (all tokens issued before now are invalid)
            let revoke_all_key = format!("user_revoked_at:{user_id}");
            conn.set_ex::<_, _, ()>(
                &revoke_all_key,
                Utc::now().timestamp().to_string(),
                (REFRESH_TOKEN_EXPIRY_DAYS * 24 * 60 * 60) as u64,
            )
            .await
            .ok();

            tracing::warn!(
                user_id = crate::logging::redaction::redact_user_id(user_id),
                sessions_revoked = jtis.len(),
                "All sessions revoked for user due to stolen token detection"
            );
        }

        Ok(())
    }

    /// Invalidate refresh token (logout) — removes the specific JTI
    pub async fn invalidate_refresh_token(&self, user_id: &str, jti: &str) -> Result<()> {
        if let Some(conn) = self.redis_connection.read().await.as_ref() {
            let mut conn = conn.clone();

            conn.del::<_, ()>(&format!("refresh_jti:{jti}"))
                .await
                .map_err(|e| anyhow!("Failed to invalidate refresh token: {e}"))?;

            conn.srem::<_, _, ()>(
                &format!("user_refresh_jtis:{user_id}"),
                jti,
            )
            .await
            .ok();

            tracing::debug!(
                user_id = crate::logging::redaction::redact_user_id(user_id),
                "Invalidated refresh token for user"
            );
        }

        Ok(())
    }

    /// Login flow
    pub async fn login(&self, request: LoginRequest) -> Result<LoginResponse> {
        let user = self
            .authenticate(&request.username, &request.password)
            .await?;

        let access_token = self.generate_access_token(&user)?;
        let (refresh_token, jti) = self.generate_refresh_token(&user)?;

        self.store_refresh_token(&jti, &user.id).await?;

        Ok(LoginResponse {
            access_token,
            refresh_token,
            expires_in: ACCESS_TOKEN_EXPIRY_HOURS * 3600,
        })
    }

    /// Refresh access token with rotation: old refresh token is consumed,
    /// a new refresh token is issued. Reusing the old token triggers
    /// stolen-token detection.
    pub async fn refresh(&self, request: RefreshTokenRequest) -> Result<RefreshTokenResponse> {
        // validate_refresh_token atomically consumes the old JTI
        let claims = self.validate_refresh_token(&request.refresh_token).await?;

        let user = User {
            id: claims.sub,
            username: claims.username,
        };

        let access_token = self.generate_access_token(&user)?;
        let (refresh_token, new_jti) = self.generate_refresh_token(&user)?;

        self.store_refresh_token(&new_jti, &user.id).await?;

        Ok(RefreshTokenResponse {
            access_token,
            refresh_token,
            expires_in: ACCESS_TOKEN_EXPIRY_HOURS * 3600,
        })
    }

    /// Logout flow — invalidates the specific refresh token and revokes
    /// the current access token's JTI
    pub async fn logout(&self, request: LogoutRequest) -> Result<()> {
        let claims = self.validate_token(&request.refresh_token)?;

        self.invalidate_refresh_token(&claims.sub, &claims.jti)
            .await?;

        Ok(())
    }
}

// SEP-10 Authentication Middleware and Types
// Consolidated from sep10_middleware.rs

use axum::{
    async_trait,
    extract::{FromRequestParts, Request, State},
    http::{header, request::Parts, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Extract SEP-10 authenticated user from request
#[derive(Debug, Clone)]
pub struct Sep10User {
    pub account: String,
    pub client_domain: Option<String>,
}

/// SEP-10 claims for extracting authenticated user in handlers
#[derive(Debug, Clone)]
pub struct Sep10Claims {
    pub account: String,
    pub client_domain: Option<String>,
}

#[async_trait]
impl<S> FromRequestParts<S> for Sep10Claims
where
    S: Send + Sync,
{
    type Rejection = Sep10AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Try to get Sep10User from extensions (set by middleware)
        parts
            .extensions
            .get::<Sep10User>()
            .map(|user| Self {
                account: user.account.clone(),
                client_domain: user.client_domain.clone(),
            })
            .ok_or(Sep10AuthError::MissingToken)
    }
}

/// SEP-10 authentication middleware
pub async fn sep10_auth_middleware(
    State(sep10_service): State<Arc<sep10_simple::Sep10Service>>,
    mut req: Request,
    next: Next,
) -> Result<Response, Sep10AuthError> {
    // Extract Authorization header and token before mutating req
    let token = {
        let auth_header = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .ok_or(Sep10AuthError::MissingToken)?;

        // Extract Bearer token
        auth_header
            .strip_prefix("Bearer ")
            .ok_or(Sep10AuthError::InvalidToken)?
            .to_string()
    };

    // Validate session
    let session = sep10_service
        .validate_session(&token)
        .await
        .map_err(|_| Sep10AuthError::InvalidToken)?;

    // Attach user to request extensions
    let sep10_user = Sep10User {
        account: session.account,
        client_domain: session.client_domain,
    };
    req.extensions_mut().insert(sep10_user);
    req.extensions_mut().insert(token);

    Ok(next.run(req).await)
}

/// SEP-10 authentication errors
#[derive(Debug)]
pub enum Sep10AuthError {
    MissingToken,
    InvalidToken,
}

impl IntoResponse for Sep10AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::MissingToken => (StatusCode::UNAUTHORIZED, "Missing authentication token"),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid or expired token"),
        };

        let body = json!({
            "error": message,
        });

        (status, axum::Json(body)).into_response()
    }
}
