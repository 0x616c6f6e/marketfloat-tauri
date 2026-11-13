(() => {
  const api = window.marketFloat;
  const $ = id => document.getElementById(id);
  const tabs = [...document.querySelectorAll('[data-setting-tab]')];
  const sources = {
    cn: { example: '600519', description: '腾讯财经 · 涨跌幅相对昨收。' },
    us: { example: 'AAPL', description: '腾讯财经 · 涨跌幅相对昨收，报价可能延迟。' },
    usd: { example: 'BTC-USD', description: 'Coinbase · 24 小时涨跌幅。' },
    usdt: { example: 'BTC-USDT', description: 'Binance · 24 小时涨跌幅。' },
    perp: { example: 'NVDAUSDT.P', description: 'Binance 永续 · 24 小时涨跌幅。' },
  };
  const fields = ['url', 'price', 'change', 'previous', 'name'];
  let state;
  let selectedTab = 'source';
  let lastSource, lastProxy;

  function status(id, message, error = false) {
    const element = $(id);
    element.textContent = message;
    element.classList.toggle('error', error);
  }

  async function action(buttons, statusId, operation, success) {
    buttons.forEach(button => { button.disabled = true; });
    status(statusId, '正在处理…');
    try {
      const result = await operation();
      if (result?.error) status(statusId, result.error, true);
      else status(statusId, success(result));
      return !result?.error;
    } catch (error) {
      status(statusId, String(error), true);
      return false;
    } finally {
      buttons.forEach(button => { button.disabled = false; });
    }
  }

  function selectTab(tab) {
    selectedTab = tab;
    for (const button of tabs) {
      const name = button.dataset.settingTab;
      button.setAttribute('aria-pressed', String(name === tab));
      $('settings-' + name).hidden = name !== tab;
    }
    $('settings-page').querySelector('.settings-body').scrollTop = 0;
  }

  function open(tab = selectedTab) {
    window.chartPreview.hide();
    $('watch-page').hidden = true;
    $('watch-footer').hidden = true;
    $('settings-page').hidden = false;
    $('settings-button').setAttribute('aria-expanded', 'true');
    selectTab(tab);
  }

  function close(focus = false) {
    if ($('settings-page').hidden) return;
    $('settings-page').hidden = true;
    $('watch-page').hidden = false;
    $('watch-footer').hidden = false;
    $('settings-button').setAttribute('aria-expanded', 'false');
    if (focus) $('settings-button').focus();
  }

  function renderSourceMode() {
    const custom = $('source-mode').value === 'custom';
    $('source-custom').hidden = !custom;
    $('source-info').hidden = custom;
    $('source-info').textContent = sources[$('source-group').value].description;
  }

  function renderSource() {
    const group = $('source-group').value;
    const source = state?.sources[group];
    const signature = JSON.stringify([group, source]);
    if (signature === lastSource) return;
    lastSource = signature;
    $('source-mode').value = source ? 'custom' : 'builtin';
    fields.forEach(field => { $('source-' + field).value = source?.[field] ?? ''; });
    $('source-test-code').value = '';
    $('source-test-code').placeholder = sources[group].example;
    renderSourceMode();
  }

  function sourceInput() {
    if ($('source-mode').value === 'builtin') return null;
    return Object.fromEntries(fields.map(field => [field, $('source-' + field).value]));
  }

  function proxyMode() {
    $('proxy-manual').hidden = $('proxy-mode').value !== 'fixed_servers';
  }

  function renderProxy() {
    const signature = JSON.stringify(state.proxy);
    if (signature === lastProxy) return;
    lastProxy = signature;
    $('proxy-mode').value = state.proxy.mode;
    $('proxy-address').value = state.proxy.address;
    proxyMode();
  }

  function update(value) {
    state = value;
    $('pinned').checked = state.pinned;
    $('interval').value = state.interval;
    renderSource();
    renderProxy();
  }

  $('settings-button').addEventListener('click', () => $('settings-page').hidden ? open() : close(true));
  $('settings-back').addEventListener('click', () => close(true));
  tabs.forEach(button => button.addEventListener('click', () => selectTab(button.dataset.settingTab)));
  $('source-group').addEventListener('change', () => { renderSource(); status('source-status', ''); });
  $('source-mode').addEventListener('change', () => { renderSourceMode(); status('source-status', ''); });
  const sourceButtons = [$('source-test'), $('source-form').querySelector('[type="submit"]'), $('source-group'), $('source-mode')];
  $('source-test').addEventListener('click', () => void action(sourceButtons, 'source-status',
    () => api.testSource($('source-group').value, sourceInput(), $('source-test-code').value),
    result => '最新价 ' + result.price +
      (Number.isFinite(result.change) ? ' · ' + result.change.toFixed(2) + '%' : '')));
  $('source-form').addEventListener('submit', event => {
    event.preventDefault();
    if (sourceButtons[1].disabled) return;
    void action(sourceButtons, 'source-status', () => api.saveSource($('source-group').value, sourceInput()), () => '来源已保存。');
  });
  $('proxy-mode').addEventListener('change', () => { proxyMode(); status('proxy-status', ''); });
  $('proxy-form').addEventListener('submit', event => {
    event.preventDefault();
    const submit = $('proxy-form').querySelector('[type="submit"]');
    if (submit.disabled) return;
    void action([submit, $('proxy-mode'), $('proxy-address')], 'proxy-status',
      () => api.saveProxy({ mode: $('proxy-mode').value, address: $('proxy-address').value }), () => '代理已保存。');
  });
  $('settings-general').addEventListener('change', event => {
    if (event.target.id === 'pinned') api.setPinned(event.target.checked).catch(error => { event.target.checked = !event.target.checked; alert(String(error)); });
    if (event.target.id === 'interval') api.setInterval(Number(event.target.value)).catch(error => { event.target.value = state.interval; alert(String(error)); });
  });
  window.marketFloatSettings = { update, close };
})();
