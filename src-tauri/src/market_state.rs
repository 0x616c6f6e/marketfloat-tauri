use crate::{
    market::{self, History, Instrument, Quote, SourceGroup},
    settings::{Period, Proxy, Settings},
};
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    quote: Option<Quote>,
    samples: VecDeque<f64>,
    error: Option<String>,
}

type HistoryResult = Result<History, String>;

pub struct MarketData {
    pub client: Client,
    pub revision: u64,
    rows: HashMap<String, Row>,
    history: HashMap<(String, Period), (Instant, HistoryResult)>,
    refreshing: bool,
}

impl MarketData {
    pub fn new(proxy: &Proxy) -> Result<Self, String> {
        Ok(Self {
            client: market::client(proxy)?,
            revision: 0,
            rows: HashMap::new(),
            history: HashMap::new(),
            refreshing: false,
        })
    }

    pub fn invalidate_quotes(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn use_client(&mut self, client: Client) {
        self.client = client;
        self.history.clear();
        self.invalidate_quotes();
    }

    pub fn remove(&mut self, key: &str) {
        self.rows.remove(key);
        self.history.retain(|(item, _), _| item != key);
    }

    pub fn change_source(&mut self, items: &[Instrument], group: SourceGroup) {
        for item in items.iter().filter(|item| item.group() == group) {
            self.rows.remove(&item.key());
        }
        self.invalidate_quotes();
    }

    pub fn listing(&self, key: &str) -> Option<String> {
        self.rows.get(key)?.quote.as_ref()?.listing.clone()
    }

    pub fn cached_history(&self, key: &(String, Period)) -> Option<HistoryResult> {
        let (until, result) = self.history.get(key)?;
        (*until > Instant::now()).then(|| result.clone())
    }

    pub fn store_history(&mut self, key: (String, Period), result: HistoryResult) {
        let ttl = if result.is_ok() { 120 } else { 8 };
        self.history
            .insert(key, (Instant::now() + Duration::from_secs(ttl), result));
    }

    pub fn set_refreshing(&mut self, refreshing: bool) {
        self.refreshing = refreshing;
    }

    pub fn update(&mut self, key: String, result: Result<Quote, String>) {
        let row = self.rows.entry(key).or_default();
        match result {
            Ok(quote) => {
                row.samples.push_back(quote.price);
                if row.samples.len() > 32 {
                    row.samples.pop_front();
                }
                row.quote = Some(quote);
                row.error = None;
            }
            Err(error) => row.error = Some(error),
        }
    }

    pub fn snapshot(&self, settings: &Settings) -> Value {
        let items: Vec<_> = settings.watchlist.iter().map(|item| {
            let key = item.key();
            let row = self.rows.get(&key);
            let quote = row.and_then(|row| row.quote.as_ref());
            let error = row.and_then(|row| row.error.as_ref());
            json!({
                "key": key,
                "market": item.market,
                "code": item.code,
                "name": item.name,
                "currency": item.currency(),
                "source": if settings.sources.contains_key(&item.group()) { "自定义行情源" } else { item.source() },
                "quote": quote,
                "samples": row.map(|row| &row.samples),
                "cached": quote.is_some() && error.is_some(),
                "error": error,
            })
        }).collect();
        json!({
            "items": items,
            "pinned": settings.pinned,
            "proxy": settings.proxy,
            "sources": settings.sources,
            "interval": settings.interval,
            "chartPeriod": settings.selected_period.unwrap_or(Period::Day),
            "refreshing": self.refreshing,
        })
    }
}
