use headers::{
    AccessControlAllowCredentials, AccessControlAllowHeaders, AccessControlAllowMethods,
    AccessControlAllowOrigin, AccessControlExposeHeaders, AccessControlMaxAge,
    AccessControlRequestHeaders, Header, HeaderMapExt, Origin,
};
use http::header::DATE;
use http::{Method, Response, StatusCode};
use std::time::Duration;

use super::super::request::HttpRequest;
use super::HttpResponse;
use super::{body::empty_body, ResponseBuilderExt};

fn header2header<H1: Header, H2: Header>(i: H1) -> Result<impl Header, headers::Error> {
    let mut v = vec![];
    i.encode(&mut v);
    H2::decode(&mut v.iter())
}

pub fn add_cors_headers(mut resp: HttpResponse, origin: Option<Origin>) -> HttpResponse {
    match origin {
        Some(o) => {
            if let Ok(allowed_origin) = header2header::<_, AccessControlAllowOrigin>(o) {
                let headers = resp.headers_mut();
                headers.typed_insert(allowed_origin);
                headers.typed_insert(AccessControlAllowCredentials);
                headers.typed_insert(
                    vec![DATE]
                        .into_iter()
                        .collect::<AccessControlExposeHeaders>(),
                );
            }
            resp
        }
        None => resp,
    }
}

pub fn preflight_cors_response(req: &HttpRequest) -> HttpResponse {
    let origin = req.headers().typed_get::<Origin>();
    const ALLOWED_METHODS: &[Method] = &[Method::GET, Method::POST, Method::OPTIONS];

    let mut resp_builder = Response::builder()
        .status(StatusCode::NO_CONTENT)
        // Allow all requested headers
        .typed_header(AccessControlAllowMethods::from_iter(
            ALLOWED_METHODS.iter().cloned(),
        ))
        .typed_header(AccessControlMaxAge::from(Duration::from_secs(24 * 3600)));

    if let Some(requested_headers) = req.headers().typed_get::<AccessControlRequestHeaders>() {
        resp_builder = resp_builder.typed_header(AccessControlAllowHeaders::from_iter(
            requested_headers.iter(),
        ));
    }

    let resp = resp_builder.body(empty_body()).unwrap();

    add_cors_headers(resp, origin)
}
