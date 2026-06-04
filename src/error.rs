use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[allow(dead_code)]
    #[error("unauthorized")]
    Unauthorized,

    #[allow(dead_code)]
    #[error("forbidden")]
    Forbidden,

    #[error("payload too large ({0} bytes)")]
    PayloadTooLarge(usize),

    #[error("s3 error: {0}")]
    S3(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("multipart error: {0}")]
    Multipart(String),

    #[error("internal: {0}")]
    Internal(String),
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::head_object::HeadObjectError>> for AppError {
    fn from(e: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::head_object::HeadObjectError>) -> Self {
        use aws_sdk_s3::error::SdkError;
        use aws_sdk_s3::operation::head_object::HeadObjectError;
        match e {
            SdkError::ServiceError(svc) => match svc.into_err() {
                HeadObjectError::NotFound(_) => AppError::NotFound("object not found".into()),
                other => AppError::S3(format!("{other:?}")),
            },
            other => AppError::S3(format!("{other:?}")),
        }
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError>> for AppError {
    fn from(e: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::get_object::GetObjectError>) -> Self {
        use aws_sdk_s3::error::SdkError;
        use aws_sdk_s3::operation::get_object::GetObjectError;
        match e {
            SdkError::ServiceError(svc) => match svc.into_err() {
                GetObjectError::NoSuchKey(_) => AppError::NotFound("object not found".into()),
                other => AppError::S3(format!("{other:?}")),
            },
            other => AppError::S3(format!("{other:?}")),
        }
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::put_object::PutObjectError>> for AppError {
    fn from(e: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::put_object::PutObjectError>) -> Self {
        use aws_sdk_s3::error::SdkError;
        match e {
            SdkError::ServiceError(svc) => AppError::S3(format!("{:?}", svc.into_err())),
            other => AppError::S3(format!("{other:?}")),
        }
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::delete_object::DeleteObjectError>> for AppError {
    fn from(e: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::delete_object::DeleteObjectError>) -> Self {
        use aws_sdk_s3::error::SdkError;
        match e {
            SdkError::ServiceError(svc) => AppError::S3(format!("{:?}", svc.into_err())),
            other => AppError::S3(format!("{other:?}")),
        }
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error>> for AppError {
    fn from(e: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error>) -> Self {
        use aws_sdk_s3::error::SdkError;
        match e {
            SdkError::ServiceError(svc) => AppError::S3(format!("{:?}", svc.into_err())),
            other => AppError::S3(format!("{other:?}")),
        }
    }
}

impl From<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::copy_object::CopyObjectError>> for AppError {
    fn from(e: aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::copy_object::CopyObjectError>) -> Self {
        use aws_sdk_s3::error::SdkError;
        match e {
            SdkError::ServiceError(svc) => AppError::S3(format!("{:?}", svc.into_err())),
            other => AppError::S3(format!("{other:?}")),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".into()),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into()),
            AppError::PayloadTooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, self.to_string()),
            AppError::S3(m) => {
                tracing::error!(error = %m, "s3 error");
                (StatusCode::BAD_GATEWAY, m.clone())
            }
            AppError::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::Multipart(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::Internal(m) => {
                tracing::error!(error = %m, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, m.clone())
            }
        };

        let body = Json(json!({
            "error": message,
            "status": status.as_u16(),
        }));
        (status, body).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
