use crate::settings::{self, Period, Proxy, Settings};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::NewWindowResponse,
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

type Result<T> = std::result::Result<T, String>;

struct Instrument {
    key: &'static str,
    market: &'static str,
    code: &'static str,
    name: &'static str,
    currency: &'static str,
    ticker: &'static str,
}

const INSTRUMENTS: &[Instrument] = &[
    Instrument {
        key: "perp/NVDAUSDT.P",
        market: "perp",
        code: "NVDAUSDT.P",
        name: "",
        currency: "USDT",
        ticker: "BINANCE:NVDAUSDT.P",
    },
    Instrument {
        key: "us/NVDA",
        market: "us",
        code: "NVDA",
        name: "英伟达",
        currency: "USD",
        ticker: "NASDAQ:NVDA",
    },
    Instrument {
        key: "cn/SH600519",
        market: "cn",
        code: "SH600519",
        name: "贵州茅台",
        currency: "CNY",
        ticker: "SSE:600519",
    },
    Instrument {
        key: "crypto/BTC-USDT",
        market: "crypto",
        code: "BTC-USDT",
        name: "比特币",
        currency: "USDT",
        ticker: "BINANCE:BTCUSDT",
    },
];

struct Desktop {
    file: PathBuf,
    settings: Mutex<Settings>,
    orb_origin: Mutex<Option<PhysicalPosition<i32>>>,
    chart_operations: tauri::async_runtime::Mutex<()>,
}

fn failure(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn local_window(app: &AppHandle, label: &str) -> Result<WebviewWindow> {
    app.get_webview_window(label)
        .ok_or_else(|| format!("找不到窗口 {label}"))
}

fn show(app: &AppHandle, target: Option<&str>) -> Result<()> {
    for label in ["panel", "orb"] {
        if target != Some(label) {
            local_window(app, label)?.hide().map_err(failure)?;
        }
    }
    if let Some(label) = target {
        let window = local_window(app, label)?;
        window.unminimize().map_err(failure)?;
        window.show().map_err(failure)?;
        window.set_focus().map_err(failure)?;
    }
    Ok(())
}

fn snapshot_data(settings: &Settings) -> Value {
    let items: Vec<_> = INSTRUMENTS
        .iter()
        .map(|instrument| {
            json!({
                "key": instrument.key,
                "market": instrument.market,
                "code": instrument.code,
                "name": instrument.name,
                "currency": instrument.currency,
                "quote": null,
                "samples": [],
            })
        })
        .collect();
    json!({
        "items": items,
        "pinned": settings.pinned,
        "proxy": settings.proxy,
        "chartPeriod": settings.selected_period.unwrap_or(Period::Day),
    })
}

fn verify(window: &WebviewWindow, expected: &str) -> Result<()> {
    if window.label() == expected {
        Ok(())
    } else {
        Err("窗口无权执行此操作。".into())
    }
}

#[tauri::command]
fn snapshot(window: WebviewWindow, state: State<'_, Desktop>) -> Result<Value> {
    verify(&window, "panel")?;
    Ok(snapshot_data(
        &state.settings.lock().map_err(failure)?.clone(),
    ))
}

fn edit_settings(app: &AppHandle, edit: impl FnOnce(&mut Settings)) -> Result<()> {
    let state = app.state::<Desktop>();
    let mut current = state.settings.lock().map_err(failure)?;
    let mut next = current.clone();
    edit(&mut next);
    settings::save(&state.file, &next).map_err(failure)?;
    *current = next;
    app.emit_to("panel", "marketfloat-state", snapshot_data(&current))
        .map_err(failure)
}

#[tauri::command]
fn select_period(window: WebviewWindow, period: Period) -> Result<()> {
    verify(&window, "panel")?;
    edit_settings(window.app_handle(), |settings| {
        settings.selected_period = Some(period)
    })
}

#[tauri::command]
fn set_pinned(window: WebviewWindow, pinned: bool) -> Result<()> {
    verify(&window, "panel")?;
    edit_settings(window.app_handle(), |settings| settings.pinned = pinned)?;
    sync_floating_order(window.app_handle())
}

fn sync_floating_order(app: &AppHandle) -> Result<()> {
    let chart_focused = app
        .webview_windows()
        .iter()
        .any(|(label, chart)| label.starts_with("chart-") && chart.is_focused().unwrap_or(false));
    let pinned = app
        .state::<Desktop>()
        .settings
        .lock()
        .map_err(failure)?
        .pinned;
    local_window(app, "panel")?
        .set_always_on_top(pinned && !chart_focused)
        .map_err(failure)?;
    local_window(app, "orb")?
        .set_always_on_top(!chart_focused)
        .map_err(failure)
}

#[tauri::command]
async fn save_proxy(window: WebviewWindow, mut proxy: Proxy) -> Result<()> {
    verify(&window, "panel")?;
    proxy.validate()?;
    let app = window.app_handle();
    let desktop = app.state::<Desktop>();
    let _operation = desktop.chart_operations.lock().await;
    if app
        .state::<Desktop>()
        .settings
        .lock()
        .map_err(failure)?
        .proxy
        == proxy
    {
        return Ok(());
    }
    edit_settings(app, |settings| settings.proxy = proxy)?;
    for (label, chart) in app.webview_windows() {
        if label.starts_with("chart-") {
            chart.destroy().map_err(failure)?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn open_chart(window: WebviewWindow, key: String) -> Result<()> {
    verify(&window, "panel")?;
    let (index, instrument) = INSTRUMENTS
        .iter()
        .enumerate()
        .find(|(_, item)| item.key == key)
        .ok_or_else(|| "找不到标的。".to_owned())?;
    let app = window.app_handle();
    let desktop = app.state::<Desktop>();
    let _operation = desktop.chart_operations.lock().await;
    let settings = app
        .state::<Desktop>()
        .settings
        .lock()
        .map_err(failure)?
        .clone();
    let url = settings::tradingview_url(instrument.ticker, settings.selected_period);
    let label = format!("chart-{index}");
    if let Some(chart) = app.get_webview_window(&label) {
        if chart.url().map_err(failure)? != url {
            chart.navigate(url).map_err(failure)?;
        }
        chart.unminimize().map_err(failure)?;
        return chart.set_focus().map_err(failure);
    }

    // WebView2 environments with different proxy arguments require separate data folders.
    let browser_args = settings.proxy.browser_args();
    let profile = match &browser_args {
        None => "system".to_owned(),
        Some(arguments) => {
            // Keep the path short and stable for the same proxy settings.
            let hash = arguments.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
            format!("{hash:016x}")
        }
    };
    let data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(failure)?
        .join("tradingview")
        .join(profile);
    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title(format!("{} · TradingView", instrument.code))
        .inner_size(1140.0, 760.0)
        .center()
        .data_directory(data_dir)
        .on_navigation(|url| {
            url.scheme() == "https"
                && url.host_str().is_some_and(|host| {
                    host == "tradingview.com" || host.ends_with(".tradingview.com")
                })
        })
        .on_new_window(|_, _| NewWindowResponse::Deny);
    if let Some(arguments) = browser_args {
        builder = builder.additional_browser_args(&arguments);
    }
    builder.build().map_err(failure)?;
    Ok(())
}

#[tauri::command]
fn show_orb(window: WebviewWindow) -> Result<()> {
    verify(&window, "panel")?;
    show(window.app_handle(), Some("orb"))
}

#[tauri::command]
fn show_panel(window: WebviewWindow) -> Result<()> {
    verify(&window, "orb")?;
    show(window.app_handle(), Some("panel"))
}

#[tauri::command]
fn hide(window: WebviewWindow) -> Result<()> {
    if !matches!(window.label(), "panel" | "orb") {
        return Err("窗口无权执行此操作。".into());
    }
    show(window.app_handle(), None)
}

#[tauri::command]
fn orb_down(window: WebviewWindow) -> Result<()> {
    verify(&window, "orb")?;
    *window
        .state::<Desktop>()
        .orb_origin
        .lock()
        .map_err(failure)? = Some(window.outer_position().map_err(failure)?);
    Ok(())
}

#[tauri::command]
fn orb_move(window: WebviewWindow, dx: f64, dy: f64) -> Result<()> {
    verify(&window, "orb")?;
    if !dx.is_finite() || !dy.is_finite() {
        return Err("光标位置无效。".into());
    }
    let state = window.state::<Desktop>();
    let origin = *state.orb_origin.lock().map_err(failure)?;
    if let Some(origin) = origin {
        let scale = window.scale_factor().map_err(failure)?;
        window
            .set_position(PhysicalPosition::new(
                origin.x.saturating_add((dx * scale).round() as i32),
                origin.y.saturating_add((dy * scale).round() as i32),
            ))
            .map_err(failure)?;
    }
    Ok(())
}

#[tauri::command]
fn orb_up(window: WebviewWindow, moved: bool) -> Result<()> {
    verify(&window, "orb")?;
    *window
        .state::<Desktop>()
        .orb_origin
        .lock()
        .map_err(failure)? = None;
    if !moved {
        show(window.app_handle(), Some("panel"))?;
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            snapshot,
            select_period,
            set_pinned,
            save_proxy,
            open_chart,
            show_orb,
            show_panel,
            hide,
            orb_down,
            orb_move,
            orb_up
        ])
        .setup(|app| {
            let file = app.path().app_config_dir()?.join("desktop.json");
            let settings = settings::load(&file);
            let pinned = settings.pinned;
            app.manage(Desktop {
                file,
                settings: Mutex::new(settings),
                orb_origin: Mutex::new(None),
                chart_operations: tauri::async_runtime::Mutex::new(()),
            });
            WebviewWindowBuilder::new(app, "panel", WebviewUrl::App("panel.html".into()))
                .title("浮窗看盘 · Tauri")
                .inner_size(464.0, 670.0)
                .decorations(false)
                .always_on_top(pinned)
                .resizable(false)
                .center()
                .build()?;
            WebviewWindowBuilder::new(app, "orb", WebviewUrl::App("orb.html".into()))
                .title("看盘悬浮图标")
                .inner_size(62.0, 62.0)
                .position(24.0, 96.0)
                .decorations(false)
                .resizable(false)
                .transparent(true)
                .shadow(false)
                .skip_taskbar(true)
                .always_on_top(true)
                .visible(false)
                .build()?;

            let panel = MenuItem::with_id(app, "panel", "显示看板", true, None::<&str>)?;
            let orb = MenuItem::with_id(app, "orb", "显示悬浮球", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "隐藏到托盘", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&panel, &orb, &hide, &quit])?;
            let mut tray = TrayIconBuilder::with_id("marketfloat")
                .tooltip("浮窗看盘 · Tauri")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    let result = match event.id.as_ref() {
                        "panel" => show(app, Some("panel")),
                        "orb" => show(app, Some("orb")),
                        "hide" => show(app, None),
                        "quit" => {
                            app.exit(0);
                            return;
                        }
                        _ => return,
                    };
                    if let Err(error) = result {
                        eprintln!("{error}");
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Err(error) = show(tray.app_handle(), Some("panel")) {
                            eprintln!("{error}");
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label().starts_with("chart-")
                && matches!(event, WindowEvent::Focused(_) | WindowEvent::Destroyed)
            {
                let app = window.app_handle().clone();
                let _ = window.run_on_main_thread(move || {
                    if let Err(error) = sync_floating_order(&app) {
                        eprintln!("{error}");
                    }
                });
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                if matches!(window.label(), "panel" | "orb") {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("启动 MarketFloat Tauri 失败");
}
