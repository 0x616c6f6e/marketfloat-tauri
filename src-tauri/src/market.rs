mod history;

pub use history::{history, History};

use crate::settings::{Proxy, ProxyMode};
use chrono::Utc;
use encoding_rs::GBK;
use futures_util::{stream, StreamExt};
use regex::Regex;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::LazyLock, time::Duration};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Market {
    Cn,
    Us,
    Crypto,
    Perp,
}

impl Market {
    fn key(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Us => "us",
            Self::Crypto => "crypto",
            Self::Perp => "perp",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceGroup {
    Cn,
    Us,
    Usd,
    Usdt,
    Perp,
}

impl SourceGroup {
    pub fn sample(self) -> (Market, &'static str) {
        match self {
            Self::Cn => (Market::Cn, "SH600519"),
            Self::Us => (Market::Us, "NVDA"),
            Self::Usd => (Market::Crypto, "BTC-USD"),
            Self::Usdt => (Market::Crypto, "BTC-USDT"),
            Self::Perp => (Market::Perp, "NVDAUSDT.P"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    pub market: Market,
    pub code: String,
    #[serde(default)]
    pub name: String,
}

impl Instrument {
    pub fn parse(mut market: Market, input: &str, name: &str) -> Option<Self> {
        static PERP: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^(?:BINANCE:)?([A-Z0-9]{1,16})-?USDT(?:\.P)?$").unwrap());
        static US: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[A-Z]{1,6}(?:\.[A-Z]{1,2})?$").unwrap());
        static SPOT: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[A-Z0-9]{1,16}-(USD|USDT)$").unwrap());
        static CHINA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(SH|SZ|BJ)\d{6}$").unwrap());
        let mut code = input.trim().to_ascii_uppercase();
        if market == Market::Perp || code.ends_with("USDT.P") {
            let base = PERP.captures(&code)?.get(1)?.as_str();
            code = format!("{base}USDT.P");
            market = Market::Perp;
        } else {
            match market {
                Market::Cn => {
                    if let Some((digits, suffix @ ("SH" | "SZ" | "BJ"))) = code.split_once('.') {
                        code = format!("{suffix}{digits}");
                    } else if code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit()) {
                        let prefix = match code.as_bytes()[0] {
                            b'5' | b'6' => "SH",
                            b'4' | b'8' | b'9' => "BJ",
                            b'0' | b'1' | b'3' => "SZ",
                            _ => return None,
                        };
                        code = format!("{prefix}{code}");
                    }
                    if !CHINA.is_match(&code) || !code.is_ascii() {
                        return None;
                    }
                }
                Market::Us => {
                    code = code.replacen('-', ".", 1);
                    if !US.is_match(&code) {
                        return None;
                    }
                }
                Market::Crypto => {
                    code = code.replace(' ', "").replacen('/', "-", 1);
                    if !code.contains('-') {
                        code.push_str("-USD");
                    }
                    if !SPOT.is_match(&code) {
                        return None;
                    }
                }
                Market::Perp => unreachable!(),
            }
        }
        Some(Self {
            market,
            code,
            name: name.chars().take(80).collect(),
        })
    }

    pub fn key(&self) -> String {
        format!("{}/{}", self.market.key(), self.code)
    }

    pub fn group(&self) -> SourceGroup {
        match self.market {
            Market::Cn => SourceGroup::Cn,
            Market::Us => SourceGroup::Us,
            Market::Perp => SourceGroup::Perp,
            Market::Crypto if self.code.ends_with("-USDT") => SourceGroup::Usdt,
            Market::Crypto => SourceGroup::Usd,
        }
    }

    pub fn currency(&self) -> &'static str {
        match self.group() {
            SourceGroup::Cn => "CNY",
            SourceGroup::Us | SourceGroup::Usd => "USD",
            SourceGroup::Usdt | SourceGroup::Perp => "USDT",
        }
    }

    pub fn source(&self) -> &'static str {
        match self.group() {
            SourceGroup::Cn | SourceGroup::Us => "腾讯财经",
            SourceGroup::Usd => "Coinbase",
            SourceGroup::Usdt => "Binance",
            SourceGroup::Perp => "Binance 永续",
        }
    }

    fn base(&self) -> &str {
        match self.market {
            Market::Perp => self.code.trim_end_matches("USDT.P"),
            Market::Crypto => self.code.split('-').next().unwrap_or(&self.code),
            _ => &self.code,
        }
    }

    fn api_symbol(&self) -> String {
        match self.market {
            Market::Cn => self.code.to_ascii_lowercase(),
            Market::Perp => format!("{}USDT", self.base()),
            _ => self.code.replace('-', ""),
        }
    }

    pub fn tradingview_symbol(&self, listing: Option<&str>) -> String {
        match self.market {
            Market::Cn => {
                let (prefix, code) = self.code.split_at(2);
                let exchange = match prefix {
                    "SH" => "SSE",
                    "SZ" => "SZSE",
                    _ => "BJSE",
                };
                format!("{exchange}:{code}")
            }
            Market::Us => {
                let exchange =
                    listing.and_then(|listing| listing.strip_prefix(&format!("{}.", self.code)));
                let prefix = match exchange {
                    Some("O" | "OQ") => "NASDAQ",
                    Some("N") => "NYSE",
                    Some("A" | "AM") => "AMEX",
                    Some("ARCA") => "ARCA",
                    Some("PK" | "OB") => "OTC",
                    _ => return self.code.clone(),
                };
                format!("{prefix}:{}", self.code)
            }
            Market::Crypto => {
                let prefix = if self.currency() == "USDT" {
                    "BINANCE"
                } else {
                    "COINBASE"
                };
                format!("{prefix}:{}", self.api_symbol())
            }
            Market::Perp => format!("BINANCE:{}", self.code),
        }
    }
}

pub fn defaults() -> Vec<Instrument> {
    [
        (Market::Perp, "NVDAUSDT.P", ""),
        (Market::Us, "NVDA", "英伟达"),
        (Market::Cn, "SH600519", "贵州茅台"),
        (Market::Crypto, "BTC-USDT", "比特币"),
        (Market::Crypto, "BTC-USD", "比特币"),
    ]
    .into_iter()
    .filter_map(|(market, code, name)| Instrument::parse(market, code, name))
    .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Source {
    pub url: String,
    pub price: String,
    pub change: String,
    pub previous: String,
    pub name: String,
}

const TOKENS: [&str; 4] = ["{code}", "{symbol}", "{base}", "{quote}"];

impl Source {
    pub fn validate(&mut self) -> Result<(), String> {
        static FIELD: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"^(?:[\p{L}\p{N}_$-]+|\[[0-9]+\])(?:\.[\p{L}\p{N}_$-]+|\[[0-9]+\])*$")
                .unwrap()
        });
        self.url = self.url.trim().to_owned();
        let mut example = self.url.clone();
        for token in TOKENS {
            example = example.replace(token, "sample");
        }
        if self.url.len() > 2000
            || example
                .chars()
                .any(|c| c.is_whitespace() || "{}\\".contains(c))
            || self.url.split_once("://").is_none_or(|(_, tail)| {
                tail.split(['/', '?', '#'])
                    .next()
                    .is_none_or(|host| host.contains('{'))
            })
        {
            return Err("请输入有效的接口地址，变量只能放在路径或查询参数中。".into());
        }
        let url = Url::parse(&example).map_err(|_| "请输入完整的 HTTP 或 HTTPS 接口地址。")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err("接口须为 HTTP 或 HTTPS 地址，且不能包含登录信息或片段。".into());
        }
        for field in [
            &mut self.price,
            &mut self.change,
            &mut self.previous,
            &mut self.name,
        ] {
            *field = field.trim().to_owned();
            if !field.is_empty() && (field.chars().count() > 160 || !FIELD.is_match(field)) {
                return Err("字段路径格式无效，例如 data.price 或 data[0].price。".into());
            }
        }
        if self.price.is_empty() {
            return Err("请输入最新价字段。".into());
        }
        Ok(())
    }

    fn url_for(&self, item: &Instrument) -> String {
        // Instrument parsing limits all substitutions to unreserved URL characters.
        TOKENS
            .into_iter()
            .zip([
                item.code.as_str(),
                &item.api_symbol(),
                item.base(),
                item.currency(),
            ])
            .fold(self.url.clone(), |url, (token, value)| {
                url.replace(token, value)
            })
    }
}

pub fn client(proxy: &Proxy) -> Result<Client, String> {
    let builder = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_millis(8500))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("MarketFloat-Tauri");
    let builder = match proxy.mode {
        ProxyMode::System => builder,
        ProxyMode::Direct => builder.no_proxy(),
        ProxyMode::FixedServers => {
            let address = proxy.address.replacen("socks5://", "socks5h://", 1);
            builder
                .no_proxy()
                .proxy(reqwest::Proxy::all(address).map_err(|_| "代理地址无效。")?)
        }
    };
    builder.build().map_err(|_| "无法初始化行情网络。".into())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub price: f64,
    pub change: Option<f64>,
    pub read_at: i64,
    pub name: Option<String>,
    pub listing: Option<String>,
}

fn quote(price: f64, change: Option<f64>, name: Option<String>, listing: Option<String>) -> Quote {
    Quote {
        price,
        change,
        name,
        listing,
        read_at: Utc::now().timestamp_millis(),
    }
}

fn numeric(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|value| value.is_finite())
}

fn percentage(price: f64, previous: Option<f64>) -> Option<f64> {
    previous
        .filter(|value| *value > 0.0)
        .map(|value| (price / value - 1.0) * 100.0)
        .filter(|value| value.is_finite())
}

fn field<'a>(mut value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }
    let path = path.replace('[', ".").replace(']', "");
    for part in path.trim_start_matches('.').split('.') {
        value = if value.is_array() {
            value.get(part.parse::<usize>().ok()?)?
        } else {
            value.get(part)?
        };
    }
    Some(value)
}

async fn response(request: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    let response = request.send().await.map_err(request_error)?;
    if !response.status().is_success() {
        let code = response.status().as_u16();
        return Err(match code {
            400 | 404 => "行情源无此代码".into(),
            _ => format!("行情源暂不可用（HTTP {code}）"),
        });
    }
    Ok(response)
}

fn request_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "行情请求超时"
    } else if error.is_decode() {
        "行情源返回了无效数据"
    } else {
        "无法连接行情源"
    }
    .into()
}

async fn json(request: reqwest::RequestBuilder) -> Result<Value, String> {
    response(request).await?.json().await.map_err(request_error)
}

pub fn tencent_request(client: &Client, url: String) -> reqwest::RequestBuilder {
    client
        .get(url)
        .header(reqwest::header::REFERER, "https://gu.qq.com/")
}

fn stock_key(item: &Instrument) -> String {
    if item.market == Market::Cn {
        item.code.to_ascii_lowercase()
    } else {
        format!("us{}", item.code)
    }
}

async fn stocks(client: &Client, items: &[Instrument]) -> Result<HashMap<String, Quote>, String> {
    let codes: Vec<_> = items.iter().map(stock_key).collect();
    let request = tencent_request(client, format!("https://qt.gtimg.cn/q={}", codes.join(",")));
    let bytes = response(request)
        .await?
        .bytes()
        .await
        .map_err(request_error)?;
    let (text, _, _) = GBK.decode(&bytes);
    Ok(text
        .lines()
        .filter_map(|line| {
            let (key, data) = line.trim().strip_prefix("v_")?.split_once("=\"")?;
            let fields: Vec<_> = data.split('"').next()?.split('~').collect();
            let price = fields
                .get(3)?
                .parse::<f64>()
                .ok()
                .filter(|price| price.is_finite() && *price > 0.0)?;
            let change = fields
                .get(32)
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite())
                .or_else(|| percentage(price, fields.get(4).and_then(|value| value.parse().ok())));
            let name = fields
                .get(1)
                .filter(|value| !value.is_empty())
                .map(|value| value.trim().to_owned());
            let listing = fields.get(2).map(|value| value.trim().to_owned());
            Some((
                key.to_ascii_lowercase(),
                quote(price, change, name, listing),
            ))
        })
        .collect())
}

fn binance_url(item: &Instrument, endpoint: &str) -> String {
    let base = if item.market == Market::Perp {
        "https://fapi.binance.com/fapi/v1"
    } else {
        "https://data-api.binance.vision/api/v3"
    };
    format!("{base}/{endpoint}?symbol={}", item.api_symbol())
}

pub async fn single_quote(
    client: &Client,
    item: &Instrument,
    source: Option<&Source>,
) -> Result<Quote, String> {
    if let Some(source) = source {
        let data = json(
            client
                .get(source.url_for(item))
                .header(reqwest::header::ACCEPT, "application/json"),
        )
        .await?;
        let price = numeric(field(&data, &source.price))
            .filter(|price| *price > 0.0)
            .ok_or("最新价字段不存在或不是正数")?;
        let change = numeric(field(&data, &source.change))
            .or_else(|| percentage(price, numeric(field(&data, &source.previous))));
        let name = field(&data, &source.name)
            .and_then(Value::as_str)
            .map(|name| name.chars().take(80).collect());
        return Ok(quote(price, change, name, None));
    }
    if matches!(item.market, Market::Cn | Market::Us) {
        return stocks(client, std::slice::from_ref(item))
            .await?
            .remove(&stock_key(item).to_ascii_lowercase())
            .ok_or_else(|| "行情源未返回报价".into());
    }
    let usdt = item.currency() == "USDT";
    let (url, last, open) = if usdt {
        (binance_url(item, "ticker/24hr"), "lastPrice", "openPrice")
    } else {
        (
            format!(
                "https://api.exchange.coinbase.com/products/{}/stats",
                item.code
            ),
            "last",
            "open",
        )
    };
    let data = json(client.get(url)).await?;
    let price = numeric(data.get(last))
        .filter(|price| *price > 0.0)
        .ok_or("行情源未返回报价")?;
    let change = numeric(data.get("priceChangePercent"))
        .or_else(|| percentage(price, numeric(data.get(open))));
    Ok(quote(price, change, None, None))
}

pub async fn quotes(
    client: &Client,
    items: &[Instrument],
    sources: &HashMap<SourceGroup, Source>,
) -> Vec<(String, Result<Quote, String>)> {
    let (shares, singles): (Vec<_>, Vec<_>) = items.iter().cloned().partition(|item| {
        matches!(item.market, Market::Cn | Market::Us) && !sources.contains_key(&item.group())
    });
    let share_jobs = async {
        let mut output = Vec::new();
        for batch in shares.chunks(30) {
            let data = stocks(client, batch).await;
            output.extend(batch.iter().map(|item| {
                let result = match &data {
                    Ok(rows) => rows
                        .get(&stock_key(item).to_ascii_lowercase())
                        .cloned()
                        .ok_or_else(|| "行情源未返回报价".into()),
                    Err(error) => Err(error.clone()),
                };
                (item.key(), result)
            }));
        }
        output
    };
    let single_jobs = stream::iter(singles)
        .map(|item| async move {
            let result = single_quote(client, &item, sources.get(&item.group())).await;
            (item.key(), result)
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>();
    let (mut shares, singles) = tokio::join!(share_jobs, single_jobs);
    shares.extend(singles);
    shares
}
