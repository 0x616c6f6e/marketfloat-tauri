use super::{
    binance_url, json, numeric, single_quote, tencent_request, Instrument, Market, QuoteProvider,
    SourceGroup,
};
use crate::settings::Period;
use chrono::{DateTime, Datelike, Days, Months, NaiveDate, Utc};
use futures_util::future::try_join_all;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

const LIMIT: usize = 30;

#[derive(Clone, Serialize)]
struct Candle {
    date: NaiveDate,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: Option<f64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    candles: Vec<Candle>,
    source: &'static str,
    volume_unit: String,
    adjusted: bool,
}

fn candle(date: NaiveDate, values: [Option<f64>; 4], volume: Option<f64>) -> Option<Candle> {
    let [Some(open), Some(high), Some(low), Some(close)] = values else {
        return None;
    };
    if values
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite() || value <= 0.0)
        || high < open.max(close)
        || low > open.min(close)
    {
        return None;
    }
    Some(Candle {
        date,
        open,
        high,
        low,
        close,
        volume: volume.filter(|volume| volume.is_finite() && *volume >= 0.0),
    })
}

fn stock_candle(row: &Value) -> Option<Candle> {
    let date = NaiveDate::parse_from_str(row.get(0)?.as_str()?, "%Y-%m-%d").ok()?;
    candle(
        date,
        [1, 3, 4, 2].map(|index| numeric(row.get(index))),
        numeric(row.get(5)),
    )
}

fn timed_candle(row: &Value, time: DateTime<Utc>, order: [usize; 4]) -> Option<Candle> {
    candle(
        time.date_naive(),
        order.map(|index| numeric(row.get(index))),
        numeric(row.get(5)),
    )
}

fn unique(rows: impl IntoIterator<Item = Candle>) -> Vec<Candle> {
    rows.into_iter()
        .map(|bar| (bar.date, bar))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

fn rows(data: &Value) -> impl Iterator<Item = &Value> {
    data.as_array().into_iter().flatten()
}

fn bucket(date: NaiveDate, period: Period) -> NaiveDate {
    match period {
        Period::Day => date,
        Period::Week => date - Days::new(date.weekday().num_days_from_monday().into()),
        Period::Month => date.with_day(1).expect("first day of month"),
    }
}

fn aggregate(daily: Vec<Candle>, period: Period) -> Vec<Candle> {
    let mut buckets: BTreeMap<NaiveDate, Candle> = BTreeMap::new();
    for mut bar in daily {
        bar.date = bucket(bar.date, period);
        buckets
            .entry(bar.date)
            .and_modify(|existing| {
                existing.high = existing.high.max(bar.high);
                existing.low = existing.low.min(bar.low);
                existing.close = bar.close;
                existing.volume = existing.volume.zip(bar.volume).map(|(a, b)| a + b);
            })
            .or_insert(bar);
    }
    buckets.into_values().collect()
}

async fn coinbase(
    client: &Client,
    item: &Instrument,
    period: Period,
) -> Result<Vec<Candle>, String> {
    let end = Utc::now();
    let current = bucket(end.date_naive(), period);
    let start = match period {
        Period::Day => current - Days::new(LIMIT as u64 - 1),
        Period::Week => current - Days::new((LIMIT as u64 - 1) * 7),
        Period::Month => current - Months::new(LIMIT as u32 - 1),
    }
    .and_hms_opt(0, 0, 0)
    .unwrap()
    .and_utc();
    let mut jobs = Vec::new();
    let mut from = start;
    while from < end {
        let to = (from + Days::new(280)).min(end);
        let request = client
            .get(format!(
                "https://api.exchange.coinbase.com/products/{}/candles",
                item.code
            ))
            .query(&[
                ("granularity", "86400".to_owned()),
                ("start", from.to_rfc3339()),
                ("end", to.to_rfc3339()),
            ]);
        jobs.push(async move {
            let data = json(request).await?;
            let bars: Vec<_> = rows(&data)
                .filter_map(|row| {
                    let time = DateTime::from_timestamp(row.get(0)?.as_i64()?, 0)?;
                    (time >= from && time < to)
                        .then(|| timed_candle(row, time, [3, 2, 1, 4]))
                        .flatten()
                })
                .collect();
            Ok::<_, String>(bars)
        });
        from = to;
    }
    let daily = unique(try_join_all(jobs).await?.into_iter().flatten());
    Ok(aggregate(daily, period))
}

fn valid_listing(item: &Instrument, listing: &str) -> bool {
    listing
        .strip_prefix(&format!("{}.", item.code))
        .is_some_and(|suffix| {
            (1..=4).contains(&suffix.len())
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        })
}

async fn stocks(
    client: &Client,
    item: &Instrument,
    listing: Option<&str>,
    period: Period,
) -> Result<History, String> {
    if item.market == Market::Cn && item.code.starts_with("BJ") {
        let interval = match period {
            Period::Day => 101,
            Period::Week => 102,
            Period::Month => 103,
        };
        let code = &item.code[2..];
        let url = format!("https://push2his.eastmoney.com/api/qt/stock/kline/get?secid=0.{code}&klt={interval}&fqt=0&end=20500101&lmt={LIMIT}&fields1=f1,f2,f3,f4,f5,f6&fields2=f51,f52,f53,f54,f55,f56");
        if let Ok(data) = json(client.get(url)).await {
            let candles = unique(rows(&data["data"]["klines"]).filter_map(|row| {
                let fields: Vec<_> = row.as_str()?.split(',').map(Value::from).collect();
                stock_candle(&Value::Array(fields))
            }));
            if !candles.is_empty() {
                return Ok(History {
                    candles,
                    source: "东方财富",
                    volume_unit: "手".into(),
                    adjusted: false,
                });
            }
        }
    }
    let (ticker, endpoint, unit) = if item.market == Market::Us {
        let listing = match listing.filter(|listing| valid_listing(item, listing)) {
            Some(known) => Some(known.to_owned()),
            None => single_quote(client, item, None).await?.listing,
        }
        .filter(|listing| valid_listing(item, listing))
        .ok_or("暂未识别该美股的上市交易所，无法获取 K 线。")?;
        (format!("us{listing}"), "usfqkline", "股")
    } else {
        (item.code.to_ascii_lowercase(), "fqkline", "手")
    };
    let period = match period {
        Period::Day => "day",
        Period::Week => "week",
        Period::Month => "month",
    };
    let url = format!("https://web.ifzq.gtimg.cn/appstock/app/{endpoint}/get?param={ticker},{period},,,{LIMIT},qfq");
    let data = json(tencent_request(client, url)).await?;
    let series = &data["data"][&ticker];
    let adjusted = unique(rows(&series[format!("qfq{period}")]).filter_map(stock_candle));
    let (candles, adjusted) = if adjusted.is_empty() {
        (
            unique(rows(&series[period]).filter_map(stock_candle)),
            false,
        )
    } else {
        (adjusted, true)
    };
    Ok(History {
        candles,
        source: "腾讯财经",
        volume_unit: unit.into(),
        adjusted,
    })
}

pub async fn history(
    client: &Client,
    item: &Instrument,
    listing: Option<&str>,
    period: Period,
    provider: QuoteProvider,
) -> Result<History, String> {
    let mut history = match item.group() {
        SourceGroup::Cn | SourceGroup::Us => stocks(client, item, listing, period).await?,
        SourceGroup::Usd => History {
            candles: coinbase(client, item, period).await?,
            source: item.source(),
            volume_unit: item.base().into(),
            adjusted: false,
        },
        SourceGroup::Usdt | SourceGroup::Perp if provider == QuoteProvider::Gate => {
            gate_history(client, item, period).await?
        }
        SourceGroup::Usdt | SourceGroup::Perp if provider == QuoteProvider::Okx => {
            okx_history(client, item, period).await?
        }
        SourceGroup::Usdt | SourceGroup::Perp => {
            let interval = match period {
                Period::Day => "1d",
                Period::Week => "1w",
                Period::Month => "1M",
            };
            let url = format!(
                "{}&interval={interval}&limit={LIMIT}",
                binance_url(item, "klines")
            );
            let data = json(client.get(url)).await?;
            let candles = unique(rows(&data).filter_map(|row| {
                let time = DateTime::from_timestamp_millis(row.get(0)?.as_i64()?)?;
                timed_candle(row, time, [1, 2, 3, 4])
            }));
            History {
                candles,
                source: provider.label(item.group()),
                volume_unit: item.base().into(),
                adjusted: false,
            }
        }
    };
    if history.candles.is_empty() {
        return Err("行情源暂未提供该标的在所选周期的 K 线。".into());
    }
    history
        .candles
        .drain(..history.candles.len().saturating_sub(LIMIT));
    Ok(history)
}

async fn gate_history(
    client: &Client,
    item: &Instrument,
    period: Period,
) -> Result<History, String> {
    let interval = match period {
        Period::Day => "1d",
        Period::Week | Period::Month => "1d",
    };
    let limit = match period {
        Period::Day => LIMIT,
        Period::Week => LIMIT * 7,
        Period::Month => LIMIT * 31,
    };
    let (url, query_key, pair, volume_unit) = match item.market {
        Market::Perp => (
            "https://api.gateio.ws/api/v4/futures/usdt/candlesticks",
            "contract",
            format!("{}_USDT", item.base()),
            "USDT".to_owned(),
        ),
        Market::Crypto => (
            "https://api.gateio.ws/api/v4/spot/candlesticks",
            "currency_pair",
            item.code.replace('-', "_"),
            item.base().to_owned(),
        ),
        _ => return Err("Gate.io 仅支持 USDT 现货和永续 K 线。".into()),
    };
    let data = json(
        client
            .get(url)
            .query(&[
                (query_key, pair),
                ("interval", interval.to_owned()),
                ("limit", limit.to_string()),
            ]),
    )
    .await?;
    let daily = unique(rows(&data).filter_map(|row| {
        let timestamp = (if item.market == Market::Perp {
            numeric(row.get("t"))?
        } else {
            numeric(row.get(0))?
        }) as i64;
        let time = DateTime::from_timestamp(timestamp, 0)?;
        let values = if item.market == Market::Perp {
            ["o", "h", "l", "c"].map(|field| numeric(row.get(field)))
        } else {
            [5, 3, 4, 2].map(|index| numeric(row.get(index)))
        };
        candle(
            time.date_naive(),
            values,
            if item.market == Market::Perp {
                numeric(row.get("sum"))
            } else {
                numeric(row.get(6))
            },
        )
    }));
    let candles = if period == Period::Day {
        daily
    } else {
        aggregate(daily, period)
    };
    Ok(History {
        candles,
        source: QuoteProvider::Gate.label(item.group()),
        volume_unit,
        adjusted: false,
    })
}

async fn okx_history(
    client: &Client,
    item: &Instrument,
    period: Period,
) -> Result<History, String> {
    let inst_id = match item.market {
        Market::Perp => format!("{}-USDT-SWAP", item.base()),
        Market::Crypto => item.code.clone(),
        _ => return Err("OKX 仅支持 USDT 现货和永续 K 线。".into()),
    };
    let bar = match period {
        Period::Day => "1D",
        Period::Week => "1W",
        Period::Month => "1M",
    };
    let data = json(
        client
            .get("https://www.okx.com/api/v5/market/candles")
            .query(&[
                ("instId", inst_id),
                ("bar", bar.to_owned()),
                ("limit", LIMIT.to_string()),
            ]),
    )
    .await?;
    if data.get("code").and_then(Value::as_str) != Some("0") {
        return Err(data
            .get("msg")
            .and_then(Value::as_str)
            .filter(|message| !message.is_empty())
            .unwrap_or("OKX 未返回该交易对 K 线。")
            .to_owned());
    }
    let candles = unique(rows(&data["data"]).filter_map(|row| {
        let timestamp = numeric(row.get(0))? as i64;
        let time = DateTime::from_timestamp_millis(timestamp)?;
        candle(
            time.date_naive(),
            [1, 2, 3, 4].map(|index| numeric(row.get(index))),
            numeric(row.get(if item.market == Market::Perp { 6 } else { 5 })),
        )
    }));
    Ok(History {
        candles,
        source: QuoteProvider::Okx.label(item.group()),
        volume_unit: item.base().into(),
        adjusted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coinbase_aggregation_preserves_ohlc_and_sums_volume() {
        let bars = [
            ("2026-11-01", 10.0, 12.0, 9.0, 11.0, 5.0),
            ("2026-11-02", 11.0, 15.0, 10.0, 14.0, 7.0),
            ("2026-11-03", 14.0, 16.0, 8.0, 12.0, 8.0),
        ]
        .into_iter()
        .map(|(date, open, high, low, close, volume)| Candle {
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            open,
            high,
            low,
            close,
            volume: Some(volume),
        })
        .collect::<Vec<_>>();
        let weekly = aggregate(bars.clone(), Period::Week);
        assert_eq!(weekly.len(), 2);
        assert_eq!(weekly[1].date.to_string(), "2026-11-02");
        assert_eq!(weekly[1].volume, Some(15.0));
        let monthly = aggregate(bars, Period::Month);
        assert_eq!(monthly.len(), 1);
        let bar = &monthly[0];
        assert_eq!(
            (bar.open, bar.high, bar.low, bar.close, bar.volume),
            (10.0, 16.0, 8.0, 12.0, Some(20.0))
        );
    }
}
