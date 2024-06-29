use headers::{Header, HeaderName, HeaderValue};
use lazy_static::lazy_static;
use log::{error, warn};
use parser::{all_string, elements, full_string, values_list};
use std::{
    fmt::Display,
    iter,
    net::{AddrParseError, IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6},
    str::Utf8Error,
};

mod parser;

lazy_static! {
    static ref X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
}

#[derive(Debug)]
pub enum AddrError<'a> {
    InvalidlyQuoted,
    InvalidAddress,
    InvalidIdentity,
    InvalidString(Utf8Error),
    ParserError(parser::Error<'a>),
    SocketInsteadIp,
}

impl<'a> From<AddrError<'a>> for headers::Error {
    fn from(_: AddrError) -> Self {
        headers::Error::invalid()
    }
}

impl<'a> From<AddrParseError> for AddrError<'a> {
    fn from(_: AddrParseError) -> Self {
        AddrError::InvalidAddress
    }
}

impl<'a> From<Utf8Error> for AddrError<'a> {
    fn from(e: Utf8Error) -> Self {
        AddrError::InvalidString(e)
    }
}

impl<'a> From<parser::Error<'a>> for AddrError<'a> {
    fn from(e: parser::Error<'a>) -> Self {
        AddrError::ParserError(e)
    }
}

impl From<parser::StringError> for headers::Error {
    fn from(_: parser::StringError) -> Self {
        headers::Error::invalid()
    }
}

enum IpOrSocket {
    Ip(IpAddr),
    Socket(SocketAddr),
    #[allow(dead_code)] //TODO: support for obfuscated ports
    SocketWithObfuscatedPort(IpAddr, Obfuscated),
}

impl From<IpOrSocket> for IpAddr {
    fn from(addr: IpOrSocket) -> Self {
        match addr {
            IpOrSocket::Ip(addr) => addr,
            IpOrSocket::Socket(s) => s.ip(),
            IpOrSocket::SocketWithObfuscatedPort(addr, _) => addr,
        }
    }
}

impl From<IpOrSocket> for NodeIdentifier {
    fn from(addr: IpOrSocket) -> Self {
        match addr {
            IpOrSocket::Ip(addr) => NodeIdentifier {
                name: NodeName::Addr(addr),
                port: None,
            },
            IpOrSocket::Socket(addr) => NodeIdentifier {
                name: NodeName::Addr(addr.ip()),
                port: Some(Port::Real(addr.port())),
            },
            IpOrSocket::SocketWithObfuscatedPort(addr, o) => NodeIdentifier {
                name: NodeName::Addr(addr),
                port: Some(Port::Obfuscated(o)),
            },
        }
    }
}

impl IpOrSocket {
    fn to_ip_only<'a>(&self) -> Result<IpAddr, AddrError<'a>> {
        match self {
            IpOrSocket::Ip(addr) => Ok(*addr),
            _ => Err(AddrError::SocketInsteadIp),
        }
    }
}
fn parse_ip(s: &str) -> Result<IpOrSocket, AddrError> {
    if s.starts_with('[') {
        if s.ends_with(']') {
            // this should be IPv6 address
            // thanks to previous test it's guaranteed that indexes are on ut8 boundaries
            let ip6: Ipv6Addr = s[1..s.len() - 1].parse()?;
            Ok(IpOrSocket::Ip(IpAddr::V6(ip6)))
        } else {
            //it still can be IPv6 socket address
            match s.parse::<SocketAddrV6>() {
                Ok(a) => Ok(IpOrSocket::Socket(SocketAddr::V6(a))),
                Err(_e) => {
                    Err(AddrError::InvalidAddress)
                    //TBD:  It can have obfuscated port
                }
            }
        }
    } else {
        s.parse::<IpAddr>()
            .map(IpOrSocket::Ip)
            .or_else(|_| s.parse::<SocketAddr>().map(IpOrSocket::Socket))
            .map_err(AddrError::from)
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
            NodeName::Addr(a) => a.fmt(f),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Port {
    Real(u16),
    Obfuscated(Obfuscated),
}

impl Display for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Port::Real(p) => p.fmt(f),
            Port::Obfuscated(o) => o.fmt(f),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct NodeIdentifier {
    pub name: NodeName,
    pub port: Option<Port>,
}

impl Display for NodeIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.port.as_ref() {
            None => write!(f, "{}", self.name),
            Some(port) => match self.name {
                NodeName::Addr(IpAddr::V6(a)) => write!(f, "[{}]:{}", a, port),
                _ => write!(f, "{}:{}", self.name, port),
            },
        }
    }
}

impl NodeIdentifier {
    pub fn ip(&self) -> Option<&IpAddr> {
        match self.name {
            NodeName::Addr(ref a) => Some(a),
            _ => None,
        }
    }

    pub fn port(&self) -> Option<u16> {
        match self.port {
            Some(Port::Real(p)) => Some(p),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ForwardNode {
    pub fwd_for: Option<NodeIdentifier>,
    pub fwd_by: Option<NodeIdentifier>,
    pub fwd_host: Option<Host>,
    pub fwd_protocol: Option<Protocol>,
}

pub struct Forwarded {
    nodes: Vec<ForwardNode>,
}

impl Forwarded {
    pub fn client(&self) -> Option<&IpAddr> {
        self.nodes
            .first()
            .and_then(|n| n.fwd_for.as_ref())
            .and_then(|i| i.ip())
    }
    pub fn client_port(&self) -> Option<u16> {
        self.nodes
            .first()
            .and_then(|n| n.fwd_for.as_ref())
            .and_then(|i| i.port())
    }

    pub fn client_protocol(&self) -> Option<&Protocol> {
        self.nodes.first().and_then(|n| n.fwd_protocol.as_ref())
    }
}

impl Header for Forwarded {
    fn name() -> &'static HeaderName {
        &http::header::FORWARDED
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i HeaderValue>,
    {
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
                let mut node = ForwardNode {
                    fwd_for: None,
                    fwd_by: None,
                    fwd_host: None,
                    fwd_protocol: None,
                };
                for (key, value) in elem {
                    match &key.to_ascii_lowercase()[..] {
                        b"for" => {
                            let id = if value.starts_with(b"_") {
                                // this should be obfuscated identifier
                                let o = full_string(&value, parser::obs).map_err(|_| {
                                    error!("Invalid obfuscated id: {:?}", value);
                                    headers::Error::invalid()
                                })?;
                                NodeIdentifier {
                                    name: NodeName::Obfuscated(Obfuscated(o)),
                                    port: None,
                                }
                            } else if value.to_ascii_lowercase() == b"unknown" {
                                // unknown
                                NodeIdentifier {
                                    name: NodeName::Unknown,
                                    port: None,
                                }
                            } else {
                                // or default is IP/Socket address
                                let n = all_string(&value).map_err(|_| {
                                    error!("from key value is not valid string");
                                    headers::Error::invalid()
                                })?;
                                parse_ip(n).map(Into::into).map_err(|e| {
                                    error!("Invalid address {:?}", e);
                                    e
                                })?
                            };

                            if node.fwd_for.is_none() {
                                node.fwd_for = Some(id);
                            } else {
                                error!("Duplicate key for");
                                return Err(headers::Error::invalid());
                            }
                        }
                        b"by" => {}
                        b"host" => {
                            let host = full_string(&value, parser::host).map_err(|_| {
                                error!("Invalid host value in Forwarded header");
                                headers::Error::invalid()
                            })?;
                            if node.fwd_host.is_none() {
                                node.fwd_host = Some(Host(host))
                            } else {
                                error!("Duplicate host key");
                                return Err(headers::Error::invalid());
                            }
                        }
                        b"proto" => {
                            let proto = full_string(&value, parser::scheme).map_err(|_| {
                                error!("Invalid proto value in Forwarded header");
                                headers::Error::invalid()
                            })?;
                            if node.fwd_protocol.is_none() {
                                node.fwd_protocol = Some(Protocol(proto))
                            } else {
                                error!("Duplicate proto key");
                                return Err(headers::Error::invalid());
                            }
                        }
                        other => warn!("Unknown key in Forwarded node {:?} ", other),
                    }
                }

                nodes.push(node);
            }
        }

        if nodes.is_empty() {
            error!("Forwarded header is empty");
            return Err(headers::Error::invalid());
        }

        Ok(Forwarded { nodes })
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
        self.ips
            .first()
            .expect("at least one record is alway present")
    }

    pub fn proxies(&self) -> impl Iterator<Item = &IpAddr> {
        self.ips.iter().skip(1)
    }
}

impl Header for XForwardedFor {
    fn name() -> &'static HeaderName {
        &X_FORWARDED_FOR
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

            for p in parts {
                let s = std::str::from_utf8(p.as_ref()).map_err(|e| {
                    error!("Invalid string {}", e);
                    headers::Error::invalid()
                })?;
                let addr = parse_ip(s).and_then(|a| a.to_ip_only());
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

    use log::debug;

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
            r#"For="[2001:db8:cafe::17]:4711", For=192.0.2.43:47011"#,
        ];

        for (n, h) in headers.iter().enumerate() {
            let v = HeaderValue::from_str(h)
                .unwrap_or_else(|_| panic!("Cannot create header value for #{}", n));
            let mut i = iter::once(&v);
            let fwd = Forwarded::decode(&mut i)
                .unwrap_or_else(|_| panic!("Failed decode header {}: {}", n, headers[n]));
            match n {
                0 => {
                    assert_eq!(fwd.nodes.len(), 1, "first case has just one node");
                    assert_eq!(fwd.client().unwrap(), &IpAddr::from([123, 34, 167, 89]));
                }
                1 => {
                    assert_eq!(fwd.nodes.len(), 2, "second case has two nodes");
                    assert_eq!(fwd.client().unwrap(), &IpAddr::from([192, 0, 2, 43]));
                    assert_eq!(
                        fwd.nodes[1].fwd_for,
                        Some(NodeIdentifier {
                            name: NodeName::Addr("2001:db8:cafe::17".parse().unwrap()),
                            port: None
                        })
                    );
                }
                2 => {
                    assert_eq!(fwd.nodes.len(), 2, "third case has two nodes");
                    let n = &fwd.nodes[1];
                    assert_eq!(n.fwd_protocol.as_ref().unwrap().as_ref(), "http");
                    assert_eq!(n.fwd_host.as_ref().unwrap().as_ref(), "example.com");
                }
                3 => {
                    assert_eq!(fwd.nodes.len(), 3, "fourth case has three nodes");
                    let n = &fwd.nodes[2];
                    debug!("Third node {:?}", n);
                    assert!(matches!(
                        n.fwd_for,
                        Some(NodeIdentifier {
                            name: NodeName::Unknown,
                            port: None
                        })
                    ));
                }
                4 => {
                    assert_eq!(fwd.nodes.len(), 2, "fifth case has two nodes");
                    let n = &fwd.nodes[1];
                    assert_eq!(
                        n.fwd_for.as_ref().unwrap().name,
                        NodeName::Obfuscated(Obfuscated("_SEVKISEK".into()))
                    )
                }
                5 => {
                    assert_eq!(fwd.nodes.len(), 2, "sixth case has two nodes");
                    let p = |idx: usize| {
                        fwd.nodes[idx]
                            .fwd_for
                            .as_ref()
                            .unwrap()
                            .port
                            .as_ref()
                            .unwrap()
                            .clone()
                    };
                    assert_eq!(p(0), Port::Real(4711));
                    assert_eq!(p(1), Port::Real(47011));
                    let addr: IpAddr = "2001:db8:cafe::17".parse().unwrap();
                    assert_eq!(fwd.client(), Some(&addr));
                    assert_eq!(fwd.client_port(), Some(4711));
                }
                _ => {}
            }
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
