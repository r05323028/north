use axum::{
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");
use std::{
    collections::HashMap,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CidrParseError;

impl fmt::Display for CidrParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid IP CIDR")
    }
}

impl std::error::Error for CidrParseError {}

/// Canonical IP network used for proxy trust and durable limiter keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IpCidr {
    network: IpAddr,
    prefix_len: u8,
}

impl IpCidr {
    pub fn new(network: IpAddr, prefix_len: u8) -> Result<Self, CidrParseError> {
        let network = normalize_ip(network);
        let max_prefix = match network {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix_len > max_prefix {
            return Err(CidrParseError);
        }
        Ok(Self {
            network: mask_ip(network, prefix_len),
            prefix_len,
        })
    }

    pub fn contains(&self, address: IpAddr) -> bool {
        let address = normalize_ip(address);
        match (self.network, address) {
            (IpAddr::V4(network), IpAddr::V4(address)) => {
                mask_ip(IpAddr::V4(address), self.prefix_len) == IpAddr::V4(network)
            }
            (IpAddr::V6(network), IpAddr::V6(address)) => {
                mask_ip(IpAddr::V6(address), self.prefix_len) == IpAddr::V6(network)
            }
            _ => false,
        }
    }

    pub fn primary_key(address: IpAddr) -> Self {
        let address = normalize_ip(address);
        let prefix_len = match address {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 64,
        };
        Self::new(address, prefix_len).expect("family-specific CIDR prefix is valid")
    }

    pub fn network(&self) -> IpAddr {
        self.network
    }

    pub fn prefix_len(&self) -> u8 {
        self.prefix_len
    }
}

impl FromStr for IpCidr {
    type Err = CidrParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (address, prefix) = value.split_once('/').ok_or(CidrParseError)?;
        let original = address.parse::<IpAddr>().map_err(|_| CidrParseError)?;
        let mut prefix_len = prefix.parse::<u8>().map_err(|_| CidrParseError)?;
        let address = match original {
            IpAddr::V6(address) => match address.to_ipv4() {
                Some(address) => {
                    if prefix_len < 96 {
                        return Err(CidrParseError);
                    }
                    prefix_len -= 96;
                    IpAddr::V4(address)
                }
                None => IpAddr::V6(address),
            },
            IpAddr::V4(address) => IpAddr::V4(address),
        };
        Self::new(address, prefix_len)
    }
}

impl fmt::Display for IpCidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix_len)
    }
}

pub fn normalize_ip(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V4(address) => IpAddr::V4(address),
        IpAddr::V6(address) => address
            .to_ipv4()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(address)),
    }
}

fn mask_ip(address: IpAddr, prefix_len: u8) -> IpAddr {
    match address {
        IpAddr::V4(address) => {
            let bits = u32::from(address);
            let mask = if prefix_len == 0 {
                0
            } else {
                u32::MAX << (32 - prefix_len)
            };
            IpAddr::V4(Ipv4Addr::from(bits & mask))
        }
        IpAddr::V6(address) => {
            let bits = u128::from(address);
            let mask = if prefix_len == 0 {
                0
            } else {
                u128::MAX << (128 - prefix_len)
            };
            IpAddr::V6(Ipv6Addr::from(bits & mask))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    pub address: IpAddr,
    pub primary_key: String,
}

pub fn resolve_client_identity(
    peer: SocketAddr,
    headers: &HeaderMap,
    trusted_proxy_cidrs: &[IpCidr],
) -> ClientIdentity {
    let immediate_peer = normalize_ip(peer.ip());
    let effective_address = if trusted_proxy_cidrs
        .iter()
        .any(|network| network.contains(immediate_peer))
    {
        parse_forwarded_chain(headers)
            .and_then(|chain| {
                chain.into_iter().rev().find(|address| {
                    !trusted_proxy_cidrs
                        .iter()
                        .any(|network| network.contains(*address))
                })
            })
            .unwrap_or(immediate_peer)
    } else {
        immediate_peer
    };
    let primary_key = IpCidr::primary_key(effective_address).to_string();
    ClientIdentity {
        address: effective_address,
        primary_key,
    }
}

fn parse_forwarded_chain(headers: &HeaderMap) -> Option<Vec<IpAddr>> {
    let values = headers.get_all(X_FORWARDED_FOR);
    let mut chain = Vec::new();
    let mut found = false;
    for value in values.iter() {
        found = true;
        let value = value.to_str().ok()?;
        for token in value.split(',') {
            let token = token.trim();
            if token.is_empty() {
                return None;
            }
            chain.push(normalize_ip(token.parse().ok()?));
        }
    }
    found.then_some(chain).filter(|chain| !chain.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PublicEndpoint {
    RequestCode,
    DaemonSetup,
}

impl PublicEndpoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RequestCode => "request_code",
            Self::DaemonSetup => "daemon_setup",
        }
    }
}

#[derive(Clone)]
pub struct PublicEndpointConfig {
    pub trusted_proxy_cidrs: Vec<IpCidr>,
    pub bucket_capacity: u32,
    pub bucket_refill_interval: Duration,
}

impl Default for PublicEndpointConfig {
    fn default() -> Self {
        Self {
            trusted_proxy_cidrs: Vec::new(),
            bucket_capacity: 5,
            bucket_refill_interval: Duration::from_secs(120),
        }
    }
}

impl PublicEndpointConfig {
    pub fn with_trusted_proxy_cidrs(trusted_proxy_cidrs: Vec<IpCidr>) -> Self {
        Self {
            trusted_proxy_cidrs,
            ..Self::default()
        }
    }

    pub fn from_trusted_proxy_strings<I, S>(values: I) -> Result<Self, CidrParseError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        values
            .into_iter()
            .map(|value| value.as_ref().parse())
            .collect::<Result<Vec<_>, _>>()
            .map(Self::with_trusted_proxy_cidrs)
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Clone)]
pub struct ClientRateLimiter {
    state: Arc<Mutex<LimiterState>>,
    clock: Arc<dyn Clock>,
    capacity: f64,
    refill_interval: Duration,
}

struct LimiterState {
    buckets: HashMap<BucketKey, TokenBucket>,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    updated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BucketKey {
    endpoint: PublicEndpoint,
    primary_key: String,
}

impl ClientRateLimiter {
    pub fn new(capacity: u32, refill_interval: Duration) -> Self {
        Self::with_clock(capacity, refill_interval, Arc::new(SystemClock))
    }

    pub fn with_clock(capacity: u32, refill_interval: Duration, clock: Arc<dyn Clock>) -> Self {
        assert!(capacity > 0, "client bucket capacity must be positive");
        assert!(
            refill_interval > Duration::ZERO,
            "client bucket refill must be positive"
        );
        Self {
            state: Arc::new(Mutex::new(LimiterState {
                buckets: HashMap::new(),
            })),
            clock,
            capacity: f64::from(capacity),
            refill_interval,
        }
    }

    pub fn reserve(
        &self,
        endpoint: PublicEndpoint,
        primary_key: impl Into<String>,
    ) -> Result<RateLimitPermit, u64> {
        let key = BucketKey {
            endpoint,
            primary_key: primary_key.into(),
        };
        let now = self.clock.now();
        let mut state = self.state.lock().expect("client limiter mutex poisoned");
        let bucket = state.buckets.entry(key.clone()).or_insert(TokenBucket {
            tokens: self.capacity,
            updated_at: now,
        });
        self.refill(bucket, now);
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(RateLimitPermit {
                limiter: self.clone(),
                key,
                committed: false,
            })
        } else {
            let seconds = ((1.0 - bucket.tokens) * self.refill_interval.as_secs_f64()).ceil();
            Err((seconds as u64).max(1))
        }
    }

    fn refill(&self, bucket: &mut TokenBucket, now: Instant) {
        let elapsed = now.saturating_duration_since(bucket.updated_at);
        if elapsed.is_zero() {
            return;
        }
        bucket.tokens = (bucket.tokens
            + elapsed.as_secs_f64() / self.refill_interval.as_secs_f64())
        .min(self.capacity);
        bucket.updated_at = now;
    }

    fn refund(&self, key: &BucketKey) {
        let now = self.clock.now();
        let mut state = self.state.lock().expect("client limiter mutex poisoned");
        let bucket = state.buckets.entry(key.clone()).or_insert(TokenBucket {
            tokens: self.capacity,
            updated_at: now,
        });
        self.refill(bucket, now);
        bucket.tokens = (bucket.tokens + 1.0).min(self.capacity);
    }
}

pub struct RateLimitPermit {
    limiter: ClientRateLimiter,
    key: BucketKey,
    committed: bool,
}

impl RateLimitPermit {
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for RateLimitPermit {
    fn drop(&mut self) {
        if !self.committed {
            self.limiter.refund(&self.key);
        }
    }
}

#[derive(Clone)]
pub struct PublicEndpointState {
    config: Arc<PublicEndpointConfig>,
    limiter: ClientRateLimiter,
}

impl PublicEndpointState {
    pub fn new(config: PublicEndpointConfig) -> Self {
        let limiter = ClientRateLimiter::new(config.bucket_capacity, config.bucket_refill_interval);
        Self::with_limiter(config, limiter)
    }

    pub fn with_limiter(config: PublicEndpointConfig, limiter: ClientRateLimiter) -> Self {
        Self {
            config: Arc::new(config),
            limiter,
        }
    }

    pub fn identity(&self, peer: SocketAddr, headers: &HeaderMap) -> ClientIdentity {
        resolve_client_identity(peer, headers, &self.config.trusted_proxy_cidrs)
    }

    pub fn reserve(
        &self,
        endpoint: PublicEndpoint,
        primary_key: impl Into<String>,
    ) -> Result<RateLimitPermit, u64> {
        self.limiter.reserve(endpoint, primary_key)
    }

    pub fn observe(&self, endpoint: PublicEndpoint, outcome: &'static str, category: &'static str) {
        eprintln!(
            "public_endpoint endpoint={} outcome={} category={} count=1",
            endpoint.as_str(),
            outcome,
            category
        );
    }
}

#[derive(Debug, Serialize)]
struct RateLimitedBody {
    error: &'static str,
}

pub fn rate_limited_response(retry_after: u64) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(RateLimitedBody {
            error: "rate_limited",
        }),
    )
        .into_response();
    let retry_after = retry_after.max(1).to_string();
    if let Ok(value) = HeaderValue::from_str(&retry_after) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn socket(address: &str) -> SocketAddr {
        let value = if address.contains(':') {
            format!("[{address}]:443")
        } else {
            format!("{address}:443")
        };
        value.parse().expect("socket address")
    }

    fn cidr(value: &str) -> IpCidr {
        value.parse().expect("CIDR")
    }

    #[test]
    fn primary_keys_normalize_mapped_and_prefix_bits() {
        let v4 = resolve_client_identity(socket("192.0.2.7"), &HeaderMap::new(), &[]);
        let mapped = resolve_client_identity(socket("::ffff:192.0.2.7"), &HeaderMap::new(), &[]);
        assert_eq!(v4, mapped);
        assert_eq!(v4.primary_key, "192.0.2.7/32");

        let first = IpCidr::primary_key("2001:db8:1:2:3::1".parse().unwrap());
        let second = IpCidr::primary_key("2001:db8:1:2:ffff::1".parse().unwrap());
        let different = IpCidr::primary_key("2001:db8:1:3::1".parse().unwrap());
        assert_eq!(first, second);
        assert_ne!(first, different);
    }

    #[test]
    fn untrusted_forwarding_header_is_ignored() {
        let mut headers = HeaderMap::new();
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("192.0.2.7"));
        let identity = resolve_client_identity(socket("198.51.100.9"), &headers, &[]);
        assert_eq!(identity.address, "198.51.100.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_chain_selects_first_untrusted_address_from_right() {
        let trusted = vec![cidr("10.0.0.0/8")];
        let mut headers = HeaderMap::new();
        headers.insert(
            X_FORWARDED_FOR,
            HeaderValue::from_static("198.51.100.7, 10.1.1.1, 10.2.2.2"),
        );
        let identity = resolve_client_identity(socket("10.3.3.3"), &headers, &trusted);
        assert_eq!(identity.address, "198.51.100.7".parse::<IpAddr>().unwrap());
        assert_eq!(identity.primary_key, "198.51.100.7/32");
    }

    #[test]
    fn malformed_empty_and_all_trusted_chains_fall_back_to_peer() {
        let trusted = vec![cidr("10.0.0.0/8")];
        for header_value in ["", "198.51.100.7, not-an-ip", "10.1.1.1,10.2.2.2"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                X_FORWARDED_FOR,
                HeaderValue::from_str(header_value).expect("header value"),
            );
            let identity = resolve_client_identity(socket("10.3.3.3"), &headers, &trusted);
            assert_eq!(identity.address, "10.3.3.3".parse::<IpAddr>().unwrap());
        }
    }

    #[test]
    fn duplicate_forwarding_fields_are_joined_in_wire_order() {
        let trusted = vec![cidr("10.0.0.0/8")];
        let mut headers = HeaderMap::new();
        headers.append(X_FORWARDED_FOR, HeaderValue::from_static("198.51.100.7"));
        headers.append(X_FORWARDED_FOR, HeaderValue::from_static("10.1.1.1"));
        let identity = resolve_client_identity(socket("10.2.2.2"), &headers, &trusted);
        assert_eq!(identity.address, "198.51.100.7".parse::<IpAddr>().unwrap());
    }

    #[derive(Clone)]
    struct ManualClock {
        now: Arc<Mutex<Instant>>,
    }

    impl ManualClock {
        fn new() -> Self {
            Self {
                now: Arc::new(Mutex::new(Instant::now())),
            }
        }

        fn advance(&self, duration: Duration) {
            *self.now.lock().expect("clock mutex") += duration;
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            *self.now.lock().expect("clock mutex")
        }
    }

    #[test]
    fn buckets_refill_and_are_endpoint_isolated() {
        let clock = Arc::new(ManualClock::new());
        let limiter = ClientRateLimiter::with_clock(5, Duration::from_secs(120), clock.clone());
        for _ in 0..5 {
            limiter
                .reserve(PublicEndpoint::RequestCode, "192.0.2.7/32")
                .expect("capacity")
                .commit();
        }
        assert!(matches!(
            limiter.reserve(PublicEndpoint::RequestCode, "192.0.2.7/32"),
            Err(120)
        ));
        limiter
            .reserve(PublicEndpoint::DaemonSetup, "192.0.2.7/32")
            .expect("endpoint isolation")
            .commit();
        clock.advance(Duration::from_secs(120));
        limiter
            .reserve(PublicEndpoint::RequestCode, "192.0.2.7/32")
            .expect("refill")
            .commit();
    }

    #[test]
    fn dropped_permit_refunds_client_bucket() {
        let limiter = ClientRateLimiter::new(1, Duration::from_secs(120));
        let permit = limiter
            .reserve(PublicEndpoint::RequestCode, "192.0.2.7/32")
            .expect("capacity");
        drop(permit);
        limiter
            .reserve(PublicEndpoint::RequestCode, "192.0.2.7/32")
            .expect("refund")
            .commit();
    }

    #[test]
    fn concurrent_reservations_cannot_bypass_capacity() {
        let limiter = ClientRateLimiter::new(5, Duration::from_secs(120));
        let successes = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..32 {
                let limiter = limiter.clone();
                let successes = successes.clone();
                scope.spawn(move || {
                    if let Ok(permit) = limiter.reserve(PublicEndpoint::RequestCode, "192.0.2.7/32")
                    {
                        permit.commit();
                        successes.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(successes.load(Ordering::Relaxed), 5);
    }
}
