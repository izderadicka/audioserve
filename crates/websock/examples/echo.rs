#[macro_use]
extern crate log;
extern crate websock as ws;

use hyper::rt;
use hyper::server::Server;
use hyper::service::service_fn_ok;
use hyper::{Body, Request, Response};
use tokio::prelude::*;
use futures::future;

/// Our server HTTP handler to initiate HTTP upgrades.
fn server_upgrade(req: Request<Body>) -> Response<Body> {
    debug!("We got these headers: {:?}", req.headers());

    ws::spawn_websocket(req, |m| {
        debug!("Got message {:?}", m);
        let counter: u64 = {
            let mut c = m.context_ref().write().unwrap();
            *c = *c + 1;
            *c
        };

        Box::new(
            future::ok(
                Some(ws::Message::text(format!("{}: {}", counter, m.to_str().unwrap()), m.context()))
            )
        )

        
    })
}

fn main() {
    pretty_env_logger::init();
    let addr = ([127, 0, 0, 1], 5000).into();

    let server = Server::bind(&addr)
        .serve(|| service_fn_ok(server_upgrade))
        .map_err(|e| eprintln!("server error: {}", e));

    rt::run(server);
}
