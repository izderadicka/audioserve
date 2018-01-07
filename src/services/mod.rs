use hyper::server::{NewService, Request, Response, Service};
use hyper::{self,Method, StatusCode};
use hyper::header::{Range,AccessControlAllowOrigin, AccessControlAllowCredentials, Cookie, 
Authorization, Bearer, ContentType, Origin,
ContentLength, SetCookie};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use self::subs::{send_file, short_response_boxed, short_response,ResponseFuture, NOT_FOUND_MESSAGE, get_folder};
use std::path::{PathBuf, Path};
use percent_encoding::percent_decode;
use futures::{Future, Stream, future};
use url::form_urlencoded;
use std::collections::HashMap;

mod subs;
mod types;


const OVERLOADED_MESSAGE: &str = "Overloaded, try later";

type Counter = Arc<AtomicUsize>;

pub trait Authenticator {
    type Credentials;
    fn authenticate(&self, req: Request) -> Box<Future<Item=Result<(Request,Self::Credentials), Response>, Error=hyper::Error>>;
}

#[derive(Clone)]
pub struct SharedSecretAuthenticator {
    shared_secret: String,
    my_secret: Vec<u8>

}

impl SharedSecretAuthenticator {
    pub fn new(shared_secret: String, my_secret: Vec<u8>) -> Self {
        SharedSecretAuthenticator{
            shared_secret,
            my_secret
        }
    }
}

const COOKIE_NAME: &str = "audioserve_token";
const ACCESS_DENIED: &str = "Access denied";

type AuthResult = Result<(Request,()), Response>;
type AuthFuture = Box<Future<Item=AuthResult, Error=hyper::Error>>;
impl Authenticator for SharedSecretAuthenticator {
    type Credentials = ();
    fn authenticate(&self, req: Request) -> AuthFuture {
        fn deny() -> AuthResult {
            Err(short_response(StatusCode::Unauthorized, ACCESS_DENIED))
        }
        // this is part where client can authenticate itself and get token
        if req.method() == &Method::Post && req.path()=="/authenticate" {
            let auth = self.clone();
            return Box::new(req.body().concat2().map(move |b| {
                    
                let params = form_urlencoded::parse(b.as_ref()).into_owned()
                .collect::<HashMap<String, String>>();
                    
                if let Some(secret) = params.get("secret") {
                        debug!("Authenticating user");
                        if auth.auth_token_ok(secret) {
                            debug!("Authentication success");
                            let token = auth.new_auth_token();
                            Err(Response::new()
                                .with_header(ContentType::plaintext())
                                .with_header(ContentLength(token.len() as u64))
                                .with_header(SetCookie(vec![format!("{}={}; Max-Age={}",COOKIE_NAME, token,10*365*24*3600)]))
                                .with_body(token)
                            )
                        } else {
                           deny()
                        }
                        
                    } else {
                         deny()
                        
                    }
            
            }));
        };
        // And in this part we check token
        {
            let mut token = req.headers().get::<Authorization<Bearer>>().map(|h| h.0.token.as_str()) ;
            if token.is_none() {
                token = req.headers().get::<Cookie>().and_then(|h| h.get(COOKIE_NAME));
            }
            
            if token.is_none() || ! self.token_ok(token.unwrap()) {
                return Box::new(future::ok(deny()))
            } 
        }
        // If everything is ok we return credentials (in this case they are just unit type) and we return back request
        Box::new(future::ok(Ok((req,()))))
    }
}

impl SharedSecretAuthenticator {
    fn auth_token_ok(&self, token: &str) -> bool{
        self.shared_secret == token
    }
    fn new_auth_token(&self) -> String {
        ::std::str::from_utf8(&self.my_secret).unwrap().into()
    }

    fn token_ok(&self, token: &str) -> bool {
        self.my_secret == token.as_bytes()
    }
}

pub struct Factory {
    pub sending_threads: Counter,
    pub max_threads: usize,
    pub base_dir: PathBuf,
    pub authenticator: Arc<Box<Authenticator<Credentials=()>>>
}

impl NewService for Factory {
    type Request = Request;
    type Response = Response;
    type Error = ::hyper::Error;
    type Instance = FileSendService;

    fn new_service(&self) -> Result<Self::Instance, io::Error> {
        Ok(FileSendService {
            sending_threads: self.sending_threads.clone(),
            max_threads: self.max_threads,
            base_dir: self.base_dir.clone(),
            authenticator: self.authenticator.clone()
        })
    }
}
pub struct FileSendService {
    sending_threads: Counter,
    max_threads: usize,
    base_dir: PathBuf,
    pub authenticator: Arc<Box<Authenticator<Credentials=()>>>
}

// use only on checked prefixes
fn get_subfolder(path: &str, prefix: &str) -> PathBuf {
    Path::new(&path).strip_prefix(prefix).unwrap().to_path_buf()
}

fn add_cors_headers(resp: Response, origin: Option<String>) -> Response {
    match origin {
        Some(o) =>
            resp.with_header(AccessControlAllowOrigin::Value(o))
            .with_header(AccessControlAllowCredentials),
        None => resp
    }
}

impl Service for FileSendService {
    type Request = Request;
    type Response = Response;
    type Error = ::hyper::Error;
    type Future = ResponseFuture;

    fn call(&self, req: Self::Request) -> Self::Future {
        if self.sending_threads.load(Ordering::SeqCst) > self.max_threads {
                    warn!("Server is busy, refusing request");
                    return short_response_boxed(
                        StatusCode::ServiceUnavailable,
                        OVERLOADED_MESSAGE,
                    );
        };
        let base_dir = self.base_dir.clone();
        let sending_threads =  self.sending_threads.clone();
        let origin = req.headers().get::<Origin>().map(|o| {
            format!("{}",o)
            }
            );
        Box::new(self.authenticator.authenticate(req).and_then(move |result| {
            match result {
                Ok((req,_creds)) => FileSendService::process_checked(req, base_dir,sending_threads),
                Err(resp) => Box::new(future::ok(resp))
            }.map(|r| add_cors_headers(r, origin))
        }))
        
    }
}

impl FileSendService {
    fn process_checked(req: Request, base_dir: PathBuf, sending_threads: Counter) -> ResponseFuture {

        match req.method() {
            &Method::Get => {
                let path = percent_decode(req.path().as_bytes()).decode_utf8_lossy().into_owned();
                
                if path.starts_with("/audio/") {
                debug!("Received request with following headers {}", req.headers());

                let range = req.headers().get::<Range>();
                let bytes_range = match range {
                    Some(&Range::Bytes(ref bytes_ranges)) => {
                        if bytes_ranges.len() < 1 {
                            return short_response_boxed(StatusCode::BadRequest, "One range is required")
                        } else if bytes_ranges.len() > 1 {
                            return short_response_boxed(StatusCode::NotImplemented, "Do not support muptiple ranges")
                        } else {
                            Some(bytes_ranges[0].clone())
                        }
                    },
                    Some(_) => return short_response_boxed(StatusCode::NotImplemented, 
                    "Other then bytes ranges are not supported"),
                    None => None
                };

                send_file(base_dir, 
                    get_subfolder(&path, "/audio/"), 
                    bytes_range, 
                    sending_threads)
                } else if path.starts_with("/folder/") {
                    get_folder(base_dir, 
                    get_subfolder(&path, "/folder/"),  
                    sending_threads) 
                } else {
                    short_response_boxed(StatusCode::NotFound, NOT_FOUND_MESSAGE)
                }
            },

            _ => short_response_boxed(StatusCode::MethodNotAllowed, "Method not supported"),
        }
    }
}
