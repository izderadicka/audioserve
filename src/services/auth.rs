use super::subs::short_response;
use data_encoding::BASE64;
use crate::error::Error;
use futures::{future, Future, Stream};
use hyper::header::{SET_COOKIE};
use hyper::{Body, Method, Request, Response, StatusCode};
use headers::{Authorization, Cookie, HeaderMapExt, ContentType, ContentLength};
use crate::util::ResponseBuilderExt;
use headers::{authorization::Bearer};
use ring::digest::{digest, SHA256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use url::form_urlencoded;

type AuthResult<T> = Result<(Request<Body>, T), Response<Body>>;
type AuthFuture<T> = Box<Future<Item = AuthResult<T>, Error = Error> + Send>;

pub trait Authenticator: Send + Sync {
    type Credentials;
    fn authenticate(&self, req: Request<Body>) -> AuthFuture<Self::Credentials>;
}

#[derive(Clone)]
pub struct SharedSecretAuthenticator {
    shared_secret: String,
    my_secret: Vec<u8>,
    token_validity_hours: u64,
}

impl SharedSecretAuthenticator {
    pub fn new(shared_secret: String, my_secret: Vec<u8>, token_validity_hours: u64) -> Self {
        SharedSecretAuthenticator {
            shared_secret,
            my_secret,
            token_validity_hours,
        }
    }
}

const COOKIE_NAME: &str = "audioserve_token";
const ACCESS_DENIED: &str = "Access denied";

impl Authenticator for SharedSecretAuthenticator {
    type Credentials = ();
    fn authenticate(&self, req: Request<Body>) -> AuthFuture<()> {
        fn deny() -> AuthResult<()> {
            Err(short_response(StatusCode::UNAUTHORIZED, ACCESS_DENIED))
        }
        // this is part where client can authenticate itself and get token
        if req.method() == Method::POST && req.uri().path() == "/authenticate" {
            debug!("Authentication request");
            let auth = self.clone();
            return Box::new(
                req.into_body()
                    .concat2()
                    .map_err(Error::new_with_cause)
                    .map(move |b| {
                        let params = form_urlencoded::parse(b.as_ref())
                            .into_owned()
                            .collect::<HashMap<String, String>>();
                        if let Some(secret) = params.get("secret") {
                            debug!("Authenticating user");
                            if auth.auth_token_ok(secret) {
                                debug!("Authentication success");
                                let token = auth.new_auth_token();
                                Err(Response::builder()
                                    .typed_header(ContentType::text())
                                    .typed_header(ContentLength(token.len() as u64))
                                    .header(
                                        SET_COOKIE,
                                        format!(
                                            "{}={}; Max-Age={}",
                                            COOKIE_NAME,
                                            token,
                                            10 * 365 * 24 * 3600
                                        )
                                        .as_str(),
                                    )
                                    .body(token.into())
                                    .unwrap())
                            } else {
                                deny()
                            }
                        } else {
                            deny()
                        }
                    }),
            );
        } else {
            // And in this part we check token
            let mut token = 
                req.headers().typed_get::<Authorization<Bearer>>()
                .map(|a| a.0.token().to_owned());
            if token.is_none() {
                token = req
                    .headers()
                    .typed_get::<Cookie>()
                    .and_then(|c| c.get(COOKIE_NAME).map(|v| v.to_owned()));
            }

            if token.is_none() || !self.token_ok(&token.unwrap()) {
                return Box::new(future::ok(deny()));
            }
        }
        // If everything is ok we return credentials (in this case they are just unit type) and we return back request
        Box::new(future::ok(Ok((req, ()))))
    }
}

impl SharedSecretAuthenticator {
    fn auth_token_ok(&self, token: &str) -> bool {
        let parts = token
            .split('|')
            .map(|s| BASE64.decode(s.as_bytes()))
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();
        if parts.len() == 2 {
            let mut hash2 = self.shared_secret.clone().into_bytes();
            let hash = &parts[1];
            hash2.extend(&parts[0]);
            let hash2 = digest(&SHA256, &hash2);

            return hash2.as_ref() == &hash[..];
        } else {
            error!("Incorrectly formed login token - {} parts", parts.len())
        }
        false
    }
    fn new_auth_token(&self) -> String {
        Token::new(self.token_validity_hours, &self.my_secret).into()
    }

    fn token_ok(&self, token: &str) -> bool {
        match token.parse::<Token>() {
            Ok(token) => token.is_valid(&self.my_secret),
            Err(e) => {
                warn!("Invalid token: {}", e);
                false
            }
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
struct Token {
    random: [u8; 32],
    validity: [u8; 8],
    signature: [u8; 32],
}

fn prepare_data(r: &[u8; 32], v: [u8; 8]) -> [u8; 40] {
    let mut to_sign = [0u8; 40];
    to_sign[0..32].copy_from_slice(&r[..]);
    to_sign[32..40].copy_from_slice(&v[..]);
    to_sign
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Invalid system time")
        .as_secs()
}

impl Token {
    fn new(token_validity_hours: u64, secret: &[u8]) -> Self {
        let mut random = [0u8; 32];
        let rng = SystemRandom::new();
        rng.fill(&mut random)
            .expect("Cannot generate random number");
        let validity: u64 = now() + token_validity_hours * 3600;
        let validity: [u8; 8] = unsafe { ::std::mem::transmute(validity.to_be()) };
        let to_sign = prepare_data(&random, validity);
        let key = hmac::SigningKey::new(&SHA256, secret);
        let sig = hmac::sign(&key, &to_sign);
        let slice = sig.as_ref();
        assert!(slice.len() == 32);
        let mut signature = [0u8; 32];
        signature.copy_from_slice(slice);

        Token {
            random,
            validity,
            signature,
        }
    }

    fn is_valid(&self, secret: &[u8]) -> bool {
        let key = hmac::VerificationKey::new(&SHA256, secret);
        let data = prepare_data(&self.random, self.validity);
        if hmac::verify(&key, &data, &self.signature).is_err() {
            return false;
        };

        self.validity() > now()
    }

    fn validity(&self) -> u64 {
        let ts: u64 = unsafe { ::std::mem::transmute_copy(&self.validity) };
        u64::from_be(ts)
    }
}

impl Into<String> for Token {
    fn into(self) -> String {
        let data = [&self.random[..], &self.validity[..], &self.signature[..]].concat();
        BASE64.encode(&data)
    }
}

quick_error! {
    #[derive(Debug, PartialEq)]
    enum TokenError {
        InvalidSize { }
        InvalidEncoding(error: ::data_encoding::DecodeError) {
            from()
        }

    }
}

impl ::std::str::FromStr for Token {
    type Err = TokenError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = BASE64.decode(s.as_bytes())?;
        if bytes.len() != 72 {
            return Err(TokenError::InvalidSize);
        };
        let mut random = [0u8; 32];
        let mut validity = [0u8; 8];
        let mut signature = [0u8; 32];

        random.copy_from_slice(&bytes[0..32]);
        validity.copy_from_slice(&bytes[32..40]);
        signature.copy_from_slice(&bytes[40..72]);

        Ok(Token {
            random,
            validity,
            signature,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token() {
        let token = Token::new(24, b"my big secret");
        assert!(token.is_valid(b"my big secret"));
        let orig_token = token.clone();
        let serialized_token: String = token.into();
        assert!(serialized_token.len() >= 72);
        let new_token: Token = serialized_token.parse().unwrap();
        assert_eq!(orig_token, new_token);
        assert!(new_token.is_valid(b"my big secret"));
        assert!(!new_token.is_valid(b"wrong secret"));
        assert!(new_token.validity() - now() <= 24 * 3600);
    }
}
