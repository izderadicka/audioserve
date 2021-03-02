#[macro_use]
extern crate log;

use futures::prelude::*;
use futures::ready;
use headers::{self, HeaderMapExt};
use hyper::header::{self, AsHeaderName, HeaderMap, HeaderValue};
use hyper::upgrade;
use hyper::{Body, Request, Response, StatusCode};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{fmt, time::Duration};
use thiserror::Error;
use tokio::{self, sync::RwLock};
use tokio_tungstenite::{
    tungstenite::{self, protocol},
    WebSocketStream,
};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Websocket error: {0}")]
    Ws(#[from] tungstenite::Error),

    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Message is of incorrect type")]
    InvalidMessageType,
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
/// This function returns immediate HTTP response, which is either of status
/// 101 Protocol upgrade, if websocket handshake is OK, or of status 400, if
/// handshake was no successful.
///
/// All messages in this websocket share (guarded by RwLock) context of type T
pub fn spawn_websocket<T, F>(req: Request<Body>, f: F) -> Response<Body>
where
    T: Default + Send + Sync + 'static,
    F: FnMut(Message<T>) -> Pin<Box<dyn Future<Output = Result<Option<Message<T>>, Error>> + Send>>
        + Send
        + 'static,
{
    spawn_websocket_inner(req, f, None)
}

pub fn spawn_websocket_with_timeout<T, F>(
    req: Request<Body>,
    f: F,
    timeout: Duration,
) -> Response<Body>
where
    T: Default + Send + Sync + 'static,
    F: FnMut(Message<T>) -> Pin<Box<dyn Future<Output = Result<Option<Message<T>>, Error>> + Send>>
        + Send
        + 'static,
{
    spawn_websocket_inner(req, f, Some(timeout))
}

/// Implementation of spawn websocket
fn spawn_websocket_inner<T, F>(
    req: Request<Body>,
    mut f: F,
    timeout: Option<Duration>,
) -> Response<Body>
where
    T: Default + Send + Sync + 'static,
    F: FnMut(Message<T>) -> Pin<Box<dyn Future<Output = Result<Option<Message<T>>, Error>> + Send>>
        + Send
        + 'static,
{
    match upgrade_connection::<T>(req) {
        Err(r) => r,
        Ok((r, ws_future)) => {
            let ws_process = async move {
                match ws_future.await {
                    Err(_) => error!("Failed upgrade to websocket"),
                    Ok(ws) => {
                        let (mut tx, mut rc) = ws.split();
                        loop {
                            let next = async {
                                match timeout {
                                    None => Ok(rc.next().await),
                                    Some(d) => tokio::time::timeout(d, rc.next()).await,
                                }
                            };
                            match next.await {
                                Err(_) => {
                                    debug!("Timeout on websocket - let's close");
                                    //TODO: Send Close or just break?
                                    break;
                                }

                                Ok(None) => {
                                    debug!("Websocket stream has ended");
                                    break;
                                }

                                Ok(Some(msg)) => {
                                    match msg {
                                        Ok(m) => {
                                            let reply: Option<Message<_>> = match m.inner {
                                                protocol::Message::Ping(p) => {
                                                    // Send Pong for Ping
                                                    debug!("Got ping {:?}", p);
                                                    Some(Message {
                                                        inner: protocol::Message::Pong(p),
                                                        context: m.context,
                                                    })
                                                }
                                                protocol::Message::Close(_) => {
                                                    debug!("Got close message from client");
                                                    // TODO: According to RFC6455 we should reply to close message - is it done by library or do we need to do it here?
                                                    None
                                                }
                                                _ => match f(m).await {
                                                    Ok(m) => m,
                                                    Err(e) => {
                                                        error!("error when processing message: {}; will close WS", e);
                                                        break;
                                                    }
                                                },
                                            };

                                            if let Some(m) = reply {
                                                if let Err(e) = tx.send(m).await {
                                                    error!("error sending reply message: {}", e);
                                                };
                                            }
                                        }
                                        Err(e) => error!("message error: {:?} {}", e, e),
                                    }
                                }
                            }
                        }
                        debug!("Websocket closed")
                    }
                }
            };
            tokio::spawn(ws_process);
            r
        }
    }
}

/// This function does basic websocket handshake,
/// return tuple of successful HTTP response (with status 101 - Protocol Upgrade) and
/// future resolving to Websocket( struct implementing Stream and Sink of messages) or
/// error response (status 400) if websocket handshake was not successful
///
/// Websocket can have context of type T, which is then shared (guarded by RwLock) with all
/// messages in this websocket.
pub fn upgrade_connection<T: Default>(
    mut req: Request<Body>,
) -> Result<
    (
        Response<Body>,
        impl Future<Output = Result<WebSocket<T>, ()>> + Send,
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
    let upgraded = upgrade::on(&mut req)
        .map_err(|err| error!("Cannot create websocket: {} ", err))
        .and_then(|upgraded| async {
            debug!("Connection upgraded to websocket");
            let r = WebSocket::new(upgraded).await;
            Ok(r)
        });

    Ok((res, upgraded))
}

/// A websocket `Stream` and `Sink`
/// This struct can hold a context for this particular connection
pub struct WebSocket<T> {
    inner: WebSocketStream<::hyper::upgrade::Upgraded>,
    context: Arc<RwLock<T>>,
}

impl<T: Default> WebSocket<T> {
    /// Creates new WebSocket from an upgraded connection with default context
    pub(crate) async fn new(upgraded: hyper::upgrade::Upgraded) -> Self {
        let inner = WebSocketStream::from_raw_socket(upgraded, protocol::Role::Server, None).await;
        WebSocket {
            inner,
            context: Arc::new(RwLock::new(T::default())),
        }
    }
}

impl<T> WebSocket<T> {
    /// Creates new WebSocket from an upgraded connection with default context
    #[allow(dead_code)]
    pub(crate) async fn new_with_context(upgraded: hyper::upgrade::Upgraded, context: T) -> Self {
        let inner = WebSocketStream::from_raw_socket(upgraded, protocol::Role::Server, None).await;
        WebSocket {
            inner,
            context: Arc::new(RwLock::new(context)),
        }
    }
}

impl<T> Stream for WebSocket<T> {
    type Item = Result<Message<T>, crate::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match ready!(Pin::new(&mut self.inner).poll_next(cx)) {
            Some(Ok(item)) => Poll::Ready(Some(Ok(Message {
                inner: item,
                context: self.context.clone(),
            }))),
            Some(Err(e)) => Poll::Ready(Some(Err(crate::Error::Ws(e)))),
            None => {
                log::trace!("websocket closed");
                Poll::Ready(None)
            }
        }
    }
}

impl<T> Sink<Message<T>> for WebSocket<T> {
    type Error = crate::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match ready!(Pin::new(&mut self.inner).poll_ready(cx)) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(crate::Error::Ws(e))),
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message<T>) -> Result<(), Self::Error> {
        match Pin::new(&mut self.inner).start_send(item.inner) {
            Ok(()) => Ok(()),
            Err(e) => {
                log::debug!("websocket start_send error: {}", e);
                Err(crate::Error::Ws(e))
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match ready!(Pin::new(&mut self.inner).poll_flush(cx)) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(crate::Error::Ws(e))),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match ready!(Pin::new(&mut self.inner).poll_close(cx)) {
            Ok(()) => Poll::Ready(Ok(())),
            Err(err) => {
                log::debug!("websocket close error: {}", err);
                Poll::Ready(Err(crate::Error::Ws(err)))
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
