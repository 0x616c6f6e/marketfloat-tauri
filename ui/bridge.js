(() => {
  const { core, event } = window.__TAURI__;
  const invoke = (command, args) => core.invoke(command, args);
  const report = error => {
    const status = document.querySelector('#status');
    if (status) status.textContent = String(error);
    else console.error(error);
  };
  const send = (command, args) => { void invoke(command, args).catch(report); };
  window.marketFloat = {
    snapshot: () => invoke('snapshot'),
    subscribe: async callback => {
      await event.listen('marketfloat-state', ({ payload }) => callback(payload));
    },
    openChart: key => send('open_chart', { key }),
    chartPreview: (key, period) => invoke('chart_preview', { key, period })
      .catch(error => ({ error: String(error) })),
    selectPeriod: period => send('select_period', { period }),
    setPinned: pinned => invoke('set_pinned', { pinned }),
    setInterval: interval => invoke('set_interval', { interval }),
    refresh: () => send('refresh_quotes'),
    add: (market, code) => invoke('add_item', { market, code }),
    remove: key => invoke('remove_item', { key }),
    saveSource: (group, source) => invoke('save_source', { group, source }),
    testSource: (group, source, code) => invoke('test_source', { group, source, code }),
    saveProxy: proxy => invoke('save_proxy', { proxy }),
    collapse: () => send('show_orb'),
    hide: () => send('hide'),
    orbOpen: () => send('show_panel'),
    orbDown: () => send('orb_down'),
    orbMove: (dx, dy) => send('orb_move', { dx, dy }),
    orbUp: moved => send('orb_up', { moved }),
  };
})();
