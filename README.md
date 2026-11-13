# MarketFloat Tauri

MarketFloat 的 Rust + Tauri 2 重构项目。复用原版 MarketFloat 的看板和悬停预览界面。Rust 负责原生窗口、托盘、行情请求、刷新调度、历史 K 线和设置存储。

## Windows 开发

需要 Rust MSVC 工具链、Visual Studio C++ Build Tools 和 Edge WebView2 Runtime。在 Visual Studio 的 x64 Developer PowerShell 中执行：

```powershell
cargo run --locked --manifest-path src-tauri/Cargo.toml
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

## GitHub CI/CD

[Windows 工作流](.github/workflows/windows.yml)在分支推送、Pull Request 和手动运行时检查 Rust 格式、执行 Rust 测试、检查前端脚本语法，并构建 Windows x64 的 NSIS `setup.exe`、MSI 和免安装 ZIP。产物和 `SHA256SUMS.txt` 可在 **Actions → 对应运行 → Artifacts** 下载。

### 免安装版

解压 `MarketFloat-Tauri-<版本>-x64-portable.zip`，双击 `MarketFloat-Tauri.exe`。运行需要系统已有 Edge WebView2 Runtime。设置仍保存在当前 Windows 用户的应用数据目录。需要 Windows 原生价格通知时请使用安装版，免安装版可在看板内查看提醒和触发记录。

也可在 PowerShell 7 中从现有 x64 release 可执行文件生成 ZIP：

```powershell
./scripts/Pack-Portable.ps1 -BinaryPath src-tauri/target/x86_64-pc-windows-msvc/release/marketfloat-tauri.exe -OutputDirectory release-files
```

### 发布

将此仓库推送到 GitHub 后，推送与 `src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 版本一致的标签，即可自动发布到 **Releases**。发布任务使用 GitHub 提供的 `GITHUB_TOKEN`。

```powershell
git push -u origin main
git tag v0.1.2
git push origin v0.1.2
```

## 验收步骤

1. 点击标题栏的收起按钮显示透明悬浮球。悬浮球可拖动，左键恢复看板，右键可展开看板、隐藏到托盘或退出。
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
- 在「设置 → 常规 → 涨跌配色」可切换红涨绿跌或绿涨红跌。看板涨跌幅、走势、仓位收益率与悬停 K 线的蜡烛和成交量柱使用同一配色。独立 TradingView 窗口使用 TradingView 自己的配色设置。

## 价格提醒与合约仓位

- 右键自选标的或点击行尾「⋯」，选择「设置价格监控」。可按大于等于或小于等于目标价提醒，在设置中编辑、停用或删除规则。价格离开条件后，再次满足才会重新提醒。应用在后台时继续监控。
- 同一菜单可录入多空方向、开仓价、标的数量和杠杆。每个标的保存一条仓位，看板随报价刷新估算收益率。点击收益率可编辑或移除，悬停可看浮动盈亏。计算按线性合约口径，未计手续费和资金费。
- 触发后保留最近触发时间和价格。看板显示提示，安装版可通过 Windows 通知接收提醒。

Electron 版设置导入待迁移。
