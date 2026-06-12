
use std::{collections::HashSet, sync::Arc};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use governor::{
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter as GovernorRL,
};
use std::num::NonZeroU32;
use std::net::SocketAddr;

pub type RateLimiter = Arc<GovernorRL<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>;

pub fn build(rps: u32) -> RateLimiter {
    let rps = NonZeroU32::new(rps).unwrap_or(NonZeroU32::new(60).unwrap());
    Arc::new(GovernorRL::direct(Quota::per_second(rps)))
}

pub async fn middleware(
    State((limiter, trusted)): State<(RateLimiter, HashSet<String>)>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let ip = addr.ip().to_string();
    if !trusted.contains(&ip) {
        limiter.check().map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    }
    Ok(next.run(request).await)
}
