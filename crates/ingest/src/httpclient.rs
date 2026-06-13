//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::ProviderError;
use governor::state::NotKeyed;
use governor::{clock::DefaultClock, state::InMemoryState, Quota, RateLimiter};
use reqwest::{Client, RequestBuilder, Response};
use std::num::NonZeroU32;
use std::sync::Arc;

/// Thread-safe rate-limited HTTP client
#[derive(Clone)]
pub struct RateLimitedClient {
    limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    client: Client,
}

impl RateLimitedClient {
    /// Create with requests per second
    pub fn per_second(client: Client, requests: u32) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(requests).unwrap());
        Self::new_with_quota(client, quota)
    }

    /// Create with requests per minute
    pub fn per_minute(client: Client, requests: u32) -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(requests).unwrap());
        Self::new_with_quota(client, quota)
    }

    /// Create custom quota
    pub fn new_with_quota(client: Client, quota: Quota) -> Self {
        Self {
            client,
            limiter: Arc::new(RateLimiter::direct(quota)),
        }
    }

    /// Execute a prepared request (rate limited)
    pub async fn execute(&self, request: RequestBuilder) -> Result<Response, ProviderError> {
        // Wait until we’re allowed to proceed
        self.limiter.until_ready().await;

        let response = request.send().await?;

        Ok(response)
    }

    /// Convenience GET
    pub async fn get(&self, url: &str) -> Result<Response, ProviderError> {
        self.limiter.until_ready().await;
        Ok(self.client.get(url).send().await?)
    }

    /// Convenience POST
    pub async fn post<T: serde::Serialize + ?Sized>(
        &self,
        url: &str,
        body: &T,
    ) -> Result<Response, ProviderError> {
        self.limiter.until_ready().await;
        Ok(self.client.post(url).json(body).send().await?)
    }

    /// Access inner client if needed
    pub fn inner(&self) -> &Client {
        &self.client
    }
}
