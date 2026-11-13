# MarketFloat Tauri

MarketFloat 的 Rust + Tauri 2 重构项目。第一阶段复用了原版 MarketFloat v2.6.0 的 HTML、CSS、看板和悬停预览界面。Rust 负责原生窗口、托盘和代理设置，前端通过 Tauri IPC 与 Rust 通信。

## Windows 开发

需要 Rust MSVC 工具链、Visual Studio C++ Build Tools 和 Edge WebView2 Runtime。在 Visual Studio 的 x64 Developer PowerShell 中执行：

```powershell
cargo run --locked --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

## 验收步骤

1. 点击标题栏的收起按钮显示透明悬浮球。拖动球后再点击，恢复看板。
2. 点击标题栏的「−」隐藏到系统托盘。左键点击托盘恢复看板，右键可切换看板、悬浮球或退出。
3. 双击 `NVDAUSDT.P`，确认独立 TradingView 窗口显示 Binance 永续合约和 `15m`。手动点击悬停预览中的日 K、周 K 或月 K 后，独立窗口沿用该周期。
4. 点击 TradingView 窗口标题栏的「×」。看板仍可使用，再次双击可重新打开图表。
5. 在设置中切换系统代理、直连或手动代理。保存更改会关闭当前图表，再打开时使用新配置。手动代理支持 HTTP、HTTPS 和 SOCKS5。

第一阶段的看板使用示例标的。行情和历史 K 线数据、价格监控、仓位收益率以及旧版数据导入将在后续迁移。代理设置目前用于独立 TradingView 窗口。
