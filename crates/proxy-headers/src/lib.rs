use headers::{Header, HeaderName, HeaderValue};
use http::header;
use lazy_static::lazy_static;
use log::{error, warn};
use parser::{all_string, elements, full_string, quoted_string, values_list};
use std::{borrow::Cow, fmt::Display, iter, net::{AddrParseError, IpAddr, Ipv6Addr}, str::{FromStr, Utf8Error}};

mod parser;

lazy_static! {
    static ref X_FORWARED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
}

#[derive(Debug)]
pub enum AddrError {
    InvalidlyQuoted,
    InvalidAddress,
    InvalidIdentity,
    InvalidString(Utf8Error),
    ParserError,
}

impl From<AddrError> for headers::Error {
    fn from(_: AddrError) -> Self {
        headers::Error::invalid()
    }
}

impl From<AddrParseError> for AddrError {
    fn from(_: AddrParseError) -> Self {
        AddrError::InvalidAddress
    }
}

impl From<Utf8Error> for AddrError {
    fn from(e: Utf8Error) -> Self {
        AddrError::InvalidString(e)
    }
}

impl<'a> From<parser::Error<'a>> for AddrError {
    fn from(_: parser::Error<'a>) -> Self {
        AddrError::ParserError
    }
}

impl From<parser::StringError> for headers::Error {
    fn from(_: parser::StringError) -> Self {
        headers::Error::invalid()
    }
}

fn parse_ip(addr: &[u8]) -> Result<IpAddr, AddrError> {
    let s = std::str::from_utf8(addr)?;
    if s.starts_with('[') && s.ends_with(']') {
        // thanks to previous test it's guaranteed that string
        let ip6: Ipv6Addr = s[1..s.len() - 1].parse()?;
        Ok(IpAddr::V6(ip6))
    } else {
        s.parse().map_err(AddrError::from)
    }
}

macro_rules!  string_newtype {
    ($($t:ident),*) => {
$(
#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct $t(String);

impl AsRef<str> for $t {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl Display for $t {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
)*
        
    };
}

string_newtype!(Obfuscated, Host, Protocol);


#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum NodeName {
    Unknown,
    Obfuscated(Obfuscated),
    Addr(IpAddr),
}

impl Display for NodeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeName::Unknown => {
                write!(f, "unknown")
            }
            NodeName::Obfuscated(s) => f.write_str(s.as_ref()),
            NodeName::Addr(a) => {
                write!(f, "{}", a)
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Port {
    Real(u16),
    Obfuscated(Obfuscated),
}

impl Display for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Port::Real(p) => p.fmt(f),
            Port::Obfuscated(o) => o.fmt(f)
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct NodeIdentifier {
    name: NodeName,
    port: Option<u16>,
}

impl Display for NodeIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.port {
            None => write!(f, "{}", self.name),
            Some(port) => match self.name {
                NodeName::Addr(IpAddr::V6(a)) => write!(f, "[{}]:{}", a, port),
                _ => write!(f, "{}:{}", self.name, port),
            },
        }
    }
}


#[derive(Debug, PartialEq, Eq)]
pub struct ForwardNode {
    fwd_for: Option<NodeIdentifier>,
    fwd_by: Option<NodeIdentifier>,
    fwd_host: Option<Host>,
    fwd_protocol: Option<Protocol>
    

}

pub struct Forwarded {
    nodes: Vec<ForwardNode>
}

impl Header for Forwarded {
    fn name() -> &'static HeaderName {
        &http::header::FORWARDED
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i HeaderValue> {
        let mut nodes = Vec::new();

        for v in values {
            let (left, elements) = elements(v.as_bytes()).map_err(|e| {
                error!("error parsing Forwarded header: {}", e);
                headers::Error::invalid()
            })?;
            if !left.is_empty() {
                error!("cannot parse full Forwarded header");
                return Err(headers::Error::invalid());
            }

            for elem in elements {
                let mut node  = ForwardNode {
                    fwd_for: None,
                    fwd_by: None,
                    fwd_host: None,
                    fwd_protocol: None
                };
                for (key, value) in elem {
                    match &key.to_ascii_lowercase()[..] {
                        b"for" => {
                            let n = all_string(&value)?;
                            let id = if n.starts_with('[') {
                                // this should be IPv6
                                todo!()
                            } else if n.starts_with('_'){
                                // this should be obfuscated identifier
                                NodeName::Obfuscated(Obfuscated(n.into()))
                                
                            } else if n.to_ascii_lowercase() == "unknown" {
                                // unknown
                                NodeName::Unknown
                            }
                            else {
                                // or default is IPv4
                                let addr: IpAddr = parse_ip(n.as_bytes()).map_err(|e| {
                                    error!("Invalid address {:?}", e);
                                    e
                                })?;
                                NodeName::Addr(addr)
                            };

                            if node.fwd_for.is_none() {
                                node.fwd_for = Some(NodeIdentifier{name: id, port: None})
                            } else {
                                error!("Duplicate key for");
                                return Err(headers::Error::invalid())
                            }



                        }
                        b"by" => {

                        }
                        b"host" => {
                            let host = full_string(&value, parser::host)?;
                            if node.fwd_host.is_none() {
                                node.fwd_host = Some(Host(host))
                            } else {
                                error!("Duplicate host key");
                                return Err(headers::Error::invalid())
                            }

                        }
                        b"protocol" => {

                        }
                        other => warn!("Unknown key in Forwarded node {:?} ", other)
                    }
                }

                nodes.push(node);
            }
        }

        if nodes.is_empty() {
            error!("Forwarded header is empty");
            return Err(headers::Error::invalid());
        }

        Ok(Forwarded{nodes})
    }

    fn encode<E: Extend<HeaderValue>>(&self, _values: &mut E) {
        todo!()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct XForwardedFor {
    ips: Vec<IpAddr>,
}

impl XForwardedFor {
    pub fn client(&self) -> &IpAddr {
        &self
            .ips
            .get(0)
            .expect("at least one record is alway present")
    }

    pub fn proxies(&self) -> impl Iterator<Item = &IpAddr> {
        self.ips.iter().skip(1)
    }
}

impl Header for XForwardedFor {
    fn name() -> &'static HeaderName {
        &X_FORWARED_FOR
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i headers::HeaderValue>,
    {
        let mut ips = Vec::new();
        for val in values {
            let (left, parts) = values_list(val.as_bytes()).map_err(|e| {
                error!("Invalid header value {:?}", e);
                headers::Error::invalid()
            })?;
            if !left.is_empty() {
                error!("Unparsed part of header {:?}", left);
                headers::Error::invalid();
            }
            let addrs = parts.into_iter().map(|p| parse_ip(p.as_ref()));
            for addr in addrs {
                match addr {
                    Ok(a) => ips.push(a),
                    Err(e) => {
                        error!("Invalid IP address: {:?}", e);
                        return Err(headers::Error::invalid());
                    }
                }
            }
        }

        if ips.is_empty() {
            return Err(headers::Error::invalid());
        }

        Ok(XForwardedFor { ips })
    }

    fn encode<E: Extend<headers::HeaderValue>>(&self, values: &mut E) {
        let s = self
            .ips
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        values.extend(iter::once(
            HeaderValue::from_maybe_shared(s)
                .expect("BUG: ips should be always valid header value"),
        ))
    }
}

#[cfg(test)]
mod test {
    use std::net::Ipv6Addr;

    use super::*;

    #[test]
    fn test_decode_forwarded() {
        env_logger::try_init().ok();
        let headers = &[
            "for=123.34.167.89",
            r#"for=192.0.2.43, for="[2001:db8:cafe::17]""#,
            r#"for=192.0.2.43,for=198.51.100.17;by=203.0.113.60;proto=http;host=example.com"#,
            r#"for=192.0.2.43, for="[2001:db8:cafe::17]", for=unknown"#,
            r#"for=_hidden, for=_SEVKISEK"#,
            r#"Forwarded: For="[2001:db8:cafe::17]:4711", For=192.0.2.43:47011"#
        ];

        for (n,h) in headers.into_iter().enumerate() {
            let v = HeaderValue::from_str(h).expect(&format!("Cannot create header value for #{}", n));
            let mut i = iter::once(&v);
            let fwd = Forwarded::decode(&mut i).expect(&format!("Failed decode header {}: {}",n, headers[n]));

        }
    }

    #[test]
    fn test_decode_x_forwarded_for() {
        env_logger::try_init().ok();
        let header1 = "2001:db8:85a3:8d3:1319:8a2e:370:7348";
        let header2 = "203.0.113.195";
        let header3 = "203.0.113.195, 70.41.3.18, 150.172.238.178";
        let header4 = "192.0.2.43, \"[2001:db8:cafe::17]\"";
        let proxy4: Ipv6Addr = "2001:db8:cafe::17".parse().unwrap();

        fn value_to_header(s: &str) -> Result<XForwardedFor, headers::Error> {
            let v = HeaderValue::from_str(s).unwrap();
            let mut iter = std::iter::once(&v);
            XForwardedFor::decode(&mut iter)
        }

        let h1 = value_to_header(header1).unwrap();
        assert_eq!(h1.client(), &header1.parse::<IpAddr>().unwrap());

        let h2 = value_to_header(header2).unwrap();
        assert_eq!(h2.client(), &header2.parse::<IpAddr>().unwrap());

        let h3 = value_to_header(header3).unwrap();
        assert_eq!(h3.client(), &header2.parse::<IpAddr>().unwrap());
        let proxies: Vec<_> = h3.proxies().collect();
        assert_eq!(proxies.len(), 2);

        let h4 = value_to_header(header4).unwrap();
        assert_eq!(h4.proxies().next().unwrap(), &proxy4);
    }

    #[test]
    fn test_encode_x_forwarded_for() {
        let header = "203.0.113.195, 70.41.3.18, 150.172.238.178";
        let hv = HeaderValue::from_str(header).unwrap();
        let mut v = iter::once(&hv);
        let h = XForwardedFor::decode(&mut v).unwrap();
        let mut values = Vec::new();
        h.encode(&mut values);
        let header2 = values[0].to_str().unwrap();
        assert_eq!(header, header2);
    }
}
