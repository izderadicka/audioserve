use super::response::ResponseResult;
use super::RequestWrapper;
use crate::config::get_config;
use crate::error::{bail, Context, Error};
use crate::services::response::box_websocket_response;
use collection::audio_meta::TimeStamp;
use collection::{Collections, Position};

use serde::Serialize;
use std::str::FromStr;
use std::sync::Arc;
use websock::{self as ws, spawn_websocket};
use ws::{Message, MessageResult};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct Location {
    collection: usize,
    group: String,
    path: String,
}

// This is workaround to match old websocket API
#[derive(Debug, Serialize)]
struct PositionCompatible {
    file: String,
    folder: String,
    timestamp: TimeStamp,
    position: f32,
}

impl From<Position> for PositionCompatible {
    fn from(p: Position) -> Self {
        PositionCompatible {
            file: p.file,
            timestamp: p.timestamp,
            position: p.position,
            folder: p.collection.to_string() + "/" + &p.folder,
        }
    }
}

impl FromStr for Location {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(3, '/');
        let group = parts
            .next()
            .ok_or_else(|| Error::msg("Missing group part"))?;
        let collection: usize = parts
            .next()
            .ok_or_else(|| Error::msg("Missin collection num"))?
            .parse()
            .context("Invalid collection number")?;
        let path = parts.next().unwrap_or("");
        Ok(Location {
            group: group.into(),
            collection,
            path: path.into(),
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
enum Msg {
    Position {
        position: f32,
        file_path: Option<Location>,
        timestamp: Option<u64>,
    },
    FolderQuery {
        folder_path: Location,
    },
    GenericQuery {
        group: String,
    },
}

#[derive(Serialize)]
struct Reply {
    folder: Option<PositionCompatible>,
    last: Option<PositionCompatible>,
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
                    file_path: Some(parts[1].parse()?),
                    timestamp,
                })
            }
        } else if parts.len() == 1 {
            if parts[0].find('/').is_some() {
                Ok(Msg::FolderQuery {
                    folder_path: parts[0].parse()?,
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

struct Ctx {
    col: Arc<Collections>,
    loc: Location,
}

async fn process_message(m: Message, ctx: &mut Ctx) -> MessageResult {
    debug!("Got message {:?}", m);
    let message = m.to_str().map_err(Error::new).and_then(str::parse);
    let col = ctx.col.clone();
    match message {
        Ok(message) => match message {
            Msg::Position {
                position,
                file_path,
                timestamp,
            } => match file_path {
                Some(file_loc) => {
                    ctx.loc = file_loc.clone();
                    if let Some(ts) = timestamp {
                        let position = Position {
                            timestamp: (ts * 1000).into(), // timestamp in WS message is in seconds!
                            collection: file_loc.collection,
                            folder: String::new(),
                            file: file_loc.path,
                            folder_finished: false,
                            position,
                        };
                        col.insert_position_if_newer_async(file_loc.group, position)
                            .await
                    } else {
                        col.insert_position_async(
                            file_loc.collection,
                            file_loc.group,
                            file_loc.path,
                            position,
                            false,
                        )
                        .await
                    }
                    .unwrap_or_else(|e| error!("Cannot insert position: {}", e));
                    Ok(None)
                }

                None => {
                    let prev = ctx.loc.clone();

                    if !prev.path.is_empty() {
                        col.insert_position_async(
                            prev.collection,
                            prev.group,
                            prev.path,
                            position,
                            false,
                        )
                        .await
                        .unwrap_or_else(|e| error!("Cannot insert position: {}", e))
                    } else {
                        error!("Client sent short position, but there is no context");
                    }

                    Ok(None)
                }
            },
            Msg::GenericQuery { group } => {
                let last = col.get_last_position_async(group).await;
                let res = Reply {
                    folder: None,
                    last: last.map(PositionCompatible::from),
                };

                Ok(Some(ws::Message::text(
                    serde_json::to_string(&res).unwrap(),
                )))
            }

            Msg::FolderQuery { folder_path } => {
                let last = col
                    .clone()
                    .get_last_position_async(folder_path.group.clone())
                    .await;
                let folder = col
                    .get_position_async(folder_path.collection, folder_path.group, folder_path.path)
                    .await;
                let res = Reply {
                    last: if last != folder {
                        last.map(PositionCompatible::from)
                    } else {
                        None
                    },
                    folder: folder.map(PositionCompatible::from),
                };

                Ok(Some(ws::Message::text(
                    serde_json::to_string(&res).unwrap(),
                )))
            }
        },
        Err(e) => {
            error!("Position message error: {}", e);
            Ok(None)
        }
    }
}

pub fn position_service(req: RequestWrapper, col: Arc<Collections>) -> ResponseResult {
    debug!("We got these headers on websocket: {:?}", req.headers());
    let res = spawn_websocket(
        req.into_request(),
        process_message,
        Ctx {
            col,
            loc: Location::default(),
        },
        Some(get_config().positions.ws_timeout),
    );

    Ok(box_websocket_response(res))
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_position_location() {
        let l = Location {
            group: "group".into(),
            collection: 1,
            path: "".into(),
        };
        let l1: Location = "group/1".parse().expect("valid path");
        assert_eq!(l.clone(), l1);
        let l2: Location = "group/1/".parse().expect("valid path");
        assert_eq!(l.clone(), l2);
    }

    #[test]
    fn test_position_msg() {
        let m1: Msg = "123.1|group/0/book1/chap1".parse().unwrap();
        let loc = Location {
            group: "group".into(),
            collection: 0,
            path: "book1/chap1".into(),
        };
        assert_eq!(
            Msg::Position {
                position: 123.1,
                file_path: Some(loc.clone()),
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

        let m5: Msg = "group/1/book1".parse().unwrap();
        let loc5 = Location {
            group: "group".into(),
            collection: 1,
            path: "book1".into(),
        };
        assert_eq!(Msg::FolderQuery { folder_path: loc5 }, m5);

        let m6 = "aaa|bbb".parse::<Msg>();
        let m7 = "||".parse::<Msg>();
        assert!(m6.is_err());
        assert!(m7.is_err());

        let m8: Msg = "123.1|group/0/book1/chap1|123456".parse().unwrap();
        assert_eq!(
            m8,
            Msg::Position {
                position: 123.1,
                file_path: Some(loc),
                timestamp: Some(123456)
            }
        );

        let m9 = "123.1||123456".parse::<Msg>();
        assert!(m9.is_err());
    }
}
