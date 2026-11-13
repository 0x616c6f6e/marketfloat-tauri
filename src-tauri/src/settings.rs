use crate::market::{self, Instrument, Source, SourceGroup};
use crate::portfolio::{Monitor, Position, MAX_MONITORS};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::Path,
};
use tauri::Url;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    System,
    Direct,
    FixedServers,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Proxy {
    pub mode: ProxyMode,
    pub address: String,
}

impl Proxy {
    pub fn validate(&mut self) -> Result<(), String> {
        if self.mode != ProxyMode::FixedServers {
            self.address.clear();
            return Ok(());
        }
        let url = Url::parse(self.address.trim()).map_err(|_| "请输入完整的代理地址。")?;
        if !matches!(url.scheme(), "http" | "https" | "socks5")
            || url.host_str().is_none()
            || url
                .port_or_known_default()
                .filter(|port| *port > 0)
                .is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "代理地址需要包含协议和主机，可填写端口，例如 socks5://127.0.0.1:1080。".into(),
            );
        }
        self.address = url.as_str().trim_end_matches('/').to_owned();
        Ok(())
    }

    pub fn browser_args(&self) -> Option<String> {
        let mode = match self.mode {
            ProxyMode::System => return None,
            ProxyMode::Direct => "--no-proxy-server".to_owned(),
            ProxyMode::FixedServers => format!("--proxy-server={}", self.address),
        };
        Some(format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection {mode}"
        ))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Day,
    Week,
    Month,
}

impl Period {
    pub fn tradingview(self) -> &'static str {
        match self {
            Self::Day => "D",
            Self::Week => "W",
            Self::Month => "M",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub proxy: Proxy,
    pub selected_period: Option<Period>,
    pub pinned: bool,
    pub red_up: bool,
    pub watchlist: Vec<Instrument>,
    pub sources: HashMap<SourceGroup, Source>,
    pub monitors: Vec<Monitor>,
    pub positions: HashMap<String, Position>,
    pub interval: u64,
}

pub const INTERVALS: [u64; 6] = [3, 5, 10, 15, 30, 60];
pub const MAX_ITEMS: usize = 40;

impl Default for Settings {
    fn default() -> Self {
        Self {
            proxy: Proxy::default(),
            selected_period: None,
            pinned: true,
            red_up: true,
            watchlist: market::defaults(),
            sources: HashMap::new(),
            monitors: Vec::new(),
            positions: HashMap::new(),
            interval: 30,
        }
    }
}

pub fn load(file: &Path) -> Settings {
    let mut settings: Settings = fs::read(file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    if settings.proxy.validate().is_err() {
        settings.proxy = Proxy::default();
    }
    let mut unique = HashSet::new();
    settings.watchlist = settings
        .watchlist
        .into_iter()
        .filter_map(|item| Instrument::parse(item.market, &item.code, &item.name))
        .filter(|item| unique.insert(item.key()))
        .take(MAX_ITEMS)
        .collect();
    settings
        .sources
        .retain(|_, source| source.validate().is_ok());
    let keys: HashSet<_> = settings.watchlist.iter().map(Instrument::key).collect();
    settings
        .positions
        .retain(|key, position| keys.contains(key) && position.validate().is_ok());
    let mut monitor_ids = HashSet::new();
    settings.monitors.retain(|monitor| {
        keys.contains(&monitor.condition.key)
            && monitor.condition.validate().is_ok()
            && monitor_ids.insert(monitor.id)
    });
    settings.monitors.truncate(MAX_MONITORS);
    if !INTERVALS.contains(&settings.interval) {
        settings.interval = 30;
    }
    settings
}

pub fn save(file: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = file.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec_pretty(settings)?)?;
    fs::rename(temp, file)
}

pub fn tradingview_url(symbol: &str, period: Option<Period>) -> Url {
    let mut url =
        Url::parse("https://www.tradingview.com/chart/").expect("constant TradingView URL");
    url.query_pairs_mut()
        .append_pair("symbol", symbol)
        .append_pair("interval", period.map_or("15", Period::tradingview));
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_defaults_to_fifteen_minutes_and_honors_explicit_period() {
        let default = tradingview_url("BINANCE:NVDAUSDT.P", None);
        assert_eq!(
            default.query(),
            Some("symbol=BINANCE%3ANVDAUSDT.P&interval=15")
        );
        let week = tradingview_url("NASDAQ:NVDA", Some(Period::Week));
        assert_eq!(week.query(), Some("symbol=NASDAQ%3ANVDA&interval=W"));
    }

    #[test]
    fn proxy_arguments_use_a_validated_authority() {
        let mut proxy = Proxy {
            mode: ProxyMode::FixedServers,
            address: " http://127.0.0.1:18080/ ".into(),
        };
        proxy.validate().unwrap();
        assert!(proxy
            .browser_args()
            .unwrap()
            .ends_with("--proxy-server=http://127.0.0.1:18080"));
        for invalid in [
            "http://host/path",
            "http://user:password@host:8080",
            "http://host:0",
            "http://host --no-proxy-server",
        ] {
            proxy.address = invalid.into();
            assert!(proxy.validate().is_err());
        }
        assert!(Proxy::default().browser_args().is_none());
        proxy.mode = ProxyMode::Direct;
        proxy.validate().unwrap();
        assert!(proxy.browser_args().unwrap().ends_with("--no-proxy-server"));
    }
}
