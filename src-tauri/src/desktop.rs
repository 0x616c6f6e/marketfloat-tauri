use crate::{
    market::{self, History, Instrument, Market, Quote, Source, SourceGroup},
    market_state::MarketData,
    settings::{self, Period, Proxy, Settings},
};
use serde_json::Value;
use std::time::Duration;
use std::{path::PathBuf, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::NewWindowResponse,
    AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tokio::sync::Notify;

type Result<T> = std::result::Result<T, String>;

struct Desktop {
    file: PathBuf,
    data: Mutex<MarketData>,
    refresh: Notify,
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
    snapshot_data(&state)
}

fn snapshot_data(state: &Desktop) -> Result<Value> {
    let settings = state.settings.lock().map_err(failure)?;
    Ok(state.data.lock().map_err(failure)?.snapshot(&settings))
}

fn publish(app: &AppHandle) -> Result<()> {
    app.emit_to(
        "panel",
        "marketfloat-state",
        snapshot_data(&app.state::<Desktop>())?,
    )
    .map_err(failure)
}

fn edit_market(
    app: &AppHandle,
    edit: impl FnOnce(&mut Settings) -> Result<()>,
    update: impl FnOnce(&mut MarketData, &Settings),
) -> Result<()> {
    let state = app.state::<Desktop>();
    let mut current = state.settings.lock().map_err(failure)?;
    let mut next = current.clone();
    edit(&mut next)?;
    settings::save(&state.file, &next).map_err(failure)?;
    update(&mut *state.data.lock().map_err(failure)?, &next);
    *current = next;
    drop(current);
    publish(app)
}

fn edit_settings(app: &AppHandle, edit: impl FnOnce(&mut Settings)) -> Result<()> {
    edit_market(
        app,
        |settings| {
            edit(settings);
            Ok(())
        },
        |_, _| {},
    )
}

fn find_item<'a>(settings: &'a Settings, key: &str) -> Result<&'a Instrument> {
    settings
        .watchlist
        .iter()
        .find(|item| item.key() == key)
        .ok_or_else(|| "找不到标的。".into())
}

async fn refresh_loop(app: AppHandle) {
    let desktop = app.state::<Desktop>();
    loop {
        let request = (|| {
            let settings = desktop.settings.lock().ok()?;
            let mut data = desktop.data.lock().ok()?;
            data.set_refreshing(!settings.watchlist.is_empty());
            Some((settings.clone(), data.client.clone(), data.revision))
        })();
        let Some((settings, client, revision)) = request else {
            return;
        };
        let started = tokio::time::Instant::now();
        let _ = publish(&app);
        let results = market::quotes(&client, &settings.watchlist, &settings.sources).await;
        {
            let Ok(mut data) = desktop.data.lock() else {
                return;
            };
            if data.revision == revision {
                for (key, result) in results {
                    data.update(key, result);
                }
            }
            data.set_refreshing(false);
        }
        let _ = publish(&app);
        let wait = (started + Duration::from_secs(settings.interval))
            .saturating_duration_since(tokio::time::Instant::now())
            .max(Duration::from_millis(250));
        tokio::select! {
            _ = desktop.refresh.notified() => {}
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

#[tauri::command]
fn refresh_quotes(window: WebviewWindow, state: State<'_, Desktop>) -> Result<()> {
    verify(&window, "panel")?;
    state.refresh.notify_one();
    Ok(())
}

#[tauri::command]
fn set_interval(window: WebviewWindow, interval: u64) -> Result<()> {
    verify(&window, "panel")?;
    if !settings::INTERVALS.contains(&interval) {
        return Err("刷新频率无效。".into());
    }
    edit_settings(window.app_handle(), |settings| settings.interval = interval)?;
    window.state::<Desktop>().refresh.notify_one();
    Ok(())
}

#[tauri::command]
fn add_item(window: WebviewWindow, market: Market, code: String) -> Result<Market> {
    verify(&window, "panel")?;
    let item = Instrument::parse(market, &code, "").ok_or("请按示例输入有效代码。")?;
    let market = item.market;
    edit_market(
        window.app_handle(),
        |settings| {
            if settings
                .watchlist
                .iter()
                .any(|saved| saved.key() == item.key())
            {
                return Err("此标的已在自选中。".into());
            }
            if settings.watchlist.len() >= settings::MAX_ITEMS {
                return Err("最多可添加 40 个标的。".into());
            }
            settings.watchlist.push(item);
            Ok(())
        },
        |data, _| data.invalidate_quotes(),
    )?;
    window.state::<Desktop>().refresh.notify_one();
    Ok(market)
}

#[tauri::command]
async fn remove_item(window: WebviewWindow, key: String) -> Result<()> {
    verify(&window, "panel")?;
    let app = window.app_handle();
    let desktop = app.state::<Desktop>();
    let _operation = desktop.chart_operations.lock().await;
    edit_market(
        app,
        |settings| {
            find_item(settings, &key)?;
            settings.watchlist.retain(|item| item.key() != key);
            Ok(())
        },
        |data, _| {
            data.remove(&key);
            data.invalidate_quotes();
        },
    )?;
    if let Some(chart) = app.get_webview_window(&chart_label(&key)) {
        chart.destroy().map_err(failure)?;
    }
    desktop.refresh.notify_one();
    Ok(())
}

#[tauri::command]
fn save_source(
    window: WebviewWindow,
    group: SourceGroup,
    mut source: Option<Source>,
) -> Result<()> {
    verify(&window, "panel")?;
    if let Some(source) = &mut source {
        source.validate()?;
    }
    edit_market(
        window.app_handle(),
        |settings| {
            if let Some(source) = source {
                settings.sources.insert(group, source);
            } else {
                settings.sources.remove(&group);
            }
            Ok(())
        },
        |data, settings| data.change_source(&settings.watchlist, group),
    )?;
    window.state::<Desktop>().refresh.notify_one();
    Ok(())
}

#[tauri::command]
async fn test_source(
    window: WebviewWindow,
    group: SourceGroup,
    mut source: Option<Source>,
    code: String,
) -> Result<Quote> {
    verify(&window, "panel")?;
    if let Some(source) = &mut source {
        source.validate()?;
    }
    let (market, sample) = group.sample();
    let code = if code.trim().is_empty() {
        sample
    } else {
        &code
    };
    let item = Instrument::parse(market, code, "")
        .filter(|item| item.group() == group)
        .ok_or("测试代码与选定市场不匹配。")?;
    let client = window
        .state::<Desktop>()
        .data
        .lock()
        .map_err(failure)?
        .client
        .clone();
    market::single_quote(&client, &item, source.as_ref()).await
}

#[tauri::command]
async fn chart_preview(window: WebviewWindow, key: String, period: Period) -> Result<History> {
    verify(&window, "panel")?;
    let desktop = window.state::<Desktop>();
    let cache_key = (key, period);
    let (item, client, listing, revision) = {
        let settings = desktop.settings.lock().map_err(failure)?;
        let item = find_item(&settings, &cache_key.0)?.clone();
        let data = desktop.data.lock().map_err(failure)?;
        if let Some(result) = data.cached_history(&cache_key) {
            return result;
        }
        (
            item,
            data.client.clone(),
            data.listing(&cache_key.0),
            data.revision,
        )
    };
    let result = market::history(&client, &item, listing.as_deref(), period).await;
    let mut data = desktop.data.lock().map_err(failure)?;
    if data.revision == revision {
        data.store_history(cache_key, result.clone());
    }
    result
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
    let client = market::client(&proxy)?;
    edit_market(
        app,
        |settings| {
            settings.proxy = proxy;
            Ok(())
        },
        |data, _| data.use_client(client),
    )?;
    desktop.refresh.notify_one();
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
    let app = window.app_handle();
    let desktop = app.state::<Desktop>();
    let _operation = desktop.chart_operations.lock().await;
    let settings = app
        .state::<Desktop>()
        .settings
        .lock()
        .map_err(failure)?
        .clone();
    let instrument = find_item(&settings, &key)?;
    let (mut listing, client) = {
        let data = desktop.data.lock().map_err(failure)?;
        (data.listing(&key), data.client.clone())
    };
    if instrument.market == Market::Us && listing.is_none() {
        listing = tokio::time::timeout(
            Duration::from_secs(3),
            market::single_quote(&client, instrument, None),
        )
        .await
        .ok()
        .and_then(Result::ok)
        .and_then(|quote| quote.listing);
    }
    let ticker = instrument.tradingview_symbol(listing.as_deref());
    let url = settings::tradingview_url(&ticker, settings.selected_period);
    let label = chart_label(&key);
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

fn chart_label(key: &str) -> String {
    format!("chart-{}", key.replace(['/', '.'], "_"))
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
            refresh_quotes,
            chart_preview,
            save_source,
            test_source,
            set_interval,
            add_item,
            remove_item,
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
                data: Mutex::new(MarketData::new(&settings.proxy)?),
                refresh: Notify::new(),
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
            tauri::async_runtime::spawn(refresh_loop(app.handle().clone()));
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
