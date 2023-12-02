#[macro_use]
extern crate log;
extern crate websock as ws;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::{Body as BodyTrait, Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{self, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::io;
use std::net::SocketAddr;
use std::{convert::Infallible, time::Duration};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type Body = Full<Bytes>;
type BoxedBody<T> = http_body_util::combinators::BoxBody<T, Infallible>;

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

fn box_body<T, D>(response: Response<T>) -> Response<BoxedBody<D>>
where
    T: BodyTrait<Error = Infallible, Data = D> + Send + Sync + 'static,
{
    let (parts, body) = response.into_parts();
    let body = body.boxed();
    Response::from_parts(parts, body)
}

async fn route(req: Request<Incoming>) -> Result<Response<BoxedBody<Bytes>>, Infallible> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => send_file(INDEX_PATH).await.map(box_body),
        (&Method::GET, "/socket") => server_upgrade(req).await.map(box_body),
        _ => Ok(box_body(not_found())),
    }
    .or_else(|e| Ok(box_body(error_response(e.to_string()))))
}

async fn process_message(m: ws::Message, ctx: &mut u32) -> ws::MessageResult {
    debug!("Got message {:?}", m);
    *ctx += 1;
    let text = format!("{}: {}", ctx, m.to_str().expect("string message"));
    Ok(Some(ws::Message::text(text)))
}

/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(req: Request<Incoming>) -> Result<Response<Empty<Bytes>>, io::Error> {
    debug!("We got these headers: {:?}", req.headers());

    Ok(ws::spawn_websocket(
        req,
        process_message,
        0,
        Some(Duration::from_secs(5 * 60)),
    ))
}

async fn serve_http(listener: TcpListener) {
    loop {
        let (socket, _) = listener.accept().await.expect("Error accepting connection");
        let stream = TokioIo::new(socket);
        tokio::spawn(async move {
            let conn = http1::Builder::new().serve_connection(stream, service_fn(route));
            let conn = conn.with_upgrades();
            if let Err(e) = conn.await {
                error!("Error in connection: {}", e);
            }
        });
    }
}
#[tokio::main]
async fn main() -> Result<(), GenericError> {
    env_logger::init();
    let addr: SocketAddr = ([127, 0, 0, 1], 5000).into();
    let listener = TcpListener::bind(addr).await.expect("failed to bind");
    info!("Serving on {}", addr);
    serve_http(listener).await;
    Ok(())
}
