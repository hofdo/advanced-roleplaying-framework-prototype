use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use engine::TurnPipelineError;
use serde::Serialize;

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "not found".into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    pub fn status(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl From<TurnPipelineError> for ApiError {
    fn from(error: TurnPipelineError) -> Self {
        match error {
            TurnPipelineError::NotFound => Self::not_found(),
            TurnPipelineError::Lock(_) => Self {
                status: StatusCode::CONFLICT,
                message: error.to_string(),
            },
            TurnPipelineError::DeltaValidation(_) => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: error.to_string(),
            },
            TurnPipelineError::Provider(_) => Self {
                status: StatusCode::BAD_GATEWAY,
                message: error.to_string(),
            },
            TurnPipelineError::Parse(_) => Self::bad_request(error.to_string()),
            TurnPipelineError::Store(_) => Self::internal(error.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct ErrorBody {
            error: String,
        }

        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}
