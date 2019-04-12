use hyper::{Request,Body};
use websock::{spawn_websocket, self as ws};
use futures::future;
use super::ResponseFuture;
use std::str::FromStr;
use cache::{Cache, Position};

mod cache;

lazy_static! {
    static ref CACHE: Cache = Cache::new(100);
}

pub fn save_positions() {
    if let Err(e) =  CACHE.save() {
        error!("Cannot save positions to file: {}", e);
    }
}

#[derive(Clone,PartialEq, Debug)]
enum Msg {
    Position {
        position: f32,
        file_path: Option<String>
    },
    FolderQuery {
        folder_path: String
    },
    GenericQuery
}

#[derive(Serialize)]
struct Reply {
    folder: Option<Position>,
    last: Option<Position>
}


impl FromStr for Msg {
    type Err = &'static str;

    fn from_str(s:&str) -> Result<Self,Self::Err> {
        let parts:Vec<_> = s.split('|').collect();
        if parts.len() == 2 {
            let position: f32 = parts[0].parse().map_err(|_| "Position is not a number")?;
            if parts[1].len() == 0 {
                Ok(Msg::Position{position, file_path:None})
            } else {
                Ok(Msg::Position{position, file_path: Some(parts[1].into())})
            }

        } else if parts.len() == 1 {
            if parts[0].is_empty() || parts[0] == "?" {
                Ok(Msg::GenericQuery)
            } else {
                Ok(Msg::FolderQuery{folder_path:parts[0].into()})
            }
        } else {
            Err("Too many |")
        }
    }
}



pub fn position_service(req: Request<Body>) -> ResponseFuture {
    debug!("We got these headers: {:?}", req.headers());

    let res = spawn_websocket::<String,_>(req, |m| {
        debug!("Got message {:?}", m);
        let message = m.to_str()
        .map_err(|_| "Invalid ws message")
        .and_then(|s| s.parse::<Msg>());

        match message {
            Ok(message) => {
                match message {
                    Msg::Position{position, file_path} => {

                        match file_path {
                            Some(file_path) => {
                                {
                                let mut p = m.context_ref().write().unwrap();
                                *p = file_path.clone();
                                }
                                CACHE.insert(file_path, position)

                            }

                            None => {
                                let prev = {
                                    m.context_ref().read().unwrap().clone()
                                };

                                if ! prev.is_empty() {
                                    CACHE.insert(prev, position)
                                } else {
                                    error!("Client sent short position, but there is no context");
                                }
                            }
                        };

                        Box::new(future::ok(None))

                    }
                    Msg::GenericQuery => {
                        let last = CACHE.get_last();
                        let res = Reply {
                            folder: None,
                            last
                        };

                        Box::new(
                        future::ok(
                            Some(ws::Message::text(serde_json::to_string(&res).unwrap(), m.context()))
                        )
        )


                    }

                    Msg::FolderQuery{folder_path} => {

                        let last = CACHE.get_last();
                        let folder = CACHE.get(&folder_path);
                        let res = Reply {
                            last: if last != folder {last} else {None},
                            folder
                        };

                        Box::new(
                        future::ok(
                            Some(ws::Message::text(serde_json::to_string(&res).unwrap(), m.context()))
                        )
                        )

                    }
                }
            },
            Err(e) => {
                error!("Position message error: {}", e);
                Box::new(future::ok(None))
            }
        }
        

        
    });

    Box::new(future::ok(res))
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_position_msg() {

        let m1:Msg = "123.1|book1/chap1".parse().unwrap();
        assert_eq!(Msg::Position{position:123.1, file_path: Some("book1/chap1".into())}, m1);
        let m2:Msg = "123.1|".parse().unwrap();
        assert_eq!(Msg::Position{position:123.1, file_path:None}, m2);
        let m3:Msg = "".parse().unwrap();
        assert_eq!(Msg::GenericQuery, m3);
        let m4:Msg = "?".parse().unwrap();
        assert_eq!(Msg::GenericQuery, m4);
        let m5:Msg = "book1".parse().unwrap();
        assert_eq!(Msg::FolderQuery{folder_path:"book1".into()}, m5);

        let m6 = "aaa|bbb".parse::<Msg>();
        let m7 = "||".parse::<Msg>();
        assert!(m6.is_err());
        assert!(m7.is_err());


    }
}
