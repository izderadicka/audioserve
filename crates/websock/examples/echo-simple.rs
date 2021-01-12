#[macro_use]
extern crate log;
use futures::prelude::*;
use futures::stream::StreamExt;
use headers::{self, HeaderMapExt};
use hyper::header::{self, AsHeaderName, HeaderMap, HeaderValue};
use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{self, Body, Method, Request, Response, StatusCode};
use std::convert::Infallible;
use std::io;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_tungstenite::{tungstenite::protocol, WebSocketStream};

type GenericError = Box<dyn std::error::Error + Send + Sync>;

static INDEX_PATH: &str = "examples/index.html";

async fn send_file(p: &str) -> Result<Response<Body>, std::io::Error> {
    let mut f = File::open(p).await?;
    let mut data = Vec::new();
    f.read_to_end(&mut data).await?;
    Ok(Response::new(data.into()))
}

fn error_response(err: String) -> Response<Body> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(err.into())
        .unwrap()
}

fn not_found() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body("Not Found".into())
        .unwrap()
}

async fn route(req: Request<Body>) -> Result<Response<Body>, io::Error> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => send_file(INDEX_PATH).await,
        (&Method::GET, "/socket") => handle_ws_connection(req),
        _ => Ok(not_found()),
    }
    .or_else(|e| Ok(error_response(e.to_string())))
}

fn header_matches<S: AsHeaderName>(headers: &HeaderMap<HeaderValue>, name: S, value: &str) -> bool {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase() == value)
        .unwrap_or(false)
}

pub fn upgrade_connection(
    mut req: Request<Body>,
) -> Result<
    (
        Response<Body>,
        impl Future<Output = Result<WebSocketStream<hyper::upgrade::Upgraded>, ()>> + Send,
    ),
    Response<Body>,
> {
    let mut res = Response::new(Body::empty());
    let mut header_error = false;
    debug!("We got these headers: {:?}", req.headers());

    if !header_matches(req.headers(), header::UPGRADE, "websocket") {
        error!("Upgrade is not to websocket");
        header_error = true;
    }

    if !header_matches(req.headers(), header::SEC_WEBSOCKET_VERSION, "13") {
        error!("Websocket protocol version must be 13");
        header_error = true;
    }

    if !req
        .headers()
        .typed_get::<headers::Connection>()
        .map(|h| h.contains("Upgrade"))
        .unwrap_or(false)
    {
        error!("It must be upgrade connection");
        header_error = true;
    }

    let key = req.headers().typed_get::<headers::SecWebsocketKey>();

    if key.is_none() {
        error!("Websocket key missing");
        header_error = true;
    }

    if header_error {
        *res.status_mut() = StatusCode::BAD_REQUEST;
        return Err(res);
    }

    *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    let h = res.headers_mut();
    h.typed_insert(headers::Upgrade::websocket());
    h.typed_insert(headers::SecWebsocketAccept::from(key.unwrap()));
    h.typed_insert(headers::Connection::upgrade());
    let upgraded = hyper::upgrade::on(&mut req)
        .map_err(|err| error!("Cannot create websocket: {} ", err))
        .and_then(|upgraded| async {
            debug!("Connection upgraded to websocket");
            let r = WebSocketStream::from_raw_socket(upgraded, protocol::Role::Server, None).await;
            Ok(r)
        });

    Ok((res, upgraded))
}

// Just echo back received messages.
fn handle_ws_connection(req: Request<Body>) -> Result<Response<Body>, io::Error> {
    let res = match upgrade_connection(req) {
        Err(res) => res,
        Ok((res, ws)) => {
            let run_ws_task = async {
                match ws.await {
                    Ok(ws) => {
                        debug!("Spawning WS");
                        let mut counter = 0;
                        let (tx, rc) = ws.split();
                        let rc = rc.try_filter_map(|m| {
                            debug!("Got message {:?}", m);
                            future::ok(match m {
                                protocol::Message::Text(text) => {
                                    counter += 1;
                                    Some(protocol::Message::text(format!(
                                        "Response {}: {}",
                                        counter, text
                                    )))
                                }
                                _ => None,
                            })
                        });
                        match rc.forward(tx).await {
                            Err(e) => error!("WS Error {}", e),
                            Ok(_) => debug!("Websocket has ended"),
                        }
                    }
                    Err(_e) => error!("WS error"),
                }
            };
            tokio::spawn(run_ws_task);
            res
        }
    };
    debug!("WS HTTP Response {:?}", res);
    Ok(res)
}

#[tokio::main]
async fn main() -> Result<(), GenericError> {
    env_logger::init();
    let addr = ([127, 0, 0, 1], 5000).into();
    let service = make_service_fn(|_| async { Ok::<_, Infallible>(service_fn(route)) });
    let server = Server::bind(&addr).serve(service);
    info!("Serving on {}", addr);
    server.await?;

    Ok(())
}
