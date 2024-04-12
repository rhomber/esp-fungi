use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use picoserve::response::{ContentBody, ForEachHeader, HeadersIter, Response, StatusCode};
use serde::Serialize;

use crate::error::{map_json_err, Error, Result};

static HTTP_HEADER_CONTENT_TYPE: &str = "Content-Type";
static HTTP_HEADER_CONNECTION: &str = "Connection";

pub(crate) type BodyResponse = Response<impl HeadersIter, ContentBody<String>>;

struct ResponseBuilder {
    status: StatusCode,
    body: String,
    headers: Headers,
}

impl ResponseBuilder {
    fn new(status: StatusCode, body: String) -> Self {
        Self {
            status,
            body,
            headers: Headers::new(),
        }
    }

    fn with_headers(mut self, key: &'static str, value: &'static str) -> Self {
        self.headers = self.headers.push(key, value);
        self
    }
    fn build(self) -> BodyResponse {
        Response::new(self.status, self.body).with_headers(self.headers)
    }
}

struct Headers(Vec<(&'static str, &'static str)>);

impl Headers {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn push(mut self, key: &'static str, value: &'static str) -> Self {
        self.0.push((key, value));
        self
    }
}

impl HeadersIter for Headers {
    async fn for_each_header<F: ForEachHeader>(
        self,
        mut f: F,
    ) -> core::result::Result<F::Output, F::Error> {
        for (name, value) in self.0 {
            f.call(name, value).await?;
        }

        f.finalize().await
    }
}

pub(crate) fn json_response<T>(status: StatusCode, body: &T) -> BodyResponse
where
    T: ?Sized + Serialize,
{
    prepare_response(_json_response::<T>(status, body)).build()
}

fn _json_response<T>(status: StatusCode, body: &T) -> Result<ResponseBuilder>
where
    T: ?Sized + Serialize,
{
    Ok(
        ResponseBuilder::new(status, serde_json::to_string(body).map_err(map_json_err)?)
            .with_headers(HTTP_HEADER_CONTENT_TYPE, "application/json"),
    )
}

#[allow(dead_code)]
pub(crate) fn error_response(status: StatusCode, err: Error) -> BodyResponse {
    prepare_response(Ok(_error_response(status, err))).build()
}

fn _error_response(status: StatusCode, err: Error) -> ResponseBuilder {
    match _json_response(
        status,
        &ErrorResponse::new(status.as_u16(), format!("{}", err)),
    ) {
        Ok(resp) => resp,
        Err(e) => {
            log::warn!("Failed to encode error response: {:?}", e);

            // Crude default:
            ResponseBuilder::new(status, format!("{}", err))
                .with_headers(HTTP_HEADER_CONNECTION, "Close")
        }
    }
}

fn prepare_response(maybe_resp: Result<ResponseBuilder>) -> ResponseBuilder {
    match maybe_resp {
        Ok(resp) => resp.with_headers(HTTP_HEADER_CONNECTION, "Close"),
        Err(e) => {
            log::error!("API encountered error while serving request: {:?}", e);
            _error_response(StatusCode::INTERNAL_SERVER_ERROR, e)
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    status: u16,
    msg: String,
}

impl ErrorResponse {
    fn new(status: u16, msg: String) -> Self {
        Self { status, msg }
    }
}
