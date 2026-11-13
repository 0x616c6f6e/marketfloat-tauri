const api = window.marketFloat;
const $ = selector => document.querySelector(selector);
const marketNames = { cn: 'A 股', us: '美股', crypto: '加密货币', perp: 'USDT 永续' };
const examples = { cn: '例如 600519、SH000001、BJ920002', us: '例如 AAPL、BRK.B、SPY', crypto: '例如 BTC-USDT、ETH-USD', perp: '例如 NVDAUSDT.P、BTCUSDT.P' };
const timeFormat = new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
const decimal = new Intl.NumberFormat('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const signed = new Intl.NumberFormat('en-US', { signDisplay: 'exceptZero', minimumFractionDigits: 2, maximumFractionDigits: 2 });
const rows = new Map();
const filters = [...document.querySelectorAll('[data-market]')];
const list = $('#watchlist');
const dialog = $('#add-dialog');
const positionDialog = $('#position-dialog');
const removeDialog = $('#remove-dialog');
const quoteMenu = $('#quote-menu');
let state, filter = 'all', positionKey, menuKey, removeKey, alertTimer;

function priceText(value) {
  if (value >= 1) return decimal.format(value);
  if (value >= 0.01) return value.toFixed(4);
  if (value >= 1e-8) return value.toFixed(8);
  return value.toExponential(3);
}

function sparkline(canvas, samples, direction) {
  const g = canvas.getContext('2d');
  const { width, height } = canvas;
  g.clearRect(0, 0, width, height);
  g.strokeStyle = { rise: '#ff899d', fall: '#74dfac' }[direction] ?? '#7185a2';
  g.lineWidth = 2.6;
  g.lineCap = 'round';
  g.lineJoin = 'round';
  g.beginPath();
  if (samples.length < 2) {
    g.moveTo(width / 2 - 3, height / 2);
    g.lineTo(width / 2 + 3, height / 2);
  } else {
    const low = Math.min(...samples);
    const range = Math.max(...samples) - low;
    samples.forEach((price, i) => {
      const x = 3 + (width - 6) * i / (samples.length - 1);
      const y = range ? height - 5 - (height - 10) * (price - low) / range : height / 2;
      if (i === 0) g.moveTo(x, y);
      else g.lineTo(x, y);
    });
  }
  g.stroke();
}

function updateRow(row, item) {
  const quote = item.quote;
  const displayCode = item.market === 'cn' ? item.code.slice(2) : item.code;
  const name = item.name || quote?.name || displayCode;
  const context = item.cached ? '缓存' : item.error ? '未获取' : marketNames[item.market];
  const price = quote ? priceText(quote.price) : '—';
  const pct = quote?.change;
  const direction = !pct || item.cached ? '' : pct > 0 ? 'rise' : 'fall';
  row.classList.toggle('stale', Boolean(item.cached || item.error));
  row.querySelector('.name').textContent = name;
  row.querySelector('.code').textContent = name === displayCode ? context : displayCode + ' · ' + context;
  const priceNode = row.querySelector('.price');
  priceNode.textContent = price;
  priceNode.classList.toggle('long', price.length > 9);
  row.querySelector('.currency').textContent = item.currency;
  const change = row.querySelector('.change');
  change.className = 'change ' + direction;
  change.textContent = Number.isFinite(pct) ? (pct > 0 ? '+' : '') + pct.toFixed(2) + '%' : '—';
  const positionButton = row.querySelector('.position-return');
  positionButton.hidden = !item.position;
  if (item.position) {
    const roi = item.positionMetrics?.roi;
    const pnl = item.positionMetrics?.pnl;
    const side = item.position.side === 'long' ? '多仓' : '空仓';
    const result = Number.isFinite(roi) ? signed.format(roi) + '%' : '—';
    positionButton.textContent = side + ' ' + result;
    positionButton.className = 'position-return' + (!roi || item.cached ? '' : roi > 0 ? ' rise' : ' fall');
    positionButton.title = side + '估算收益率 ' + result +
      (Number.isFinite(pnl) ? ' · 浮动盈亏 ' + signed.format(pnl) + ' ' + item.currency : '') +
      (item.cached ? ' · 缓存报价' : '');
    positionButton.setAttribute('aria-label', positionButton.title + '，编辑合约仓位');
  }
  row.querySelector('.row-menu').setAttribute('aria-label', name + '选项');
  row.setAttribute('aria-label', name + ' ' + displayCode + ' · ' + price + ' ' + item.currency + ' · 按 Enter 打开 TradingView');
  sparkline(row.querySelector('canvas'), item.samples ?? [], direction);
}

function render() {
  if (!state) return;
  filters.forEach(button => button.setAttribute('aria-pressed', String(button.dataset.market === filter)));
  const visible = state.items.filter(item => filter === 'all' || item.market === filter);
  const keys = new Set(visible.map(item => item.key));
  for (const [key, row] of rows) {
    if (keys.has(key)) continue;
    window.chartPreview.hide(row);
    row.remove();
    rows.delete(key);
  }
  list.querySelector('.empty')?.remove();
  for (const item of visible) {
    let row = rows.get(item.key);
    if (!row) {
      row = $('#row-template').content.firstElementChild.cloneNode(true);
      row.addEventListener('contextmenu', event => {
        event.preventDefault();
        openQuoteMenu(item.key, event.clientX, event.clientY);
      });
      row.querySelector('.row-menu').addEventListener('click', event => {
        const rect = event.currentTarget.getBoundingClientRect();
        openQuoteMenu(item.key, rect.right, rect.bottom);
      });
      row.querySelector('.position-return').addEventListener('click', () => showPosition(item.key));
      row.dataset.key = item.key;
      window.chartPreview.attach(row, () => ({ item: state.items.find(entry => entry.key === item.key), redUp: true }));
      rows.set(item.key, row);
      list.append(row);
    }
    updateRow(row, item);
  }
  if (!visible.length) {
    const empty = document.createElement('li');
    empty.className = 'empty';
    empty.textContent = '暂无自选，点击「添加」关注标的';
    list.append(empty);
  }
  $('#count').textContent = visible.length + ' 项';
  $('#frequency').textContent = state.interval + ' 秒刷新';
  $('#refresh').textContent = state.refreshing ? '更新中…' : '↻ 刷新';
  let status = '等待获取行情';
  let indicator = '';
  if (state.refreshing) status = '正在获取行情…';
  else if (!state.items.length) status = '自选列表为空';
  else if (state.items.every(item => !item.quote || item.cached)) {
    status = state.items.some(item => item.cached) ? '行情连接失败 · 显示缓存' : '行情连接失败 · 等待重试';
    indicator = 'warn';
  } else {
    const latest = Math.max(...state.items.map(item => item.quote?.readAt ?? 0));
    const errors = state.items.filter(item => item.error).length;
    status = '更新 ' + timeFormat.format(latest) + (errors ? ' · ' + errors + ' 项未获取' : '');
    indicator = errors ? 'warn' : 'ok';
  }
  $('#status').textContent = status;
  $('#indicator').className = 'indicator ' + indicator;
}

function setState(value) {
  state = value;
  window.chartPreview.setPeriod(value.chartPeriod);
  window.marketFloatSettings.update(value);
  render();
}

function chooseMarket(value) {
  window.chartPreview.hide();
  filter = value;
  list.scrollTop = 0;
  render();
}

function openQuoteMenu(key, x, y) {
  const item = state.items.find(item => item.key === key);
  if (!item) return;
  window.chartPreview.hide();
  menuKey = key;
  quoteMenu.querySelector('[data-quote-action="position"]').textContent =
    item.position ? '编辑合约仓位' : '录入合约仓位';
  if (!quoteMenu.matches(':popover-open')) quoteMenu.showPopover();
  const rect = quoteMenu.getBoundingClientRect();
  quoteMenu.style.left = Math.max(8, Math.min(x, innerWidth - rect.width - 8)) + 'px';
  quoteMenu.style.top = Math.max(8, Math.min(y, innerHeight - rect.height - 8)) + 'px';
  quoteMenu.querySelector('button').focus();
}

quoteMenu.addEventListener('click', event => {
  const operation = event.target.closest('[data-quote-action]')?.dataset.quoteAction;
  if (!operation) return;
  quoteMenu.hidePopover();
  const item = state.items.find(item => item.key === menuKey);
  if (!item) return;
  if (operation === 'chart') api.openChart(item.key);
  else if (operation === 'monitor') window.marketFloatSettings.open('monitor', item.key);
  else if (operation === 'position') showPosition(item.key);
  else if (operation === 'remove') showRemove(item);
});

function showRemove(item) {
  if (document.querySelector('dialog[open]')) return;
  const linked = [item.position && '仓位', state.monitors.some(rule => rule.key === item.key) && '价格提醒']
    .filter(Boolean).join('和');
  removeKey = item.key;
  $('#remove-description').textContent = '将 ' + item.code + ' 移出自选？' +
    (linked ? '关联的' + linked + '也会删除。' : '');
  $('#remove-error').hidden = true;
  removeDialog.showModal();
}

$('#remove-cancel').addEventListener('click', () => removeDialog.close());
removeDialog.addEventListener('cancel', event => {
  if ($('#remove-confirm').disabled) event.preventDefault();
});
removeDialog.addEventListener('close', () => {
  if (removeDialog.open) return;
  const trigger = rows.get(removeKey)?.querySelector('.row-menu') ?? $('#add');
  removeKey = undefined;
  trigger.focus();
});
$('#remove-form').addEventListener('submit', async event => {
  event.preventDefault();
  const confirm = $('#remove-confirm');
  const cancel = $('#remove-cancel');
  if (!removeDialog.open || confirm.disabled) return;
  confirm.disabled = cancel.disabled = true;
  const error = $('#remove-error');
  error.hidden = true;
  try {
    await api.remove(removeKey);
    removeDialog.close();
  } catch (failure) {
    error.textContent = String(failure);
    error.hidden = false;
  } finally {
    confirm.disabled = cancel.disabled = false;
    if (removeDialog.open) cancel.focus();
  }
});

function hint() {
  $('#add-hint').classList.remove('error');
  $('#add-hint').textContent = examples[$('#new-market').value];
}

function showAdd() {
  if (dialog.open) return;
  window.marketFloatSettings.close();
  window.chartPreview.hide();
  $('#new-market').value = filter === 'all' ? 'cn' : filter;
  $('#new-code').value = '';
  hint();
  dialog.showModal();
  $('#new-code').focus();
}

function showPosition(key) {
  const item = state?.items.find(entry => entry.key === key);
  if (!item || document.querySelector('dialog[open]')) return;
  window.chartPreview.hide();
  positionKey = key;
  $('#position-symbol').textContent = item.code;
  $('#position-currency').textContent = '(' + item.currency + ')';
  const base = item.base ?? (item.market === 'crypto' ? item.code.split('-')[0] : '');
  $('#position-quantity-unit').textContent = base ? '(' + base + ')' : '';
  $('#position-side').value = item.position?.side ?? 'long';
  $('#position-entry').value = item.position?.entryPrice ?? '';
  $('#position-quantity').value = item.position?.quantity ?? '';
  $('#position-leverage').value = item.position?.leverage ?? 1;
  $('#position-remove').hidden = !item.position;
  $('#position-error').hidden = true;
  positionDialog.showModal();
  $('#position-entry').focus();
}

async function savePosition(input) {
  const save = $('#position-save');
  const remove = $('#position-remove');
  if (save.disabled) return;
  save.disabled = remove.disabled = true;
  const error = $('#position-error');
  error.hidden = true;
  try {
    if (input && (!Number.isFinite(input.entryPrice) || input.entryPrice <= 0 ||
        !Number.isFinite(input.quantity) || input.quantity <= 0 ||
        !Number.isFinite(input.leverage) || input.leverage < 1)) {
      throw new Error('请填写大于零的开仓价和数量，杠杆至少为 1 倍。');
    }
    await api.savePosition(positionKey, input);
    positionDialog.close();
  } catch (failure) {
    error.textContent = String(failure);
    error.hidden = false;
  } finally {
    save.disabled = remove.disabled = false;
  }
}

for (const button of filters) button.addEventListener('click', () => chooseMarket(button.dataset.market));
$('#add').addEventListener('click', showAdd);
$('#add-cancel').addEventListener('click', () => dialog.close());
$('#new-market').addEventListener('change', hint);
$('#add-form').addEventListener('submit', async event => {
  event.preventDefault();
  const submit = event.currentTarget.querySelector('[type="submit"]');
  submit.disabled = true;
  const market = $('#new-market').value;
  try {
    const selected = await api.add(market, $('#new-code').value);
    if (filter !== 'all' && filter !== selected) chooseMarket(selected);
    list.scrollTop = list.scrollHeight;
    dialog.close();
  } catch (error) {
    $('#add-hint').classList.add('error');
    $('#add-hint').textContent = String(error);
    $('#new-code').focus();
  } finally { submit.disabled = false; }
});
$('#position-cancel').addEventListener('click', () => positionDialog.close());
$('#position-remove').addEventListener('click', () => void savePosition(null));
$('#position-form').addEventListener('submit', event => {
  event.preventDefault();
  void savePosition({
    side: $('#position-side').value, entryPrice: $('#position-entry').valueAsNumber,
    quantity: $('#position-quantity').valueAsNumber, leverage: $('#position-leverage').valueAsNumber,
  });
});
$('#collapse').addEventListener('click', api.collapse);
$('#hide').addEventListener('click', api.hide);
$('#refresh').addEventListener('click', api.refresh);
document.addEventListener('keydown', event => {
  if (event.key === 'F5') { event.preventDefault(); api.refresh(); }
  if (document.querySelector('dialog[open]')) return;
  if (event.key === 'Escape') {
    if (quoteMenu.matches(':popover-open')) return;
    if (window.chartPreview.hide()) return;
    if (!$('#settings-page').hidden) window.marketFloatSettings.close(true);
    else api.collapse();
  }
  if (event.ctrlKey && event.key.toLowerCase() === 'n') { event.preventDefault(); showAdd(); }
});
api.subscribe(setState).then(() => api.snapshot()).then(setState).catch(error => {
  $('#status').textContent = String(error);
});
api.onAlert(message => {
  const alert = $('#price-alert');
  alert.textContent = message;
  alert.hidden = false;
  clearTimeout(alertTimer);
  alertTimer = setTimeout(() => { alert.hidden = true; }, 9000);
}).catch(error => { $('#status').textContent = String(error); });
