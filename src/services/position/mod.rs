use super::{RequestWrapper, ResponseFuture};
use crate::config::get_config;
use crate::error::{bail, Context, Error};
use cache::{Cache, Position};
use futures::future;
use std::str::FromStr;
use websock::{self as ws, spawn_websocket_with_timeout};

mod cache;

lazy_static! {
    static ref CACHE: Cache = Cache::new(100, 100);
}

pub async fn save_positions() {
    if let Err(e) = CACHE.save().await {
        error!("Cannot save positions to file: {}", e);
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Msg {
    Position {
        position: f32,
        file_path: Option<String>,
        timestamp: Option<u64>,
    },
    FolderQuery {
        folder_path: String,
    },
    GenericQuery {
        group: String,
    },
}

#[derive(Serialize)]
struct Reply {
    folder: Option<Position>,
    last: Option<Position>,
}

impl FromStr for Msg {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split('|').collect();
        let mut timestamp: Option<u64> = None;
        if parts.len() == 3 {
            timestamp = Some(parts[2].parse().context("Invalid timestamp")?);
            if parts[1].is_empty() {
                return Err(Error::msg(
                    "If timestamp is present, then also file path must be present",
                ));
            }
        }
        if parts.len() >= 2 && parts.len() <= 3 {
            let position: f32 = parts[0].parse().context("Position is not a number")?;
            if parts[1].is_empty() {
                Ok(Msg::Position {
                    position,
                    file_path: None,
                    timestamp,
                })
            } else {
                Ok(Msg::Position {
                    position,
                    file_path: Some(parts[1].into()),
                    timestamp,
                })
            }
        } else if parts.len() == 1 {
            if parts[0].find('/').is_some() {
                Ok(Msg::FolderQuery {
                    folder_path: parts[0].into(),
                })
            } else {
                Ok(Msg::GenericQuery {
                    group: parts[0].into(),
                })
            }
        } else {
            bail!("Too many |")
        }
    }
}

pub fn position_service(req: RequestWrapper) -> ResponseFuture {
    debug!("We got these headers on websocket: {:?}", req.headers());

    let res = spawn_websocket_with_timeout::<String, _>(
        req.into_request(),
        |m| {
            debug!("Got message {:?}", m);
            let message = m.to_str().map_err(Error::new).and_then(str::parse);

            match message {
                Ok(message) => Box::pin(async {
                    Ok(match message {
                        Msg::Position {
                            position,
                            file_path,
                            timestamp,
                        } => {
                            match file_path {
                                Some(file_path) => {
                                    {
                                        let mut p = m.context_ref().write().await;
                                        *p = file_path.clone();
                                    }
                                    if let Some(ts) = timestamp {
                                        CACHE.insert_if_newer(file_path, position, ts).await
                                    } else {
                                        CACHE.insert(file_path, position).await
                                    }
                                    .unwrap_or_else(|e| error!("Cannot insert position: {}", e))
                                }

                                None => {
                                    let prev = { m.context_ref().read().await.clone() };

                                    if !prev.is_empty() {
                                        CACHE.insert(prev, position).await.unwrap_or_else(|e| {
                                            error!("Cannot insert position: {}", e)
                                        })
                                    } else {
                                        error!(
                                            "Client sent short position, but there is no context"
                                        );
                                    }
                                }
                            };

                            None
                        }
                        Msg::GenericQuery { group } => {
                            let last = CACHE.get_last(group).await;
                            let res = Reply { folder: None, last };

                            Some(ws::Message::text(
                                serde_json::to_string(&res).unwrap(),
                                m.context(),
                            ))
                        }

                        Msg::FolderQuery { folder_path } => {
                            let group = Some(folder_path.splitn(2, '/')).and_then(|mut p| p.next());
                            let last = CACHE.get_last(group.unwrap()).await;
                            let folder = CACHE.get(&folder_path).await;
                            let res = Reply {
                                last: if last != folder { last } else { None },
                                folder,
                            };

                            Some(ws::Message::text(
                                serde_json::to_string(&res).unwrap(),
                                m.context(),
                            ))
                        }
                    })
                }),
                Err(e) => {
                    error!("Position message error: {}", e);
                    Box::pin(future::ok(None))
                }
            }
        },
        get_config().positions_ws_timeout,
    );

    Box::pin(future::ok(res))
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_position_msg() {
        let m1: Msg = "123.1|group/book1/chap1".parse().unwrap();
        assert_eq!(
            Msg::Position {
                position: 123.1,
                file_path: Some("group/book1/chap1".into()),
                timestamp: None
            },
            m1
        );
        let m2: Msg = "123.1|".parse().unwrap();
        assert_eq!(
            Msg::Position {
                position: 123.1,
                file_path: None,
                timestamp: None
            },
            m2
        );
        let m3: Msg = "group".parse().unwrap();
        assert_eq!(
            Msg::GenericQuery {
                group: "group".into()
            },
            m3
        );

        let m5: Msg = "group/book1".parse().unwrap();
        assert_eq!(
            Msg::FolderQuery {
                folder_path: "group/book1".into()
            },
            m5
        );

        let m6 = "aaa|bbb".parse::<Msg>();
        let m7 = "||".parse::<Msg>();
        assert!(m6.is_err());
        assert!(m7.is_err());

        let m8: Msg = "123.1|group/book1/chap1|123456".parse().unwrap();
        assert_eq!(
            m8,
            Msg::Position {
                position: 123.1,
                file_path: Some("group/book1/chap1".into()),
                timestamp: Some(123456)
            }
        );

        let m9 = "123.1||123456".parse::<Msg>();
        assert!(m9.is_err());
    }
}
