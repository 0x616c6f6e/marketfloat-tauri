(() => {
  const api = window.marketFloat;
  const $ = id => document.getElementById(id);
  const tabs = [...document.querySelectorAll('[data-setting-tab]')];
  let selectedTab = 'proxy';
  let savedProxy;

  function showTab(tab) {
    selectedTab = tab;
    for (const button of tabs) {
      const active = button.dataset.settingTab === tab;
      button.setAttribute('aria-pressed', String(active));
      $('settings-' + button.dataset.settingTab).hidden = !active;
    }
  }

  function close(focus = false) {
    $('settings-page').hidden = true;
    $('watch-page').hidden = false;
    $('watch-footer').hidden = false;
    $('settings-button').setAttribute('aria-expanded', 'false');
    if (focus) $('settings-button').focus();
  }

  function proxyMode() {
    $('proxy-manual').hidden = $('proxy-mode').value !== 'fixed_servers';
  }

  $('settings-button').addEventListener('click', () => {
    if (!$('settings-page').hidden) { close(); return; }
    window.chartPreview.hide();
    $('watch-page').hidden = true;
    $('watch-footer').hidden = true;
    $('settings-page').hidden = false;
    $('settings-button').setAttribute('aria-expanded', 'true');
    showTab(selectedTab);
  });
  $('settings-back').addEventListener('click', () => close(true));
  for (const button of tabs) button.addEventListener('click', () => showTab(button.dataset.settingTab));
  $('proxy-mode').addEventListener('change', proxyMode);
  $('proxy-form').addEventListener('submit', async event => {
    event.preventDefault();
    const submit = event.currentTarget.querySelector('button[type=submit]');
    const status = $('proxy-status');
    submit.disabled = true;
    status.classList.remove('error');
    status.textContent = '正在保存…';
    try {
      await api.saveProxy({ mode: $('proxy-mode').value, address: $('proxy-address').value });
      status.textContent = '代理已保存。';
    } catch (error) {
      status.textContent = String(error);
      status.classList.add('error');
    } finally {
      submit.disabled = false;
    }
  });
  $('pinned').addEventListener('change', async event => {
    const input = event.target;
    try { await api.setPinned(input.checked); }
    catch (error) { input.checked = !input.checked; $('status').textContent = String(error); }
  });
  window.marketFloatSettings = {
    close,
    update(state) {
      $('pinned').checked = state.pinned;
      const signature = JSON.stringify(state.proxy);
      if (signature === savedProxy) return;
      savedProxy = signature;
      $('proxy-mode').value = state.proxy.mode;
      $('proxy-address').value = state.proxy.address;
      proxyMode();
    },
  };
})();
