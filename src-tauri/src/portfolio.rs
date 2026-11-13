use crate::market::{Instrument, Quote};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_MONITORS: usize = 100;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Side {
    Long,
    Short,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    side: Side,
    entry_price: f64,
    quantity: f64,
    leverage: f64,
}

#[derive(Serialize)]
pub struct PositionMetrics {
    roi: Option<f64>,
    pnl: Option<f64>,
}

impl Position {
    pub fn validate(&self) -> Result<(), String> {
        if !positive(self.entry_price) {
            return Err("开仓价必须大于零。".into());
        }
        if !positive(self.quantity) {
            return Err("数量必须大于零。".into());
        }
        if !self.leverage.is_finite() || self.leverage < 1.0 {
            return Err("杠杆至少为 1 倍。".into());
        }
        Ok(())
    }

    pub fn metrics(&self, price: f64) -> PositionMetrics {
        let direction = match self.side {
            Side::Long => 1.0,
            Side::Short => -1.0,
        };
        let movement = (price - self.entry_price) * direction;
        let roi = movement / self.entry_price * self.leverage * 100.0;
        let pnl = movement * self.quantity;
        PositionMetrics {
            roi: roi.is_finite().then_some(roi),
            pnl: pnl.is_finite().then_some(pnl),
        }
    }
}

fn positive(number: f64) -> bool {
    number.is_finite() && number > 0.0
}

#[derive(Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Above,
    Below,
}

#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct Condition {
    pub key: String,
    pub direction: Direction,
    pub threshold: f64,
}

impl Condition {
    pub fn validate(&self) -> Result<(), String> {
        if positive(self.threshold) {
            Ok(())
        } else {
            Err("目标价格必须大于零。".into())
        }
    }

    fn matches(&self, price: f64) -> bool {
        match self.direction {
            Direction::Above => price >= self.threshold,
            Direction::Below => price <= self.threshold,
        }
    }
}

#[derive(Deserialize)]
pub struct MonitorInput {
    pub id: Option<Uuid>,
    #[serde(flatten)]
    pub condition: Condition,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: Uuid,
    #[serde(flatten)]
    pub condition: Condition,
    pub enabled: bool,
    matched: bool,
    triggered_at: Option<i64>,
    triggered_price: Option<f64>,
}

impl Monitor {
    pub fn new(condition: Condition) -> Self {
        Self {
            id: Uuid::new_v4(),
            condition,
            enabled: true,
            matched: false,
            triggered_at: None,
            triggered_price: None,
        }
    }

    pub fn set_condition(&mut self, condition: Condition) {
        if self.condition != condition {
            self.condition = condition;
            self.matched = false;
            self.triggered_at = None;
            self.triggered_price = None;
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.matched = false;
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MonitorAction {
    Enable,
    Disable,
    Remove,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: Uuid,
    pub code: String,
    pub currency: &'static str,
    pub direction: Direction,
    pub threshold: f64,
    pub price: f64,
}

impl Alert {
    pub fn message(&self) -> String {
        let comparator = match self.direction {
            Direction::Above => "≥",
            Direction::Below => "≤",
        };
        format!(
            "{} {comparator} {}，现价 {} {}",
            self.code, self.threshold, self.price, self.currency
        )
    }
}

// Returns whether the persisted state changed and the conditions that just became true.
pub fn evaluate(
    monitors: &mut [Monitor],
    item: &Instrument,
    key: &str,
    quote: &Quote,
    alerts: &mut Vec<Alert>,
) -> bool {
    let mut changed = false;
    for monitor in monitors
        .iter_mut()
        .filter(|monitor| monitor.enabled && monitor.condition.key == key)
    {
        let matched = monitor.condition.matches(quote.price);
        if matched == monitor.matched {
            continue;
        }
        monitor.matched = matched;
        changed = true;
        if matched {
            monitor.triggered_at = Some(quote.read_at);
            monitor.triggered_price = Some(quote.price);
            alerts.push(Alert {
                id: monitor.id,
                code: item.code.clone(),
                currency: item.currency(),
                direction: monitor.condition.direction,
                threshold: monitor.condition.threshold,
                price: quote.price,
            });
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::Market;

    #[test]
    fn alerts_rearm_only_after_fresh_quote_leaves_condition() {
        let item = Instrument::parse(Market::Perp, "NVDAUSDT.P", "").unwrap();
        let key = item.key();
        let mut monitors = [
            Monitor::new(Condition {
                key: key.clone(),
                direction: Direction::Above,
                threshold: 155.0,
            }),
            Monitor::new(Condition {
                key: key.clone(),
                direction: Direction::Below,
                threshold: 150.0,
            }),
        ];
        let mut alerts = Vec::new();
        for (read_at, price) in [154.0, 155.0, 156.0, 150.0, 155.0].into_iter().enumerate() {
            let quote = Quote {
                price,
                change: None,
                read_at: read_at as i64 + 1,
                name: None,
                listing: None,
            };
            evaluate(&mut monitors, &item, &key, &quote, &mut alerts);
        }
        assert_eq!(
            alerts.iter().map(|alert| alert.price).collect::<Vec<_>>(),
            [155.0, 150.0, 155.0]
        );
        assert_eq!(monitors[0].triggered_at, Some(5));
    }

    #[test]
    fn leverage_scales_return_and_quantity_scales_pnl() {
        let mut position = Position {
            side: Side::Long,
            entry_price: 150.0,
            quantity: 2.0,
            leverage: 5.0,
        };
        let long = position.metrics(165.0);
        assert_eq!((long.roi, long.pnl), (Some(50.0), Some(30.0)));
        position.side = Side::Short;
        let short = position.metrics(165.0);
        assert_eq!((short.roi, short.pnl), (Some(-50.0), Some(-30.0)));
    }
}
