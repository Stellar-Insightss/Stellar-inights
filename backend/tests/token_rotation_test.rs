use stellar_insights_backend::auth::{AuthService, Claims, LoginRequest, RefreshTokenRequest};
use std::sync::Arc;
use tokio::sync::RwLock;

fn setup_env() {
    if std::env::var("JWT_SECRET").is_err() {
        std::env::set_var(
            "JWT_SECRET",
            "test_jwt_secret_key_that_is_long_enough_for_tests_32",
        );
    }
}

fn create_auth_service_no_redis() -> AuthService {
    setup_env();
    let redis = Arc::new(RwLock::new(None));
    let pool = futures::executor::block_on(sqlx::SqlitePool::connect("sqlite::memory:")).unwrap();
    AuthService::new(redis, pool)
}

#[test]
fn test_claims_include_jti() {
    let service = create_auth_service_no_redis();
    let user = stellar_insights_backend::auth::User {
        id: "user-1".to_string(),
        username: "testuser".to_string(),
    };

    let token = service.generate_access_token(&user).unwrap();
    let claims = service.validate_token(&token).unwrap();

    assert!(!claims.jti.is_empty(), "JTI must be present");
    assert_eq!(claims.token_type, "access");
    assert_eq!(claims.sub, "user-1");
}

#[test]
fn test_refresh_token_returns_jti() {
    let service = create_auth_service_no_redis();
    let user = stellar_insights_backend::auth::User {
        id: "user-1".to_string(),
        username: "testuser".to_string(),
    };

    let (token, jti) = service.generate_refresh_token(&user).unwrap();
    assert!(!jti.is_empty(), "JTI must be returned");

    let claims = service.validate_token(&token).unwrap();
    assert_eq!(claims.jti, jti, "JTI in token must match returned JTI");
    assert_eq!(claims.token_type, "refresh");
}

#[test]
fn test_each_token_has_unique_jti() {
    let service = create_auth_service_no_redis();
    let user = stellar_insights_backend::auth::User {
        id: "user-1".to_string(),
        username: "testuser".to_string(),
    };

    let token1 = service.generate_access_token(&user).unwrap();
    let token2 = service.generate_access_token(&user).unwrap();
    let claims1 = service.validate_token(&token1).unwrap();
    let claims2 = service.validate_token(&token2).unwrap();

    assert_ne!(claims1.jti, claims2.jti, "Each token must have a unique JTI");
}
