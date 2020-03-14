#[macro_use]
extern crate log;
extern crate websock as ws;
use futures::future;
use hyper::server::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, self, Method, StatusCode};
use std::convert::Infallible;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use std::io;

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

async fn route(req: Request<Body>) -> Result<Response<Body>,io::Error> {

    
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/") => send_file(INDEX_PATH).await,
        (&Method::GET, "/socket") => server_upgrade(req).await,
        _ => Ok(not_found())
    }
    .or_else(|e| Ok(error_response(e.to_string())))
    
}

/// Our server HTTP handler to initiate HTTP upgrades.
async fn server_upgrade(req: Request<Body>) -> Result<Response<Body>, io::Error> {
    debug!("We got these headers: {:?}", req.headers());

    Ok(ws::spawn_websocket(req, |m| {
        debug!("Got message {:?}", m);
        let counter: u64 = {
            let mut c = m.context_ref().write().unwrap();
            *c = *c + 1;
            *c
        };

        Box::pin(future::ok(Some(ws::Message::text(
            format!("{}: {}", counter, m.to_str().unwrap()),
            m.context(),
        ))))
    }))
}
#[tokio::main]
async fn main() -> Result<(), GenericError> {
    env_logger::init();
    let addr = ([127, 0, 0, 1], 5000).into();
    let service = make_service_fn(|_| async { Ok::<_, Infallible>(service_fn(route))});
    let server = Server::bind(&addr).serve(service);
    info!("Serving on {}", addr);
    server.await?;

    Ok(())
}
