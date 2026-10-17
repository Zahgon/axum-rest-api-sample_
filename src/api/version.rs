use std::future::{Ready, ready};

use actix_web::{FromRequest, HttpRequest, dev::Payload, http::StatusCode};
use thiserror::Error;

use crate::api::error::{APIError, APIErrorCode, APIErrorEntry, APIErrorKind};

#[derive(Debug, Clone, Copy)]
pub enum APIVersion {
    V1,
    V2,
}

impl std::str::FromStr for APIVersion {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "v1" => Ok(Self::V1),
            "v2" => Ok(Self::V2),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for APIVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        };
        write!(f, "{}", v)
    }
}

pub fn parse_version(version: &str) -> Result<APIVersion, APIError> {
    version.parse().map_or_else(
        |_| Err(ApiVersionError::InvalidVersion(version.to_owned()).into()),
        |v| Ok(v),
    )
}

impl FromRequest for APIVersion {
    type Error = APIError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let version = match req.match_info().get("version") {
            Some(version) => version.to_owned(),
            None => return ready(Err(ApiVersionError::ParameterMissing.into())),
        };

        ready(parse_version(&version))
    }
}

#[derive(Debug, Error)]
pub enum ApiVersionError {
    #[error("unknown version: {0}")]
    InvalidVersion(String),
    #[error("parameter is missing: version")]
    ParameterMissing,
    #[error("could not extract api version")]
    VersionExtractError,
}

impl From<ApiVersionError> for APIError {
    fn from(err: ApiVersionError) -> Self {
        let error_entry = APIErrorEntry::new(&err.to_string())
            .code(APIErrorCode::ApiVersionError)
            .kind(APIErrorKind::ValidationError);

        (StatusCode::BAD_REQUEST, error_entry).into()
    }
}
