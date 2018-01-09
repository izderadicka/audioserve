use hyper::{self,Request, Response, Method, StatusCode};
use hyper::header::{ContentLength, ContentType, SetCookie, Cookie, Authorization, Bearer};
use futures::{Future,Stream, future};
use url::form_urlencoded;
use std::collections::HashMap;
use super::subs::short_response;

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