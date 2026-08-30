use std::{rc::Rc, sync::Arc};

use serde_json::value::RawValue;

use crate::protocol as acp;

/// Grow-owned v1 agent handler boundary.
///
/// SDK 2.x handlers are `Send`, while Grow's session owner intentionally runs
/// on a `LocalSet`. The connection adapter bridges between the two with
/// channels; implementations of this trait may therefore remain `!Send`.
#[async_trait::async_trait(?Send)]
pub trait AcpAgentHandler {
    async fn initialize(
        &self,
        args: acp::InitializeRequest,
    ) -> acp::Result<acp::InitializeResponse>;

    async fn authenticate(
        &self,
        args: acp::AuthenticateRequest,
    ) -> acp::Result<acp::AuthenticateResponse>;

    async fn new_session(
        &self,
        args: acp::NewSessionRequest,
    ) -> acp::Result<acp::NewSessionResponse>;

    async fn prompt(&self, args: acp::PromptRequest) -> acp::Result<acp::PromptResponse>;

    async fn cancel(&self, args: acp::CancelNotification) -> acp::Result<()>;

    async fn load_session(
        &self,
        _args: acp::LoadSessionRequest,
    ) -> acp::Result<acp::LoadSessionResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn set_session_mode(
        &self,
        _args: acp::SetSessionModeRequest,
    ) -> acp::Result<acp::SetSessionModeResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn set_session_config_option(
        &self,
        _args: acp::SetSessionConfigOptionRequest,
    ) -> acp::Result<acp::SetSessionConfigOptionResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn list_sessions(
        &self,
        _args: acp::ListSessionsRequest,
    ) -> acp::Result<acp::ListSessionsResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Ok(acp::ExtResponse::new(RawValue::NULL.to_owned().into()))
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

/// Grow-owned v1 client handler boundary.
#[async_trait::async_trait(?Send)]
pub trait AcpClientHandler {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse>;

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()>;

    async fn write_text_file(
        &self,
        _args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal(
        &self,
        _args: acp::KillTerminalRequest,
    ) -> acp::Result<acp::KillTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Ok(acp::ExtResponse::new(RawValue::NULL.to_owned().into()))
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Ok(())
    }
}

macro_rules! delegate_agent_handler {
    ($wrapper:ident) => {
        #[async_trait::async_trait(?Send)]
        impl<T: AcpAgentHandler> AcpAgentHandler for $wrapper<T> {
            async fn initialize(
                &self,
                args: acp::InitializeRequest,
            ) -> acp::Result<acp::InitializeResponse> {
                self.as_ref().initialize(args).await
            }
            async fn authenticate(
                &self,
                args: acp::AuthenticateRequest,
            ) -> acp::Result<acp::AuthenticateResponse> {
                self.as_ref().authenticate(args).await
            }
            async fn new_session(
                &self,
                args: acp::NewSessionRequest,
            ) -> acp::Result<acp::NewSessionResponse> {
                self.as_ref().new_session(args).await
            }
            async fn prompt(&self, args: acp::PromptRequest) -> acp::Result<acp::PromptResponse> {
                self.as_ref().prompt(args).await
            }
            async fn cancel(&self, args: acp::CancelNotification) -> acp::Result<()> {
                self.as_ref().cancel(args).await
            }
            async fn load_session(
                &self,
                args: acp::LoadSessionRequest,
            ) -> acp::Result<acp::LoadSessionResponse> {
                self.as_ref().load_session(args).await
            }
            async fn set_session_mode(
                &self,
                args: acp::SetSessionModeRequest,
            ) -> acp::Result<acp::SetSessionModeResponse> {
                self.as_ref().set_session_mode(args).await
            }
            async fn set_session_config_option(
                &self,
                args: acp::SetSessionConfigOptionRequest,
            ) -> acp::Result<acp::SetSessionConfigOptionResponse> {
                self.as_ref().set_session_config_option(args).await
            }
            async fn list_sessions(
                &self,
                args: acp::ListSessionsRequest,
            ) -> acp::Result<acp::ListSessionsResponse> {
                self.as_ref().list_sessions(args).await
            }
            async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
                self.as_ref().ext_method(args).await
            }
            async fn ext_notification(&self, args: acp::ExtNotification) -> acp::Result<()> {
                self.as_ref().ext_notification(args).await
            }
        }
    };
}

macro_rules! delegate_client_handler {
    ($wrapper:ident) => {
        #[async_trait::async_trait(?Send)]
        impl<T: AcpClientHandler> AcpClientHandler for $wrapper<T> {
            async fn request_permission(
                &self,
                args: acp::RequestPermissionRequest,
            ) -> acp::Result<acp::RequestPermissionResponse> {
                self.as_ref().request_permission(args).await
            }
            async fn session_notification(
                &self,
                args: acp::SessionNotification,
            ) -> acp::Result<()> {
                self.as_ref().session_notification(args).await
            }
            async fn write_text_file(
                &self,
                args: acp::WriteTextFileRequest,
            ) -> acp::Result<acp::WriteTextFileResponse> {
                self.as_ref().write_text_file(args).await
            }
            async fn read_text_file(
                &self,
                args: acp::ReadTextFileRequest,
            ) -> acp::Result<acp::ReadTextFileResponse> {
                self.as_ref().read_text_file(args).await
            }
            async fn create_terminal(
                &self,
                args: acp::CreateTerminalRequest,
            ) -> acp::Result<acp::CreateTerminalResponse> {
                self.as_ref().create_terminal(args).await
            }
            async fn terminal_output(
                &self,
                args: acp::TerminalOutputRequest,
            ) -> acp::Result<acp::TerminalOutputResponse> {
                self.as_ref().terminal_output(args).await
            }
            async fn release_terminal(
                &self,
                args: acp::ReleaseTerminalRequest,
            ) -> acp::Result<acp::ReleaseTerminalResponse> {
                self.as_ref().release_terminal(args).await
            }
            async fn wait_for_terminal_exit(
                &self,
                args: acp::WaitForTerminalExitRequest,
            ) -> acp::Result<acp::WaitForTerminalExitResponse> {
                self.as_ref().wait_for_terminal_exit(args).await
            }
            async fn kill_terminal(
                &self,
                args: acp::KillTerminalRequest,
            ) -> acp::Result<acp::KillTerminalResponse> {
                self.as_ref().kill_terminal(args).await
            }
            async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
                self.as_ref().ext_method(args).await
            }
            async fn ext_notification(&self, args: acp::ExtNotification) -> acp::Result<()> {
                self.as_ref().ext_notification(args).await
            }
        }
    };
}

delegate_agent_handler!(Rc);
delegate_agent_handler!(Arc);
delegate_client_handler!(Rc);
delegate_client_handler!(Arc);
