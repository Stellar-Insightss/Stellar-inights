use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::fmt;
use uuid::Uuid;

/// Request ID wrapper for storing in request extensions
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

impl RequestId {
    /// Generate a new random request ID
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Get the request ID as a string slice
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for RequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Correlation context carried through the request lifecycle.
/// Every log line emitted while this is in scope will include these fields.
#[derive(Clone, Debug)]
pub struct CorrelationContext {
    pub trace_id: String,
    pub service_name: String,
    pub user_id: Option<String>,
}

impl CorrelationContext {
    #[must_use]
    pub fn new(trace_id: String, service_name: String) -> Self {
        Self {
            trace_id,
            service_name,
            user_id: None,
        }
    }

    #[must_use]
    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }
}

/// Middleware to add request ID tracking
///
/// This middleware:
/// - Generates a unique request ID for each request
/// - Adds it to request extensions for use in handlers
/// - Builds a CorrelationContext with trace_id and service_name
/// - Includes it in response headers as X-Request-ID
/// - Logs the request ID for tracing
pub async fn request_id_middleware(mut req: Request<Body>, next: Next) -> Response {
    let request_id = if let Some(existing_id) = req.headers().get("X-Request-ID") {
        existing_id.to_str().ok().map_or_else(
            || Uuid::new_v4().to_string(),
            std::string::ToString::to_string,
        )
    } else {
        Uuid::new_v4().to_string()
    };

    let service_name = std::env::var("SERVICE_NAME")
        .unwrap_or_else(|_| "stellar-insights-backend".to_string());

    let correlation = CorrelationContext::new(request_id.clone(), service_name.clone());

    req.extensions_mut().insert(RequestId(request_id.clone()));
    req.extensions_mut().insert(correlation);

    let method = req.method().clone();
    let uri = req.uri().clone();
    tracing::info!(
        trace_id = %request_id,
        service_name = %service_name,
        method = %method,
        uri = %uri,
        "Incoming request"
    );

    let response = next.run(req).await;

    let (mut parts, body) = response.into_parts();

    if let Ok(header_value) = HeaderValue::from_str(&request_id) {
        parts.headers.insert("X-Request-ID", header_value);
    }

    Response::from_parts(parts, body)
}

/// Extract request ID from request extensions
///
/// Returns None if no request ID is found (shouldn't happen if middleware is applied)
pub fn get_request_id(req: &Request<Body>) -> Option<String> {
    req.extensions().get::<RequestId>().map(|id| id.0.clone())
}

/// Extract correlation context from request extensions
pub fn get_correlation_context(req: &Request<Body>) -> Option<CorrelationContext> {
    req.extensions().get::<CorrelationContext>().cloned()
}

/// Error response with request ID
#[must_use]
pub fn error_with_request_id(
    status: StatusCode,
    message: String,
    request_id: Option<String>,
) -> Response {
    let body = if let Some(id) = request_id {
        serde_json::json!({
            "error": message,
            "request_id": id
        })
    } else {
        serde_json::json!({
            "error": message
        })
    };

    (status, axum::Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware,
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    #[test]
    fn test_request_id_creation() {
        let id1 = RequestId::new();
        let id2 = RequestId::new();

        // IDs should be different
        assert_ne!(id1.0, id2.0);

        // IDs should be valid UUIDs (36 characters with hyphens)
        assert_eq!(id1.0.len(), 36);
        assert_eq!(id2.0.len(), 36);
    }

    #[test]
    fn test_request_id_display() {
        let id = RequestId::new();
        let display = format!("{}", id);
        assert_eq!(display, id.0);
    }

    #[test]
    fn test_request_id_as_str() {
        let id = RequestId::new();
        assert_eq!(id.as_str(), &id.0);
    }

    #[test]
    fn test_request_id_clone() {
        let id1 = RequestId::new();
        let id2 = id1.clone();
        assert_eq!(id1.0, id2.0);
    }

    #[test]
    fn test_request_id_default() {
        let id = RequestId::default();
        assert_eq!(id.0.len(), 36);
    }

    #[tokio::test]
    async fn middleware_sets_response_request_id() {
        let app = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(request_id_middleware));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("X-Request-ID").is_some());
    }

    #[tokio::test]
    async fn middleware_preserves_upstream_request_id() {
        let app = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn(request_id_middleware));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header("X-Request-ID", "upstream-request-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("X-Request-ID")
                .and_then(|h| h.to_str().ok()),
            Some("upstream-request-id")
        );
    }
}
