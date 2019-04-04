#[macro_use]
extern crate log;

use futures::future::{self, poll_fn};
use futures::prelude::*;
use headers::{self, HeaderMapExt};
use hyper::header::{self, AsHeaderName, HeaderMap, HeaderValue};
use hyper::rt;
use hyper::{Body, Request, Response, StatusCode};
use quick_error::quick_error;
use std::fmt;
use std::io;
use tungstenite::protocol;

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        Ws(err: tungstenite::Error) {
            from()
        }
        Io(err: io::Error) {
            from()
        }

        InvalidMessageType {
            description("Message is of incorrect type")
        }
    }
}

fn header_matches<S: AsHeaderName>(headers: &HeaderMap<HeaderValue>, name: S, value: &str) -> bool {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_lowercase() == value)
        .unwrap_or(false)
}

/// This is a high level function that spawn a websocket handler from 
/// appropriate HTTP request (e.g. websocket upgrade request).
/// Each incomming message can be then processed with function f,
/// which returns future with optional response to this message.
/// This function returns immediate HTPP response, which is either of status
/// 101 Protocol upgrade, if websocket handshake is OK, or of status 400, if 
/// handshake was no successful.
/// 
/// All messages in this websocket share (guarded by RwLock) context of type T
pub fn spawn_websocket<T, F>(req: Request<Body>, mut f: F) -> Response<Body>
where
    T: Default + Send + Sync + 'static,
    F: FnMut(Message<T>) -> Box<Future<Item = Option<Message<T>>, Error = Error> + Send>
        + Send
        + 'static,
{
    let res = match upgrade_connection::<T>(req) {
        Err(r) => r,
        Ok((r, ws_future)) => {
            let ws_process = ws_future
                .map_err(|err| error!("Cannot create websocket: {} ", err))
                .and_then(move |ws| {
                    let (tx, rc) = ws.split();
                    rc.and_then(move |m| match m.inner {
                        protocol::Message::Ping(p) => {
                            debug!("Got ping {:?}",p);
                            Box::new(future::ok(Some(Message {
                            inner: protocol::Message::Pong(p),
                            context: m.context,
                        })))},
                        _ => f(m),
                    })
                    .filter_map(|m| m)
                    .forward(tx)
                    .map(|_| debug!("Websocket has ended"))
                    .map_err(|err| error!("Socket error {}", err))
                });
            rt::spawn(ws_process);
            r
        }
    };
    res
}

/// This function does basic websocket handshake, 
/// return tuple of successful HTTP response (with status 101 - Protocol Upgrade) and
/// future resolving to Websocket( struct implementing Stream and Sink of messages) or 
/// error response (status 400) oif websocket handshake was not successful
/// 
/// Websocket can have context of type T, which is then shared (guarded by RwLock) with all
/// messages in this websocket.
pub fn upgrade_connection<T: Default>(
    req: Request<Body>,
) -> Result<
    (
        Response<Body>,
        impl Future<Item = WebSocket<T>, Error = hyper::Error> + Send,
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
    let upgraded = req.into_body().on_upgrade().map(|upgraded| {
        debug!("Connection upgraded to websocket");
        WebSocket::new(upgraded)
    });

    Ok((res, upgraded))
}

use std::sync::{Arc, RwLock};

/// A websocket `Stream` and `Sink`
/// This struct can hold a context for this particular connection
pub struct WebSocket<T> {
    inner: protocol::WebSocket<::hyper::upgrade::Upgraded>,
    context: Arc<RwLock<T>>,
}

impl<T: Default> WebSocket<T> {

    /// Creates new WebSocket from an upgraded connection with default context
    pub(crate) fn new(upgraded: hyper::upgrade::Upgraded) -> Self {
        let inner = protocol::WebSocket::from_raw_socket(upgraded, protocol::Role::Server, None);
        WebSocket {
            inner,
            context: Arc::new(RwLock::new(T::default())),
        }
    }
}

impl <T> WebSocket<T> {

    /// Creates new WebSocket from an upgraded connection with default context
    #[allow(dead_code)]
    pub(crate) fn new_with_context(upgraded: hyper::upgrade::Upgraded, context: T) -> Self {
        let inner = protocol::WebSocket::from_raw_socket(upgraded, protocol::Role::Server, None);
        WebSocket {
            inner,
            context: Arc::new(RwLock::new(context)),
        }
    }

    /// Gracefully close this websocket.
    pub fn close(mut self) -> impl Future<Item = (), Error = Error> {
        poll_fn(move || Sink::close(&mut self))
    }
}

impl<T> Stream for WebSocket<T> {
    type Item = Message<T>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let msg = match self.inner.read_message() {
                Ok(item) => item,
                Err(::tungstenite::Error::Io(ref err))
                    if err.kind() == io::ErrorKind::WouldBlock =>
                {
                    return Ok(Async::NotReady);
                }
                Err(::tungstenite::Error::ConnectionClosed(frame)) => {
                    trace!("websocket closed: {:?}", frame);
                    return Ok(Async::Ready(None));
                }
                Err(e) => {
                    debug!("websocket poll error: {}", e);
                    return Err(Error::Ws(e));
                }
            };

            match msg {
                msg @ protocol::Message::Text(..)
                | msg @ protocol::Message::Binary(..)
                | msg @ protocol::Message::Ping(..) => {
                    return Ok(Async::Ready(Some(Message {
                        inner: msg,
                        context: self.context.clone(),
                    })));
                }
                protocol::Message::Pong(payload) => {
                    trace!("websocket client pong: {:?}", payload);
                }
            }
        }
    }
}

impl<T> Sink for WebSocket<T> {
    type SinkItem = Message<T>;
    type SinkError = Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        match item.inner {
            protocol::Message::Ping(..) => {
                // warp doesn't yet expose a way to construct a `Ping` message,
                // so the only way this could is if the user is forwarding the
                // received `Ping`s straight back.
                //
                // tungstenite already auto-reponds to `Ping`s with a `Pong`,
                // so this just prevents accidentally sending extra pings.
                return Ok(AsyncSink::Ready);
            }
            _ => (),
        }

        match self.inner.write_message(item.inner) {
            Ok(()) => Ok(AsyncSink::Ready),
            Err(::tungstenite::Error::SendQueueFull(inner)) => {
                debug!("websocket send queue full");
                Ok(AsyncSink::NotReady(Message {
                    inner,
                    context: self.context.clone(),
                }))
            }
            Err(::tungstenite::Error::Io(ref err)) if err.kind() == io::ErrorKind::WouldBlock => {
                // the message was accepted and partly written, so this
                // isn't an error.
                Ok(AsyncSink::Ready)
            }
            Err(e) => {
                debug!("websocket start_send error: {}", e);
                Err(Error::Ws(e))
            }
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        match self.inner.write_pending() {
            Ok(()) => Ok(Async::Ready(())),
            Err(::tungstenite::Error::Io(ref err)) if err.kind() == io::ErrorKind::WouldBlock => {
                Ok(Async::NotReady)
            }
            Err(err) => {
                debug!("websocket poll_complete error: {}", err);
                Err(Error::Ws(err))
            }
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        match self.inner.close(None) {
            Ok(()) => Ok(Async::Ready(())),
            Err(::tungstenite::Error::Io(ref err)) if err.kind() == io::ErrorKind::WouldBlock => {
                Ok(Async::NotReady)
            }
            Err(::tungstenite::Error::ConnectionClosed(frame)) => {
                trace!("websocket closed: {:?}", frame);
                return Ok(Async::Ready(()));
            }
            Err(err) => {
                debug!("websocket close error: {}", err);
                Err(Error::Ws(err))
            }
        }
    }
}

impl<T> fmt::Debug for WebSocket<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("WebSocket").finish()
    }
}

/// A WebSocket message.
///
/// Only repesents Text and Binary messages.
///
#[derive(Clone)]
pub struct Message<T> {
    inner: protocol::Message,
    context: Arc<RwLock<T>>,
}

impl<T> Message<T> {
    /// Constructs a new Text `Message`.
    pub fn text<S: Into<String>>(s: S, context: Arc<RwLock<T>>) -> Self {
        Message {
            inner: protocol::Message::text(s),
            context,
        }
    }

    /// Constructs a new Binary `Message`.
    pub fn binary<V: Into<Vec<u8>>>(v: V, context: Arc<RwLock<T>>) -> Self {
        Message {
            inner: protocol::Message::binary(v),
            context,
        }
    }

    /// Returns true if this message is a Text message.
    pub fn is_text(&self) -> bool {
        self.inner.is_text()
    }

    /// Returns true if this message is a Binary message.
    pub fn is_binary(&self) -> bool {
        self.inner.is_binary()
    }

    /// Returns true if this message is a Ping message.
    pub fn is_ping(&self) -> bool {
        self.inner.is_ping()
    }

    /// Tries to get a reference to the string text, if this is a Text message.
    pub fn to_str(&self) -> Result<&str, Error> {
        match self.inner {
            protocol::Message::Text(ref s) => Ok(s),
            _ => Err(Error::InvalidMessageType),
        }
    }

    /// Returns the bytes of this message.
    pub fn as_bytes(&self) -> &[u8] {
        match self.inner {
            protocol::Message::Text(ref s) => s.as_bytes(),
            protocol::Message::Binary(ref v) => v,
            _ => unreachable!(),
        }
    }

    /// Consumes this message and returns it's context
    pub fn context(self) -> Arc<RwLock<T>> {
        self.context
    }

    /// Returns reference to this message context
    pub fn context_ref(&self) -> &Arc<RwLock<T>> {
        &self.context
    }
}

impl<T> fmt::Debug for Message<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}
