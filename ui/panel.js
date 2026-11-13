const api = window.marketFloat;
const $ = selector => document.querySelector(selector);
const marketNames = { cn: 'A 股', us: '美股', crypto: '加密货币', perp: 'USDT 永续' };
const timeFormat = new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
const decimal = new Intl.NumberFormat('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const rows = new Map();
const filters = [...document.querySelectorAll('[data-market]')];
const list = $('#watchlist');
let state, filter = 'all';

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
  const direction = !pct || item.cached ? '' : (pct > 0) === state.redUp ? 'rise' : 'fall';
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
  row.setAttribute('aria-label', name + ' ' + displayCode + ' · ' + price + ' ' + item.currency + ' · 按 Enter 打开 TradingView');
  sparkline(row.querySelector('canvas'), item.samples, direction);
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
    empty.textContent = '该市场暂无示例标的';
    list.append(empty);
  }
  $('#count').textContent = visible.length + ' 项';
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

for (const button of filters) button.addEventListener('click', () => chooseMarket(button.dataset.market));
$('#collapse').addEventListener('click', api.collapse);
$('#hide').addEventListener('click', api.hide);
document.addEventListener('keydown', event => {
  if (document.querySelector('dialog[open]')) return;
  if (event.key === 'Escape') {
    if (window.chartPreview.hide()) return;
    if (!$('#settings-page').hidden) window.marketFloatSettings.close(true);
    else api.collapse();
  }
});
api.subscribe(setState).then(() => api.snapshot()).then(setState).catch(error => {
  $('#status').textContent = String(error);
});
