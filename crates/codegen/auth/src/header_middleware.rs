//! `reqwest-middleware` layer that stamps the current BYOK bearer once.

use std::sync::Arc;

use reqwest::{Request, Response, header::HeaderValue};
use reqwest_middleware::{Error, Middleware, Next};

use crate::AuthCredentialProvider;

pub struct AuthHeaderMiddleware {
    credentials: Arc<dyn AuthCredentialProvider>,
}

impl AuthHeaderMiddleware {
    pub fn new(credentials: Arc<dyn AuthCredentialProvider>) -> Self {
        Self { credentials }
    }
}

fn apply_auth_header(req: &mut Request, token: &str) {
    match HeaderValue::from_str(&format!("Bearer {token}")) {
        Ok(value) => {
            req.headers_mut()
                .insert(reqwest::header::AUTHORIZATION, value);
        }
        Err(error) => {
            tracing::warn!(%error, "failed to build BYOK Authorization header");
        }
    }
}

#[async_trait::async_trait]
impl Middleware for AuthHeaderMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> Result<Response, Error> {
        if let Some(token) = self.credentials.snapshot().token {
            apply_auth_header(&mut req, &token);
        }
        next.run(req, extensions).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CredentialSnapshot, HttpAuth};
    use reqwest_middleware::ClientBuilder;

    struct Provider(Option<String>);

    impl HttpAuth for Provider {
        fn apply(&self, builder: reqwest::RequestBuilder, _: &str) -> reqwest::RequestBuilder {
            builder
        }
    }

    impl AuthCredentialProvider for Provider {
        fn snapshot(&self) -> CredentialSnapshot {
            CredentialSnapshot {
                token: self.0.clone(),
            }
        }
    }

    fn client(token: Option<&str>) -> reqwest_middleware::ClientWithMiddleware {
        let provider = Arc::new(Provider(token.map(str::to_owned)));
        ClientBuilder::new(reqwest::Client::new())
            .with(AuthHeaderMiddleware::new(provider))
            .build()
    }

    #[tokio::test]
    async fn stamps_byok_bearer_once() {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/")
            .match_header("authorization", "Bearer my-key")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;

        let client = client(Some("my-key"));
        let response = client.get(server.url()).send().await.unwrap();
        assert_eq!(response.status(), 401);
        request.assert_async().await;
    }

    #[tokio::test]
    async fn keyless_request_has_no_stamp() {
        let mut server = mockito::Server::new_async().await;
        let request = server
            .mock("GET", "/")
            .with_status(200)
            .expect(1)
            .create_async()
            .await;

        let client = client(None);
        let response = client.get(server.url()).send().await.unwrap();
        assert_eq!(response.status(), 200);
        request.assert_async().await;
    }
}
