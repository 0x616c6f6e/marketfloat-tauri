# MarketFloat Tauri

MarketFloat 的 Rust + Tauri 2 重构项目。复用原版 MarketFloat 的看板和悬停预览界面。Rust 负责原生窗口、托盘、行情请求、刷新调度、历史 K 线和设置存储。

## Windows 开发

需要 Rust MSVC 工具链、Visual Studio C++ Build Tools 和 Edge WebView2 Runtime。在 Visual Studio 的 x64 Developer PowerShell 中执行：

```powershell
cargo run --locked --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

## 验收步骤

1. 点击标题栏的收起按钮显示透明悬浮球。拖动球后再点击，恢复看板。
2. 点击标题栏的「−」隐藏到系统托盘。左键点击托盘恢复看板，右键可切换看板、悬浮球或退出。
3. 双击 `NVDAUSDT.P`，确认独立 TradingView 窗口显示 Binance 永续合约和 `15m`。悬停后可以查看 K 线和成交量。手动点击日 K、周 K 或月 K 后，独立窗口沿用该周期。
4. 点击 TradingView 窗口标题栏的「×」。看板仍可使用，再次双击可重新打开图表。
5. 在设置中切换系统代理、直连或手动代理。行情和历史 K 线使用新配置，已打开的 TradingView 窗口会关闭并在下次打开时使用新配置。手动代理支持 HTTP、HTTPS 和 SOCKS5。

## 行情

- A 股、美股默认使用腾讯财经。北交所 K 线优先使用东方财富。股票报价的更新速度取决于数据源。
- 加密货币现货使用 Binance USDT 或 Coinbase USD。Coinbase 的周 K、月 K 按 UTC 日历聚合日 K，周一开周。
- USDT 永续使用 Binance USDⓈ-M 合约。添加 `NVDAUSDT.P` 或 `BINANCE:NVDAUSDT.P` 后，报价和 K 线都使用该永续合约。
- 「设置 → 行情来源」可按市场配置 JSON 报价接口。支持 `{code}`、`{symbol}`、`{base}`、`{quote}` 和 `data[0].price` 等字段路径。可以先测试再保存。悬停 K 线继续使用内置历史行情源。
- 刷新频率可设为 3、5、10、15、30 或 60 秒。

价格监控、仓位收益率和 Electron 版设置导入待迁移。
