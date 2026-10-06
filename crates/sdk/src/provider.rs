use crate::{ApiError, ArtworkCandidate, ArtworkGame, ArtworkKind, DiscoveredGame, Game};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Debug)]
pub struct ProviderDescriptor {
    pub id: String,
    pub name: String,
}

/// A failed or partial scan must not mark previously discovered games as uninstalled.
pub struct DiscoveryReport {
    pub games: Vec<DiscoveredGame>,
    pub complete: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Default)]
pub struct DiscoverySettings {
    pub steam_directory: Option<PathBuf>,
}

/// Cooperative cancellation is checked between I/O operations; network calls have timeouts.
#[derive(Clone)]
pub struct JobContext {
    cancellation: Arc<AtomicBool>,
    progress: Arc<dyn Fn(String) + Send + Sync>,
}

impl JobContext {
    pub fn new(
        cancellation: Arc<AtomicBool>,
        progress: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        Self {
            cancellation,
            progress: Arc::new(progress),
        }
    }

    pub fn checkpoint(&self) -> Result<(), ApiError> {
        if self.cancellation.load(Ordering::Relaxed) {
            Err(ApiError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn report(&self, message: impl Into<String>) {
        (self.progress)(message.into());
    }
}

/// Compiled-in integrations use the same contract that a future plugin host will adapt.
pub trait DiscoveryProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }
    fn discover(
        &self,
        settings: &DiscoverySettings,
        context: &JobContext,
    ) -> Result<DiscoveryReport, ApiError>;
}

pub trait ArtworkProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn image_hosts(&self) -> Vec<String>;
    fn search(
        &self,
        query: &str,
        credential: &str,
        context: &JobContext,
    ) -> Result<Vec<ArtworkGame>, ApiError>;
    fn requires_credential(&self) -> bool {
        true
    }
    fn validate_credential(
        &self,
        _credential: &str,
        _context: &JobContext,
    ) -> Result<(), ApiError> {
        Err(ApiError::Validation(
            "This provider does not support credential verification".into(),
        ))
    }
    fn game(
        &self,
        _game: &Game,
        _credential: &str,
        _context: &JobContext,
    ) -> Result<ArtworkGame, ApiError> {
        Err(ApiError::NoArtwork)
    }
    fn artwork(
        &self,
        game: &Game,
        remote_game_id: Option<&str>,
        kind: ArtworkKind,
        credential: &str,
        context: &JobContext,
    ) -> Result<Vec<ArtworkCandidate>, ApiError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_and_progress_are_observable() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(std::sync::Mutex::new(String::new()));
        let reported = progress.clone();
        let context = JobContext::new(cancelled.clone(), move |message| {
            *reported.lock().expect("progress mutex") = message;
        });
        assert!(context.checkpoint().is_ok());
        context.report("Reading installed games");
        assert_eq!(
            *progress.lock().expect("progress mutex"),
            "Reading installed games"
        );
        cancelled.store(true, Ordering::Relaxed);
        assert!(matches!(context.checkpoint(), Err(ApiError::Cancelled)));
    }
}
