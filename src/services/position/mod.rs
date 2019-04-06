use hyper::{Request,Body};
use websock::{spawn_websocket, self as ws};
use futures::future;
use super::ResponseFuture;

mod cache;



pub fn position_service(req: Request<Body>) -> ResponseFuture {
    debug!("We got these headers: {:?}", req.headers());

    let res = spawn_websocket(req, |m| {
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

        
    });

    Box::new(future::ok(res))
}
