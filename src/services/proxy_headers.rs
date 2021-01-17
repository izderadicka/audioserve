use std::{
    fmt::Display,
    iter,
    net::{AddrParseError, IpAddr, Ipv6Addr},
};

use headers::{Header, HeaderName, HeaderValue};
use hyper::http::header;

lazy_static! {
    static ref X_FORWARED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
}

enum AddrError {
    InvalidlyQuoted,
    InvalidAddress,
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

// assumes that str is acsii, other can panic, but this should be assured by HeaderValue
fn unquote_str(s: &str, start: char, end: char) -> Result<Option<&str>, AddrError> {
    if s.starts_with(start) {
        if s.len() > 1 && s.ends_with(end) {
            return Ok(Some(&s[1..s.len() - 1]));
        } else {
            return Err(AddrError::InvalidlyQuoted);
        }
    }
    Ok(None)
}

fn parse_ip(s: &str) -> Result<IpAddr, AddrError> {
    if let Some(quoted) = unquote_str(s, '"', '"')? {
        if let Some(addr) = unquote_str(quoted, '[', ']')? {
            let ip6: Ipv6Addr = addr.parse()?;
            Ok(IpAddr::V6(ip6))
        } else {
            Err(AddrError::InvalidlyQuoted)
        }
    } else {
        s.parse().map_err(AddrError::from)
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum NodeName {
    Unknown,
    Obfuscated(String),
    Addr(IpAddr),
}

impl Display for NodeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeName::Unknown => {
                write!(f, "unknown")
            }
            NodeName::Obfuscated(s) => f.write_str(s),
            NodeName::Addr(a) => {
                write!(f, "{}", a)
            }
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
            let parts = val
                .to_str()
                .map_err(|_| headers::Error::invalid())?
                .split(",");
            let addrs = parts.map(|p| parse_ip(p.trim()));
            for addr in addrs {
                match addr {
                    Ok(a) => ips.push(a),
                    Err(_) => return Err(headers::Error::invalid()),
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
    fn test_decode_x_forwarded_for() {
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
