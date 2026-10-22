//! Public Vision bytes. The client refuses any host other than the Vision
//! download host and the public S3 listing host.

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use url::Url;

use crate::vision::{list_page, list_url, ListedObject};
use crate::Error;

const VISION_HOST: &str = "data.binance.vision";
const LIST_HOST: &str = "s3-ap-northeast-1.amazonaws.com";

pub trait BlobGet: Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, Error>;
}

pub struct MapBlob {
    objects: HashMap<String, Vec<u8>>,
}

impl MapBlob {
    pub fn new(objects: HashMap<String, Vec<u8>>) -> Self {
        Self { objects }
    }
}

impl BlobGet for MapBlob {
    fn get(&self, url: &str) -> Result<Vec<u8>, Error> {
        self.objects.get(url).cloned().ok_or_else(|| Error::Http {
            url: url.to_owned(),
            detail: "not in the fixture map".into(),
        })
    }
}

pub struct HttpGet {
    client: reqwest::blocking::Client,
}

impl HttpGet {
    pub fn new() -> Result<Self, Error> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("parangsae-collect/0.1")
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|err| Error::Http {
                url: VISION_HOST.to_owned(),
                detail: err.to_string(),
            })?;
        Ok(Self { client })
    }
}

impl BlobGet for HttpGet {
    fn get(&self, url: &str) -> Result<Vec<u8>, Error> {
        require_public_host(url)?;
        let response = self.client.get(url).send().map_err(|err| Error::Http {
            url: url.to_owned(),
            detail: err.to_string(),
        })?;
        if !response.status().is_success() {
            return Err(Error::Http {
                url: url.to_owned(),
                detail: response.status().to_string(),
            });
        }
        let bytes = response.bytes().map_err(|err| Error::Http {
            url: url.to_owned(),
            detail: err.to_string(),
        })?;
        Ok(bytes.to_vec())
    }
}

pub fn require_public_host(url: &str) -> Result<(), Error> {
    let parsed = Url::parse(url).map_err(|err| Error::check("host", err.to_string()))?;
    match parsed.host_str() {
        Some(VISION_HOST) | Some(LIST_HOST) => Ok(()),
        Some(host) => Err(Error::check("host", format!("refusing {host}"))),
        None => Err(Error::check("host", "url has no host")),
    }
}

/// How long the caller should wait before the next request.
pub fn wait_budget(earliest: Instant, now: Instant) -> Option<Duration> {
    if now >= earliest {
        None
    } else {
        Some(earliest.saturating_duration_since(now))
    }
}

pub struct Pace {
    min_gap: Duration,
    next_ok: Mutex<Instant>,
}

impl Pace {
    pub fn new(min_gap: Duration) -> Self {
        Self {
            min_gap,
            next_ok: Mutex::new(Instant::now()),
        }
    }

    pub fn wait(&self) -> Result<(), Error> {
        let mut next_ok = self
            .next_ok
            .lock()
            .map_err(|_| Error::check("pace", "lock poisoned"))?;
        let now = Instant::now();
        if let Some(delay) = wait_budget(*next_ok, now) {
            *next_ok += self.min_gap;
            drop(next_ok);
            thread::sleep(delay);
        } else {
            *next_ok = now + self.min_gap;
        }
        Ok(())
    }
}

pub fn list_prefix<G: BlobGet>(getter: &G, prefix: &str) -> Result<Vec<ListedObject>, Error> {
    let mut marker = String::new();
    let mut all = Vec::new();
    for _ in 0..100 {
        let url = list_url(prefix, &marker)?;
        require_public_host(&url)?;
        let bytes = getter.get(&url)?;
        let text = String::from_utf8(bytes).map_err(|err| Error::Xml(err.to_string()))?;
        let page = list_page(&text)?;
        if page.objects.is_empty() {
            if page.truncated {
                return Err(Error::Xml("truncated listing page is empty".into()));
            }
            break;
        }
        let next = page
            .objects
            .last()
            .map(|object| object.key.clone())
            .ok_or_else(|| Error::Xml("listing page lost its last key".into()))?;
        if page.truncated && next == marker {
            return Err(Error::Xml("listing marker did not advance".into()));
        }
        let truncated = page.truncated;
        all.extend(page.objects);
        if !truncated {
            break;
        }
        marker = next;
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_budget_is_zero_once_the_earliest_instant_has_passed() {
        let earliest = Instant::now();
        assert_eq!(wait_budget(earliest, earliest), None);
        assert!(wait_budget(earliest + Duration::from_millis(10), earliest).is_some());
    }

    #[test]
    fn other_hosts_are_refused() -> Result<(), String> {
        let err = match require_public_host("https://example.com/market") {
            Err(err) => err,
            Ok(_) => return Err("expected a host error".to_string()),
        };
        assert_eq!(err.check_name(), Some("host"));
        require_public_host("https://data.binance.vision/data/spot/BTCUSDT.zip")
            .map_err(|err| err.to_string())?;
        require_public_host(
            "https://s3-ap-northeast-1.amazonaws.com/data.binance.vision/?prefix=data/",
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }
}
