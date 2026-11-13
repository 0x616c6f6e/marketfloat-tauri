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
    chartPreview: async () => ({ error: '历史行情将在下一阶段接入。' }),
    selectPeriod: period => send('select_period', { period }),
    setPinned: pinned => invoke('set_pinned', { pinned }),
    saveProxy: proxy => invoke('save_proxy', { proxy }),
    collapse: () => send('show_orb'),
    hide: () => send('hide'),
    orbOpen: () => send('show_panel'),
    orbDown: () => send('orb_down'),
    orbMove: (dx, dy) => send('orb_move', { dx, dy }),
    orbUp: moved => send('orb_up', { moved }),
  };
})();
