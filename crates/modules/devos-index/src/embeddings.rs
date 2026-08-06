//! Chunk/query embeddings from a local Ollama.
//!
//! Ollama is the right source here for the same reasons it is DevOS's
//! offline chat provider: it is already a supported provider, it runs on
//! localhost, it needs no API key, and no source code leaves the machine.
//!
//! **Every error in this module is a soft error.** Nothing here is allowed
//! to fail an index run or a search — the caller degrades to lexical-only.

use std::sync::Mutex;
use std::time::Duration;

/// Conventional local embedding model (`ollama pull nomic-embed-text`).
pub const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";
/// Same default the AI module uses for Ollama.
pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434";

/// Generous: a cold local model can take a while to load on the first call.
pub const INDEX_TIMEOUT: Duration = Duration::from_secs(60);
/// Tight: search is interactive, and a stalled backend must not hold up the
/// lexical results that are already in hand.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    /// Ollama isn't running, isn't reachable, or the model isn't pulled.
    /// The caller's only correct response is to carry on without vectors.
    #[error("embeddings backend unavailable: {0}")]
    Unavailable(String),
    /// Reached it, but the payload wasn't a usable vector.
    #[error("embeddings backend returned an unusable response: {0}")]
    BadResponse(String),
}

pub type EmbedResult<T> = Result<T, EmbedError>;

/// Anything that can turn text into vectors. A trait so the index can be
/// tested — including its degradation path — without a live Ollama.
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    /// Stored alongside each vector so a model change invalidates the rows
    /// it produced instead of silently mixing incompatible embedding spaces.
    fn model_id(&self) -> &str;

    /// Embed `texts` in order. The returned vector must be the same length
    /// as `texts`; a short reply is a [`EmbedError::BadResponse`].
    async fn embed(&self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>>;
}

/// Which embeddings route this server speaks. Newer Ollama has `/api/embed`
/// (batched); older builds only have `/api/embeddings` (one text per call).
/// We probe once per process and remember the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    /// `POST /api/embed` — `{"model", "input": [...]}` → `{"embeddings": [[...]]}`
    Batch,
    /// `POST /api/embeddings` — `{"model", "prompt"}` → `{"embedding": [...]}`
    Single,
}

pub struct OllamaEmbedder {
    client: reqwest::Client,
    base_url: String,
    model: String,
    timeout: Duration,
    route: Mutex<Option<Route>>,
}

impl OllamaEmbedder {
    pub fn new(base_url: &str, model: &str, timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            timeout,
            route: Mutex::new(None),
        }
    }

    fn cached_route(&self) -> Option<Route> {
        self.route.lock().ok().and_then(|guard| *guard)
    }

    fn remember_route(&self, route: Route) {
        if let Ok(mut guard) = self.route.lock() {
            *guard = Some(route);
        }
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> EmbedResult<serde_json::Value> {
        let response = self
            .client
            .post(format!("{}{path}", self.base_url))
            .timeout(self.timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbedError::Unavailable(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            // 404 covers both "this Ollama has no such route" and "model not
            // pulled". Either way there is nothing to retry against.
            let detail = response.text().await.unwrap_or_default();
            let detail = detail.chars().take(200).collect::<String>();
            return Err(EmbedError::Unavailable(format!(
                "{path} returned HTTP {}: {detail}",
                status.as_u16()
            )));
        }
        response
            .json()
            .await
            .map_err(|e| EmbedError::BadResponse(e.to_string()))
    }

    async fn embed_batch_route(&self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
        let value = self
            .post(
                "/api/embed",
                serde_json::json!({ "model": self.model, "input": texts }),
            )
            .await?;
        let rows = value["embeddings"].as_array().ok_or_else(|| {
            EmbedError::BadResponse("/api/embed reply had no `embeddings` array".into())
        })?;
        let vectors: Vec<Vec<f32>> = rows.iter().filter_map(parse_vector).collect();
        if vectors.len() != texts.len() {
            return Err(EmbedError::BadResponse(format!(
                "/api/embed returned {} vectors for {} inputs",
                vectors.len(),
                texts.len()
            )));
        }
        Ok(vectors)
    }

    async fn embed_single_route(&self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let value = self
                .post(
                    "/api/embeddings",
                    serde_json::json!({ "model": self.model, "prompt": text }),
                )
                .await?;
            let vector = parse_vector(&value["embedding"]).ok_or_else(|| {
                EmbedError::BadResponse("/api/embeddings reply had no `embedding` array".into())
            })?;
            out.push(vector);
        }
        Ok(out)
    }
}

#[async_trait::async_trait]
impl Embedder for OllamaEmbedder {
    fn model_id(&self) -> &str {
        &self.model
    }

    async fn embed(&self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(route) = self.cached_route() {
            return match route {
                Route::Batch => self.embed_batch_route(texts).await,
                Route::Single => self.embed_single_route(texts).await,
            };
        }

        // First call: try the modern route, fall back to the legacy one, and
        // remember whichever answered so later calls cost one request.
        match self.embed_batch_route(texts).await {
            Ok(vectors) => {
                self.remember_route(Route::Batch);
                Ok(vectors)
            }
            Err(batch_err) => match self.embed_single_route(texts).await {
                Ok(vectors) => {
                    self.remember_route(Route::Single);
                    Ok(vectors)
                }
                Err(single_err) => Err(EmbedError::Unavailable(format!(
                    "/api/embed: {batch_err}; /api/embeddings: {single_err}"
                ))),
            },
        }
    }
}

/// A JSON array of numbers → `Vec<f32>`. `None` if it isn't one, or if it's
/// empty (an empty vector can never contribute to a ranking).
fn parse_vector(value: &serde_json::Value) -> Option<Vec<f32>> {
    let array = value.as_array()?;
    if array.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(array.len());
    for entry in array {
        out.push(entry.as_f64()? as f32);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ollama_vector_shapes() {
        let batch: serde_json::Value =
            serde_json::from_str(r#"{"embeddings":[[0.5,-1.0],[0.0,2.0]]}"#).unwrap();
        assert_eq!(
            parse_vector(&batch["embeddings"][0]).unwrap(),
            vec![0.5, -1.0]
        );

        let single: serde_json::Value = serde_json::from_str(r#"{"embedding":[1,2,3]}"#).unwrap();
        assert_eq!(
            parse_vector(&single["embedding"]).unwrap(),
            vec![1.0, 2.0, 3.0]
        );

        // Shapes that must not be mistaken for a vector.
        assert_eq!(parse_vector(&serde_json::json!([])), None);
        assert_eq!(parse_vector(&serde_json::json!(null)), None);
        assert_eq!(parse_vector(&serde_json::json!(["a", "b"])), None);
        assert_eq!(
            parse_vector(&serde_json::json!({"error": "no model"})),
            None
        );
    }

    /// Nothing is listening on port 1, so this is the "Ollama isn't running"
    /// case — the common one — reported as a soft `Unavailable`, quickly.
    #[tokio::test]
    async fn unreachable_backend_reports_unavailable() {
        let embedder = OllamaEmbedder::new(
            "http://127.0.0.1:1",
            DEFAULT_EMBED_MODEL,
            Duration::from_millis(500),
        );
        let err = embedder
            .embed(&["hello".to_string()])
            .await
            .expect_err("a closed port cannot produce embeddings");
        assert!(matches!(err, EmbedError::Unavailable(_)), "got {err:?}");

        // Empty input never touches the network at all.
        assert!(embedder.embed(&[]).await.unwrap().is_empty());
    }
}
