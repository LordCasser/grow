//! Remote storage client for the backend.

pub mod agent;
pub mod client;

pub use agent::{
    SandboxClient, SandboxCreateEnvironmentRequest, SandboxEnvironment, SandboxEnvironmentResponse,
    SandboxEnvironmentVariable, SandboxEnvironmentWithMetadata, SandboxForkRequest,
    SandboxForkResponse, SandboxForkedSession, SandboxHibernateResponse,
    SandboxListEnvironmentsRequest, SandboxListEnvironmentsResponse,
    SandboxListPreinstalledPackagesResponse, SandboxLogsExitCodes, SandboxLogsResponse,
    SandboxMode, SandboxPreinstalledPackage, SandboxRestoreRequest, SandboxRestoreResponse,
    SandboxSecretInput, SandboxStartRequest, SandboxStartResponse, SandboxStatusResponse,
    SandboxTerminateRequest, SandboxUpdateEnvironmentRequest,
};
pub use client::{
    BackendClient, BackendError, FetchModelsResult, FetchedBundle, SettingsFetch, fetch_bundle,
    fetch_login_device_flow, fetch_settings_blocking, fetch_subagent_bundle, share_url,
};
pub(crate) use client::{DEFAULT_CONTEXT_WINDOW, fetch_models_blocking, models_list_url};
