/* ===========================================================
   EcoAlert 前端主逻辑（Tauri 版）
   - 登录 / 鉴权
   - 视图切换：实时监控 / 监控总览 / 视频管理 / 基础设置 / 检测配置 / 通知配置 / 日志
   - 视频播放（HLS / MP4 / 摄像头）
   - 事件订阅（替代 WebSocket）
   =========================================================== */
import Hls from 'hls.js';
import { convertFileSrc } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  login, logout, checkAuth,
  listSources, listGroups, createGroup, updateGroup, deleteGroup, reorder,
  createSource, updateSource, deleteSource, importTestSourcesFromFolder, setTestSourcesEnabled, TEST_SOURCE_IDS,
  reportSceneState, getStateHistory, listDetectionHistory,
  listAlarms, ackAlarm, resolveAlarm,
  getAlgorithmConfig, listAlgorithmConfigSources, updateAlgorithmConfig, deleteAlgorithmConfig,
  testVlmConfig, testVlmVision,
  testYoloConnection,
  getRoiConfig, listRoiConfigSources, updateRoiConfig, deleteRoiConfig, testRoiConfig,
  listNotificationTargets, createNotificationTarget, updateNotificationTarget, deleteNotificationTarget,
  listNotificationHistory, testNotificationTarget, resendNotification,
  changePassword, getDataDir, resetAllAppData,
  startOAuthBinding, checkOAuthStatus, verifyChannelCredentials,
  onEvent, onStatus, onRuntimeStatus, onAlgorithmSchedule, onSources, onSceneState, onAlarm, onNotification, isTauriEnv,
  openDevtools, probeUrl, checkFfmpegStatus,
} from './api.js';

const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => Array.from(el.querySelectorAll(sel));

/* -------------------- 工具 -------------------- */
const ICONS = {
  lock: '<rect width="18" height="11" x="3" y="11" rx="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  eye: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/>',
  monitor: '<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8M12 17v4"/>',
  chart: '<path d="M3 3v18h18"/><path d="m7 15 4-4 3 3 5-7"/>',
  camera: '<path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3Z"/><circle cx="12" cy="13" r="3"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-8 9-4.5-1.5-8-4-8-9V5l8-3 8 3v8Z"/><path d="M9 12l2 2 4-4"/>',
  target: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="5"/><circle cx="12" cy="12" r="1"/>',
  bell: '<path d="M10 21a2 2 0 0 0 4 0"/><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9"/>',
  terminal: '<path d="m4 17 6-6-6-6"/><path d="M12 19h8"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  'folder-plus': '<path d="M12 10v6M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.8a2 2 0 0 1-1.6-.8L9.4 3.6A2 2 0 0 0 7.8 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  refresh: '<path d="M21 12a9 9 0 0 1-15.5 6.2L3 16"/><path d="M3 21v-5h5"/><path d="M3 12A9 9 0 0 1 18.5 5.8L21 8"/><path d="M21 3v5h-5"/>',
  wrench: '<path d="M14.7 6.3a4 4 0 0 0-5.5 5.5L3 18v3h3l6.2-6.2a4 4 0 0 0 5.5-5.5l-2.6 2.6-3-3 2.6-2.6Z"/>',
  video: '<path d="m22 8-6 4 6 4V8Z"/><rect width="14" height="12" x="2" y="6" rx="2"/>',
  satellite: '<path d="M13 7 9 3 5 7l4 4 4-4Z"/><path d="m17 11 4 4-4 4-4-4 4-4Z"/><path d="m9 11 4 4"/><path d="M6 14a6 6 0 0 0 4 4"/><path d="M3 15a9 9 0 0 0 6 6"/>',
  edit: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5Z"/>',
  trash: '<path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="m19 6-1 14H6L5 6"/><path d="M10 11v5M14 11v5"/>',
  user: '<path d="M20 21a8 8 0 0 0-16 0"/><circle cx="12" cy="7" r="4"/>',
  bulb: '<path d="M9 18h6"/><path d="M10 22h4"/><path d="M8 14a6 6 0 1 1 8 0c-.7.6-1 1.4-1 2H9c0-.6-.3-1.4-1-2Z"/>',
  alarm: '<path d="M10 2h4"/><path d="M12 8v5"/><path d="M12 17h.01"/><path d="M4.9 19.1a10 10 0 1 1 14.2 0"/><path d="M3 5 1 7M21 5l2 2"/>',
  check: '<circle cx="12" cy="12" r="10"/><path d="m8 12 3 3 5-6"/>',
  alert: '<path d="m21.7 18-8-14a2 2 0 0 0-3.4 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.7-3Z"/><path d="M12 9v4M12 17h.01"/>',
  qr: '<rect width="6" height="6" x="3" y="3" rx="1"/><rect width="6" height="6" x="15" y="3" rx="1"/><rect width="6" height="6" x="3" y="15" rx="1"/><path d="M15 15h2v2h-2zM19 15h2M15 19h2M19 19h2v2h-2z"/>',
  chevron: '<path d="m6 9 6 6 6-6"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>',
};

const icon = (name, extraClass = '') =>
  `<svg class="icon-svg ${extraClass}" viewBox="0 0 24 24" aria-hidden="true" focusable="false">${ICONS[name] || ''}</svg>`;

const hydrateIcons = () => {
  $$('[data-icon]').forEach((el) => {
    el.innerHTML = icon(el.dataset.icon);
  });
};

const stateMarker = (on, label) =>
  `<span class="state-marker ${on ? 'on' : 'off'}">${icon(on ? 'check' : 'alert')}<span>${label}</span></span>`;

const escapeHtml = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));

const VIDEO_DIALOG_FILTERS = [{
  name: '视频文件',
  extensions: ['mp4', 'm4v', 'mov', 'mkv', 'avi', 'webm'],
}];

const TEST_VIDEO_GROUP_ID = 'grp-test-videos';
const isWindowsDrivePath = (value) => /^[a-zA-Z]:[\\/]/.test(value || '');
const isUncPath = (value) => /^\\\\/.test(value || '');
const hasUrlScheme = (value) => /^[a-zA-Z][a-zA-Z\d+.-]*:/.test(value || '');

const playableVideoUrl = (src) => {
  const url = src?.url || '';
  if (
    src?.type === 'mp4'
    && isTauriEnv
    && url
    && (!hasUrlScheme(url) || isWindowsDrivePath(url) || isUncPath(url))
  ) {
    return convertFileSrc(url);
  }
  return url;
};

const fileNameFromPath = (path) => {
  const name = String(path || '').split(/[\\/]/).pop() || '';
  return name.replace(/\.[^.]+$/, '');
};

const fmtTime = (ts) => {
  const d = ts ? new Date(ts) : new Date();
  const pad = (n) => String(n).padStart(2, '0');
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};
const fmtDate = (ts) => {
  const d = new Date(ts);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};
const fmtNow = () => {
  const d = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  const w = ['日', '一', '二', '三', '四', '五', '六'][d.getDay()];
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} 周${w} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};

/* -------------------- 鉴权 -------------------- */
const showLogin = () => {
  $('#login-page').classList.remove('hidden');
  $('#app-page').classList.add('hidden');
  setTimeout(() => $('#login-password').focus(), 50);
};
const showApp = () => {
  $('#login-page').classList.add('hidden');
  $('#app-page').classList.remove('hidden');
};

$('#toggle-pw').addEventListener('click', () => {
  const inp = $('#login-password');
  inp.type = inp.type === 'password' ? 'text' : 'password';
});

$('#login-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const pw = $('#login-password').value.trim();
  const errEl = $('#login-error');
  const btn = $('#login-submit');
  errEl.textContent = '';
  if (!pw) { errEl.textContent = '请输入密码'; return; }
  btn.disabled = true;
  btn.querySelector('.btn-text').textContent = '登录中…';
  try {
    await login(pw);
    await enterApp();
  } catch (err) {
    errEl.textContent = err.message || '登录失败';
  } finally {
    btn.disabled = false;
    btn.querySelector('.btn-text').textContent = '登 录';
  }
});

$('#logout-btn').addEventListener('click', async () => {
  try { await logout(); } catch (e) { console.warn('登出失败:', e); }
  showLogin();
});

const enterApp = async () => {
  showApp();
  try {
    startClock();
    await loadSources();
    await loadGroups();
    try {
      const globalAlgorithmConfig = await getAlgorithmConfig(null);
      setDeveloperMode(!!(globalAlgorithmConfig.developerMode ?? globalAlgorithmConfig.developer_mode));
    } catch (e) {
      console.warn('开发者模式配置读取失败:', e);
      setDeveloperMode(false);
    }
    renderLive();
    renderSourcesTable();
    switchView('live');
    await subscribeEvents();
    setupSettingsEnhancements();
    setupSettingsTabs();
    if (!isTauriEnv) addLog('warn', '当前在浏览器预览模式（未连接 Tauri 后端）');
  } catch (err) {
    console.warn('应用初始化过程中出现非致命错误:', err);
    addLog('warn', `初始化警告: ${err.message || err}`);
  }
};

/* -------------------- 设置页增强：chip / slider 同步 -------------------- */
function setupSettingsEnhancements() {
  // 1) 星期 chip 切换 → 同步到隐藏 input（main.js 仍读 #algo-weekdays）
  const chipBox = $('#algo-weekday-chips');
  const hidden = $('#algo-weekdays');
  if (chipBox && hidden && !chipBox.dataset.bound) {
    chipBox.dataset.bound = '1';
    const refresh = () => {
      const active = $$('.weekday-chip.active', chipBox).map((c) => c.dataset.wd);
      hidden.value = active.join(',');
      // 触发 input 事件，让外部能 watch
      hidden.dispatchEvent(new Event('input', { bubbles: true }));
    };
    chipBox.addEventListener('click', (e) => {
      const btn = e.target.closest('.weekday-chip');
      if (!btn) return;
      btn.classList.toggle('active');
      refresh();
    });
    // 全局辅助：fillAlgorithmForm 写完 hidden 后，外部可手动调 syncChipsFromHidden()
    syncChipsFromHidden = () => {
      const set = new Set(String(hidden.value || '').split(',').map((s) => s.trim()).filter(Boolean));
      $$('.weekday-chip', chipBox).forEach((c) => {
        c.classList.toggle('active', set.has(c.dataset.wd));
      });
    };
  }

  // 2) 阈值 slider 同步显示数字
  const syncSlider = (sliderId, labelId) => {
    const s = $(sliderId);
    const l = $(labelId);
    if (!s || !l || s.dataset.bound) return;
    s.dataset.bound = '1';
    const fmt = (v) => Number(v).toFixed(3);
    const update = () => (l.textContent = fmt(s.value));
    s.addEventListener('input', update);
    update();
  };
  syncSlider('#algo-person-threshold', '#algo-person-threshold-val');

  setupRoiPreviewDrag();
}

let syncChipsFromHidden = () => {};

function setRoiFields(x, y, w, h) {
  $('#roi-x').value = x.toFixed(2);
  $('#roi-y').value = y.toFixed(2);
  $('#roi-w').value = w.toFixed(2);
  $('#roi-h').value = h.toFixed(2);
  updateRoiPreview();
}

function setupRoiPreviewDrag() {
  const preview = $('#roi-preview');
  const box = $('#roi-preview-box');
  const handle = $('#roi-resize-handle');
  if (!preview || !box || !handle || preview.dataset.bound) return;
  preview.dataset.bound = '1';

  let drag = null;
  const readRect = () => ({
    x: clamp01($('#roi-x').value, 0.2),
    y: clamp01($('#roi-y').value, 0.2),
    w: Math.max(0.01, clamp01($('#roi-w').value, 0.5)),
    h: Math.max(0.01, clamp01($('#roi-h').value, 0.5)),
  });
  const pointToNorm = (event) => {
    const rect = preview.getBoundingClientRect();
    return {
      x: clamp01((event.clientX - rect.left) / rect.width, 0),
      y: clamp01((event.clientY - rect.top) / rect.height, 0),
    };
  };

  preview.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    const p = pointToNorm(event);
    const rect = readRect();
    const mode = event.target === handle ? 'resize' : (box.contains(event.target) ? 'move' : 'place');
    drag = {
      mode,
      start: p,
      rect,
      offsetX: p.x - rect.x,
      offsetY: p.y - rect.y,
    };
    preview.setPointerCapture?.(event.pointerId);
    event.preventDefault();
    if (mode === 'place') {
      const w = rect.w;
      const h = rect.h;
      setRoiFields(
        Math.min(1 - w, Math.max(0, p.x - w / 2)),
        Math.min(1 - h, Math.max(0, p.y - h / 2)),
        w,
        h
      );
    }
  });

  preview.addEventListener('pointermove', (event) => {
    if (!drag) return;
    const p = pointToNorm(event);
    if (drag.mode === 'resize') {
      const w = Math.max(0.01, Math.min(1 - drag.rect.x, p.x - drag.rect.x));
      const h = Math.max(0.01, Math.min(1 - drag.rect.y, p.y - drag.rect.y));
      setRoiFields(drag.rect.x, drag.rect.y, w, h);
      return;
    }
    const current = readRect();
    const x = Math.min(1 - current.w, Math.max(0, p.x - drag.offsetX));
    const y = Math.min(1 - current.h, Math.max(0, p.y - drag.offsetY));
    setRoiFields(x, y, current.w, current.h);
  });

  const endDrag = (event) => {
    if (!drag) return;
    drag = null;
    preview.releasePointerCapture?.(event.pointerId);
  };
  preview.addEventListener('pointerup', endDrag);
  preview.addEventListener('pointercancel', endDrag);
}

/* -------------------- 设置页 Tab 切换 -------------------- */
function setupSettingsTabs() {
  const tabBars = $$('.settings-tabs');
  if (tabBars.length === 0) return;

  const getTabTargets = (tab) => (tab.dataset.targets || tab.dataset.target || '')
    .split(',')
    .map((id) => id.trim())
    .filter(Boolean);

  tabBars.forEach((tabBar) => {
    const tabs = $$('.settings-tab', tabBar);
    if (tabs.length === 0) return;
    const panelIds = new Set();
    tabs.forEach((tab) => getTabTargets(tab).forEach((id) => panelIds.add(id)));
    const panels = [...panelIds]
      .map((id) => document.getElementById(id))
      .filter(Boolean);
    const show = (tab) => {
      const visibleIds = new Set(getTabTargets(tab));
      tabs.forEach((t) => t.classList.toggle('active', t === tab));
      panels.forEach((p) => p.classList.toggle('hidden', !visibleIds.has(p.id)));
      // 切 tab 时把页面滚到顶，避免停留在上一个 panel 的中间
      const main = document.querySelector('.main');
      if (main) main.scrollTo?.({ top: 0, behavior: 'smooth' });
      window.scrollTo?.({ top: 0, behavior: 'smooth' });
    };
    tabs.forEach((tab) => {
      tab.addEventListener('click', (e) => {
        e.preventDefault();
        show(tab);
      });
    });
    // 初始化：默认显示第一个 tab 对应 panel
    const initial = tabs.find((t) => t.classList.contains('active')) || tabs[0];
    show(initial);
  });
}

/* -------------------- 时钟 -------------------- */
let clockTimer = null;
const startClock = () => {
  if (clockTimer) return;
  const el = $('#topbar-time');
  const tick = () => (el.textContent = fmtNow());
  tick();
  clockTimer = setInterval(tick, 1000);
};

/* -------------------- 视图切换 -------------------- */
const VIEW_META = {
  live: { title: '实时监控', sub: '查看所有视频源的实时画面' },
  overview: { title: '监控总览', sub: '全局统计与各通道状态' },
  sources: { title: '视频管理', sub: '新增 / 编辑 / 删除视频流' },
  'basic-settings': { title: '基础设置', sub: '账号安全、数据目录与调试开关' },
  'detection-settings': { title: '检测配置', sub: '算法调度、阈值和 ROI 标定' },
  'notification-settings': { title: '通知管理', sub: '通知渠道配置与发送历史' },
  'detection-history': { title: '检测历史', sub: '按视频源查看灯光、人员、报警和算法参数曲线' },
  console: { title: '系统日志', sub: '服务端与客户端事件流' },
  about: { title: '关于', sub: '项目信息与作者' },
};
const switchView = (name) => {
  if (!VIEW_META[name]) return;
  if (name === 'detection-history' && !developerMode) return;
  $$('.nav-item').forEach((b) => b.classList.toggle('active', b.dataset.view === name));
  $$('.view').forEach((v) => v.classList.toggle('hidden', v.id !== `view-${name}`));
  $('#view-title').textContent = VIEW_META[name].title;
  $('#view-sub').textContent = VIEW_META[name].sub;
  if (name === 'overview') renderOverview();
  if (name === 'basic-settings') renderBasicSettings();
  if (name === 'detection-settings') renderDetectionSettings();
  if (name === 'notification-settings') renderNotificationSettings();
  if (name === 'detection-history') renderDetectionHistory();
  if (name !== 'detection-settings') destroyRoiVideo();
};
$$('.nav-item').forEach((b) => b.addEventListener('click', () => switchView(b.dataset.view)));

/* -------------------- 数据 -------------------- */
let sources = [];
let groups = [];
let stats = [];
let alarms = [];
let algorithmConfig = null;
let roiConfig = null;
let notificationTargets = [];
let notificationHistory = [];
let developerMode = false;
/** 实时状态：sourceId -> { person, light, alarm, confidence, brightness, motion, ts } */
let sceneStates = new Map();
let runtimeStatuses = new Map();
let vlmStates = new Map();


const setDeveloperMode = (enabled) => {
  developerMode = !!enabled;
  document.body?.classList.toggle('developer-mode', developerMode);
  $$('.developer-only').forEach((el) => el.classList.toggle('hidden', !developerMode));
  if (!developerMode && !$('#view-detection-history')?.classList.contains('hidden')) {
    switchView('live');
  }
};

const loadSources = async () => {
  try { sources = await listSources(); }
  catch (e) { console.warn('加载视频源失败:', e); sources = []; }
};
const loadGroups = async () => {
  try { groups = await listGroups(); }
  catch (e) { console.warn('加载分组失败:', e); groups = []; }
};

/* -------------------- 实时监控 -------------------- */
const renderLive = () => {
  const grid = $('#video-grid');
  const empty = $('#live-empty');
  const enabled = sources.filter((s) => s.enabled);
  const enabledIds = new Set(enabled.map((s) => s.id));
  $('#live-count').textContent = `${enabled.length} 路`;
  if (enabled.length === 0) {
    grid.innerHTML = '';
    empty.classList.remove('hidden');
    return;
  }
  empty.classList.add('hidden');

  // 按分组聚合
  const groupsSorted = [...groups].sort((a, b) => a.order - b.order);
  // 给每个组分配卡片
  const grouped = new Map();
  for (const g of groupsSorted) grouped.set(g.id, []);
  const noGroup = [];
  for (const s of enabled) {
    const gid = s.groupId || 'grp-default';
    if (grouped.has(gid)) grouped.get(gid).push(s);
    else noGroup.push(s);
  }
  // 每组内按 order 排序
  for (const arr of grouped.values()) arr.sort((a, b) => a.order - b.order);
  noGroup.sort((a, b) => a.order - b.order);

  const sectionHtml = (g, list) => `
    <section class="group-section" data-group-id="${g.id}" data-group-order="${g.order ?? 0}">
      <header class="group-header">
        <span class="group-drag-handle" title="拖动排序">${icon('chevron')}</span>
        <span class="caret ${g.collapsed ? 'collapsed' : ''}" data-toggle="${g.id}">${icon('chevron')}</span>
        <div class="group-name">
          <span class="grp-label">${escapeHtml(g.name)}</span>
          <input class="grp-input hidden" data-grp-input="${g.id}" value="${escapeHtml(g.name)}" />
        </div>
        <span class="group-count">${list.length} 路</span>
        <label class="domain-toggle" title="开启后，该分组内所有启用视频源均报警时才发送报警通知">
          <input type="checkbox" data-domain-detect="${g.id}" ${g.domainDetectionEnabled ? 'checked' : ''} />
          <span class="domain-toggle-track" aria-hidden="true"></span>
          <span class="domain-toggle-text">域检测</span>
        </label>
        <div class="group-actions">
          ${g.id === 'grp-default' ? '' : `<button data-rename="${g.id}" title="重命名">${icon('edit')}</button>`}
          ${g.id === 'grp-default' ? '' : `<button data-delgrp="${g.id}" title="删除分组">${icon('trash')}</button>`}
        </div>
      </header>
      <div class="group-body ${g.collapsed ? 'collapsed' : ''}" data-dropzone="${g.id}">
        ${list.length === 0 ? '<div class="drop-hint">拖拽视频源到此处</div>' : ''}
        ${list.map((s) => videoCardHtml(s)).join('')}
      </div>
    </section>
  `;

  let html = groupsSorted.map((g) => sectionHtml(g, grouped.get(g.id) || [])).join('');
  if (noGroup.length > 0) {
    // 临时兜底分组（理论上不会到这里）
    html += sectionHtml(
      { id: '__nogroup', name: '其他', collapsed: false, order: 9999, domainDetectionEnabled: false },
      noGroup
    );
  }
  grid.innerHTML = html;

  // 挂载视频 + 事件
  enabled.forEach((s) => mountVideo(s));
  $$('.btn-edit', grid).forEach((b) => b.addEventListener('click', () => openModal(b.dataset.id)));
  $$('.btn-del', grid).forEach((b) => b.addEventListener('click', () => removeSource(b.dataset.id)));
  $$('.caret[data-toggle]', grid).forEach((el) => {
    el.addEventListener('click', () => toggleGroup(el.dataset.toggle));
  });
  $$('[data-delgrp]', grid).forEach((b) => {
    b.addEventListener('click', () => removeGroup(b.dataset.delgrp));
  });
  $$('[data-rename]', grid).forEach((b) => {
    b.addEventListener('click', () => startRenameGroup(b.dataset.rename));
  });
  $$('[data-domain-detect]', grid).forEach((input) => {
    input.addEventListener('change', () => toggleDomainDetection(input.dataset.domainDetect, input.checked));
  });
  $$('[data-grp-input]', grid).forEach((inp) => {
    inp.addEventListener('blur', () => finishRenameGroup(inp.dataset.grpInput, inp.value));
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') inp.blur();
      if (e.key === 'Escape') { inp.value = inp.defaultValue; inp.classList.add('hidden'); inp.previousElementSibling.classList.remove('hidden'); }
    });
  });

  bindDragAndDrop();
  applyStateIcons();
  updateAlarmBanner();
};

/* -------------------- 拖拽：视频卡 / 分组 -------------------- */
/*
 * 不使用 HTML5 Drag API — <video> 元素会让浏览器"拖拽源确定"算法中断，
 * 使父级 card 的 draggable 失效。改用 document 级 mouse 事件实现自定义拖拽。
 */
let dragSourceId = null;
function bindDragAndDrop() {
  bindGroupDragSort();
  $$('.video-card').forEach((card) => {
    card.addEventListener('mousedown', (e) => {
      // 仅左键
      if (e.button !== 0) return;
      // 排除交互控件（但不排除 video，video 区域也应可拖拽）
      if (e.target.closest('button, input, select, textarea, a[href]')) return;
      // 阻止 <video> 的默认拖拽行为（不阻止 click）
      e.preventDefault();

      const startX = e.clientX;
      const startY = e.clientY;
      let dragging = false;
      let clone = null;

      const onMove = (ev) => {
        const dx = ev.clientX - startX;
        const dy = ev.clientY - startY;
        if (!dragging && (dx * dx + dy * dy) > 25) {
          dragging = true;
          dragSourceId = card.dataset.id;
          card.classList.add('dragging');
          const srcSection = card.closest('.group-section');
          if (srcSection) srcSection.classList.add('drag-source');
          clone = card.cloneNode(true);
          clone.style.cssText = `position:fixed;z-index:9999;pointer-events:none;opacity:.75;width:${card.offsetWidth}px;`;
          clone.style.left = `${ev.clientX - card.offsetWidth / 2}px`;
          clone.style.top = `${ev.clientY - 20}px`;
          document.body.appendChild(clone);
        }
        if (dragging) {
          if (clone) {
            clone.style.left = `${ev.clientX - card.offsetWidth / 2}px`;
            clone.style.top = `${ev.clientY - 20}px`;
          }
          $$('[data-dropzone]').forEach((z) => {
            const r = z.getBoundingClientRect();
            const over = ev.clientX >= r.left && ev.clientX <= r.right && ev.clientY >= r.top && ev.clientY <= r.bottom;
            z.classList.toggle('drag-active', over);
            const sec = z.closest('.group-section');
            if (sec) sec.classList.toggle('drag-over', over);
          });
        }
      };

      const cleanup = () => {
        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
        if (dragging) {
          card._dragJustEnded = true;
          setTimeout(() => { card._dragJustEnded = false; }, 0);
        }
        if (clone) { clone.remove(); clone = null; }
        card.classList.remove('dragging');
        $$('.group-section').forEach((g) => {
          g.classList.remove('drag-over', 'drag-source');
          const body = g.querySelector('.group-body');
          if (body) body.classList.remove('drag-active');
        });
      };

      const onUp = async (ev) => {
        if (dragging) {
          const target = document.elementsFromPoint(ev.clientX, ev.clientY)
            .find((el) => el.dataset?.dropzone);
          if (target && dragSourceId) {
            await moveSourceToGroup(dragSourceId, target.dataset.dropzone);
          }
        }
        dragSourceId = null;
        cleanup();
      };

      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp, { once: true });
    });
  });

  // 拖拽结束后阻止冒泡 click（防止误触按钮/编辑等操作）
  if (!document._dragClickGuard) {
    document._dragClickGuard = true;
    document.addEventListener('click', (e) => {
      const card = e.target.closest('.video-card');
      if (card?._dragJustEnded) { e.preventDefault(); e.stopPropagation(); }
    }, true);
  }
}

function bindGroupDragSort() {
  $$('.group-header').forEach((header) => {
    header.addEventListener('mousedown', (e) => {
      if (e.button !== 0) return;
      if (e.target.closest('button, input, select, textarea, a[href], .caret, .domain-toggle')) return;
      const section = header.closest('.group-section');
      const grid = $('#video-grid');
      if (!section || !grid) return;
      e.preventDefault();

      const startX = e.clientX;
      const startY = e.clientY;
      let dragging = false;
      let placeholder = null;
      let clone = null;

      const sortableSections = () => $$('.group-section', grid).filter((item) => item !== section);

      const onMove = (ev) => {
        const dx = ev.clientX - startX;
        const dy = ev.clientY - startY;
        if (!dragging && (dx * dx + dy * dy) > 25) {
          dragging = true;
          section.classList.add('group-dragging');
          placeholder = document.createElement('section');
          placeholder.className = 'group-section group-placeholder';
          placeholder.style.height = `${section.offsetHeight}px`;
          section.after(placeholder);
          clone = section.cloneNode(true);
          clone.classList.add('group-drag-clone');
          clone.style.cssText = `position:fixed;z-index:9998;pointer-events:none;opacity:.82;width:${section.offsetWidth}px;left:${section.getBoundingClientRect().left}px;top:${ev.clientY - 24}px;`;
          document.body.appendChild(clone);
        }
        if (!dragging) return;
        if (clone) clone.style.top = `${ev.clientY - 24}px`;
        const target = sortableSections().find((item) => {
          if (item === placeholder) return false;
          const rect = item.getBoundingClientRect();
          return ev.clientY >= rect.top && ev.clientY <= rect.bottom;
        });
        if (target && placeholder) {
          const rect = target.getBoundingClientRect();
          if (ev.clientY < rect.top + rect.height / 2) {
            grid.insertBefore(placeholder, target);
          } else {
            grid.insertBefore(placeholder, target.nextSibling);
          }
        }
      };

      const cleanup = () => {
        document.removeEventListener('mousemove', onMove);
        document.removeEventListener('mouseup', onUp);
        if (clone) clone.remove();
        section.classList.remove('group-dragging');
      };

      const onUp = async () => {
        if (dragging && placeholder) {
          grid.insertBefore(section, placeholder);
          placeholder.remove();
          placeholder = null;
          await persistGroupOrderFromDom();
        }
        cleanup();
      };

      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp, { once: true });
    });
  });
}

async function persistGroupOrderFromDom() {
  const orderedIds = $$('.group-section', $('#video-grid'))
    .map((section) => section.dataset.groupId)
    .filter(Boolean);
  if (orderedIds.length === 0) return;
  const previous = new Map(groups.map((g) => [g.id, g.order]));
  try {
    const updates = orderedIds
      .map((id, index) => {
        const group = groups.find((g) => g.id === id);
        if (!group || group.order === index) return null;
        return { group, order: index };
      })
      .filter(Boolean);
    for (const item of updates) {
      const saved = await updateGroup(item.group.id, {
        name: item.group.name,
        order: item.order,
        collapsed: item.group.collapsed,
        domainDetectionEnabled: item.group.domainDetectionEnabled,
      });
      item.group.order = saved.order;
    }
    addLog('info', '分组排序已保存');
  } catch (err) {
    for (const group of groups) {
      if (previous.has(group.id)) group.order = previous.get(group.id);
    }
    addLog('error', `分组排序保存失败: ${err.message || err}`);
    renderLive();
  }
}

async function moveSourceToGroup(sourceId, groupId) {
  const s = sources.find((x) => x.id === sourceId);
  if (!s || s.groupId === groupId) return;
  // 重新计算 order：插到该组末尾
  const siblings = sources.filter((x) => x.groupId === groupId && x.id !== sourceId);
  const newOrder = siblings.length > 0 ? Math.max(...siblings.map((x) => x.order)) + 1 : 0;
  const oldGroupId = s.groupId;
  try {
    await updateSource(sourceId, { ...s, groupId, order: newOrder });
    // 更新内存数据（不再全量重载 + 重建 DOM）
    s.groupId = groupId;
    s.order = newOrder;

    // —— 轻量 DOM 更新：只移动被拖的卡片，其他视频完全不动 ——
    const card = document.querySelector(`.video-card[data-id="${sourceId}"]`);
    const targetZone = document.querySelector(`[data-dropzone="${groupId}"]`);
    if (card && targetZone) {
      // 目标组若空，移除占位提示
      targetZone.querySelectorAll('.drop-hint').forEach((h) => h.remove());
      targetZone.appendChild(card);
      card.dataset.groupId = groupId;
    }
    // 源组变空则补回提示
    const oldZone = document.querySelector(`[data-dropzone="${oldGroupId}"]`);
    if (oldZone && !oldZone.querySelectorAll('.video-card').length) {
      const hint = document.createElement('div');
      hint.className = 'drop-hint';
      hint.textContent = '拖拽视频源到此处';
      oldZone.appendChild(hint);
    }
    // 更新分组计数
    refreshGroupCounts();

    // 管理页表格是独立视图，全量刷也不影响视频
    renderSourcesTable();
    addLog('info', `已把「${s.name}」移到分组`);
  } catch (err) {
    addLog('error', `移动失败: ${err.message}`);
  }
}

/* 只刷新分组头部「N 路」数字，不重建任何 DOM */
function refreshGroupCounts() {
  $$('.group-section').forEach((section) => {
    const gid = section.dataset.groupId;
    const count = section.querySelectorAll('.video-card').length;
    const label = section.querySelector('.group-count');
    if (label) label.textContent = `${count} 路`;
  });
}

/* -------------------- 分组操作 -------------------- */
async function toggleGroup(groupId) {
  const g = groups.find((x) => x.id === groupId);
  if (!g) return;
  try {
    const updated = await updateGroup(groupId, {
      name: g.name,
      order: g.order,
      collapsed: !g.collapsed,
      domainDetectionEnabled: g.domainDetectionEnabled,
    });
    g.collapsed = updated.collapsed;
    renderLive();
  } catch (err) {
    addLog('error', `折叠状态切换失败: ${err.message}`);
  }
}

async function toggleDomainDetection(groupId, enabled) {
  const g = groups.find((x) => x.id === groupId);
  if (!g) return;
  try {
    const updated = await updateGroup(groupId, {
      name: g.name,
      order: g.order,
      collapsed: g.collapsed,
      domainDetectionEnabled: enabled,
    });
    g.domainDetectionEnabled = updated.domainDetectionEnabled;
    addLog('info', `分组「${g.name}」域检测已${g.domainDetectionEnabled ? '开启' : '关闭'}`);
    renderLive();
  } catch (err) {
    addLog('error', `域检测切换失败: ${err.message}`);
    renderLive();
  }
}

async function removeGroup(groupId) {
  const g = groups.find((x) => x.id === groupId);
  if (!g) return;
  if (!confirm(`确定删除分组「${g.name}」？该组下的视频源会移到默认分组。`)) return;
  try {
    await deleteGroup(groupId);
    await loadSources();
    await loadGroups();
    renderLive();
    renderSourcesTable();
    addLog('warn', `已删除分组: ${g.name}`);
  } catch (err) {
    addLog('error', `删除失败: ${err.message}`);
  }
}
function startRenameGroup(groupId) {
  const g = groups.find((x) => x.id === groupId);
  if (!g) return;
  const section = document.querySelector(`[data-group-id="${groupId}"]`);
  if (!section) return;
  const label = section.querySelector('.grp-label');
  const input = section.querySelector('.grp-input');
  if (!label || !input) return;
  label.classList.add('hidden');
  input.classList.remove('hidden');
  input.focus(); input.select();
}
async function finishRenameGroup(groupId, name) {
  const g = groups.find((x) => x.id === groupId);
  if (!g) return;
  const newName = (name || '').trim();
  if (!newName || newName === g.name) {
    // 还原
    renderLive();
    return;
  }
  try {
    const updated = await updateGroup(groupId, {
      name: newName,
      order: g.order,
      collapsed: g.collapsed,
      domainDetectionEnabled: g.domainDetectionEnabled,
    });
    g.name = updated.name;
    addLog('info', `分组已重命名: ${updated.name}`);
    renderLive();
  } catch (err) {
    addLog('error', `重命名失败: ${err.message}`);
    renderLive();
  }
}

async function addNewGroup() {
  const name = prompt('输入新分组名：', '新分组');
  if (!name) return;
  try {
    const order = groups.length > 0 ? Math.max(...groups.map((g) => g.order)) + 1 : 0;
    const grp = await createGroup({ name, order, collapsed: false, domainDetectionEnabled: false });
    groups.push(grp);
    renderLive();
    addLog('info', `新增分组: ${grp.name}`);
  } catch (err) {
    addLog('error', `新增分组失败: ${err.message}`);
  }
}

/* -------------------- 状态图标（人/灯/报警）实时更新 -------------------- */
const isVlmEnabledForSource = (sourceId) => {
  const rt = runtimeStatuses.get(sourceId);
  return !!(rt?.vlmEnabled ?? rt?.vlm_enabled);
};

const vlmStateFromScene = (scene) => {
  const vlmStatus = scene?.vlmStatus || scene?.vlm_status;
  if (vlmStatus && vlmStatus !== 'none') {
    if (vlmStatus === 'error') {
      return {
        status: 'error',
        label: '失败',
        confidence: null,
        latencyMs: scene.modelLatencyMs ?? scene.processMs ?? null,
        ts: scene.ts || Date.now(),
        reason: scene.reason || 'vlm_error',
      };
    }
    const hasPerson = vlmStatus === 'person' || scene.vlmPerson === true;
    return {
      status: hasPerson ? 'person' : 'no_person',
      label: hasPerson ? '有人' : '无人',
      confidence: scene.vlmPersonConfidence ?? null,
      latencyMs: scene.modelLatencyMs ?? scene.processMs ?? null,
      ts: scene.ts || Date.now(),
      reason: scene.reason || vlmStatus,
    };
  }
  const reason = String(scene?.reason || '');
  if (reason === 'vlm_person_detected') {
    return {
      status: 'person',
      label: '有人',
      confidence: Number(scene.personConfidence ?? scene.confidence ?? 0),
      latencyMs: scene.modelLatencyMs ?? scene.processMs ?? null,
      ts: scene.ts || Date.now(),
      reason,
    };
  }
  if (reason.startsWith('vlm_no_person')) {
    return {
      status: 'no_person',
      label: '无人',
      confidence: 1 - Number(scene.personConfidence ?? 0),
      latencyMs: scene.modelLatencyMs ?? scene.processMs ?? null,
      ts: scene.ts || Date.now(),
      reason,
    };
  }
  return null;
};

const renderVlmIcon = (el, sourceId) => {
  const vlm = el.querySelector('.vlm');
  if (!vlm) return;
  const enabled = isVlmEnabledForSource(sourceId);
  vlm.classList.toggle('hidden', !enabled);
  if (!enabled) return;
  const state = vlmStates.get(sourceId);
  vlm.classList.remove('is-active', 'is-negative', 'is-error', 'is-pending');
  if (!state) {
    vlm.classList.add('is-negative');
    vlm.title = 'VLM：无人/0（暂无 VLM 判断结果）';
    return;
  }
  if (state.status === 'person') {
    vlm.classList.add('is-active');
  } else if (state.status === 'no_person') {
    vlm.classList.add('is-negative');
  } else if (state.status === 'error') {
    vlm.classList.add('is-error');
  } else {
    vlm.classList.add('is-pending');
  }
  const conf = state.confidence == null ? '' : ` · 置信度 ${(Number(state.confidence) * 100).toFixed(0)}%`;
  const latency = state.latencyMs == null ? '' : ` · ${Number(state.latencyMs).toFixed(0)}ms`;
  vlm.title = `VLM：${state.label || '等待'}${conf}${latency} · ${fmtTime(state.ts)} · ${state.reason || '-'}`;
};

function applyStateIcons() {
  $$('.state-icons').forEach((el) => {
    const id = el.dataset.state;
    const s = sceneStates.get(id) || { person: false, light: false };
    const person = el.querySelector('.person');
    const light = el.querySelector('.light');
    const alarm = el.querySelector('.alarm');
    if (person) {
      const simplePerson = s.simplePerson ?? s.person;
      const simpleConfidence = s.simplePersonConfidence ?? s.personConfidence ?? 0;
      person.classList.toggle('is-active', !!simplePerson);
      person.title = simplePerson
        ? `常规模型：有人 (置信度 ${(simpleConfidence * 100).toFixed(0)}%)`
        : `常规模型：无人 (置信度 ${(simpleConfidence * 100).toFixed(0)}%)`;
    }
    if (light) {
      light.classList.toggle('is-active', !!s.light);
      light.title = s.light
        ? `灯：亮 (置信度 ${(s.lightConfidence * 100).toFixed(0)}%)`
        : '灯：关';
    }
    if (alarm) {
      const isAlarm = !!s.alarm;
      const vlmProgress = Math.max(0, Math.min(1, Number(s.vlmProgress ?? s.alarmProgress ?? 0)));
      const countdownProgress = Math.max(0, Math.min(1, Number(s.alarmCountdownProgress ?? 0)));
      alarm.classList.toggle('is-active', isAlarm);
      alarm.classList.toggle('has-progress', (vlmProgress > 0 || countdownProgress > 0) && !isAlarm);
      alarm.style.setProperty('--vlm-progress', vlmProgress.toFixed(3));
      alarm.style.setProperty('--alarm-countdown-progress', countdownProgress.toFixed(3));
      alarm.title = isAlarm
        ? '报警：无人 + 亮灯'
        : (countdownProgress > 0
          ? `报警倒计时：${Math.round(countdownProgress * 100)}%`
          : (vlmProgress > 0 ? `VLM无人确认进度：${Math.round(vlmProgress * 100)}%` : (s.alarmStatus === 'suspected' ? '疑似：等待保持时间' : '正常')));
    }
    renderVlmIcon(el, id);
  });
  $$('.scene-readout').forEach((el) => {
    if (!developerMode) {
      el.classList.add('hidden');
      return;
    }
    el.classList.remove('hidden');
    const id = el.dataset.scene;
    const s = sceneStates.get(id);
    if (!s) {
      const rt = runtimeStatuses.get(id);
      if (rt) {
        const status = rt.algorithmStatus || rt.algorithm_status || 'idle';
        const err = rt.lastError || rt.last_error;
        const last = rt.lastAlgorithmAt || rt.last_algorithm_at;
        if (status === 'disabled') {
          el.textContent = '未运行';
          el.title = '算法被关闭、通道停用或当前不在启用时段 · ' + (err || '');
        } else if (status === 'error') {
          el.textContent = '抽帧失败';
          el.title = err || '请检查 ffmpeg、HLS URL 和推流器';
        } else if (last) {
          el.textContent = '等待中';
          el.title = `上次算法时间 ${fmtTime(last)}`;
        } else {
          el.textContent = status === 'running' ? '抽帧中' : '等待首次';
          el.title = err || 'Tauri 后端完成首次抽帧检测后显示';
        }
      } else {
        el.textContent = '待连接';
        el.title = '等待 runtime_status 或 scene_state 事件';
      }
      return;
    }
    const colorText = s.colorScore == null ? '-' : `${Number(s.colorScore).toFixed(3)}`;
    const motionText = s.motionScore == null ? '-' : `${Number(s.motionScore).toFixed(3)}`;
    const cost = s.processMs ?? s.modelLatencyMs;
    const costText = cost == null ? '-' : `${Number(cost).toFixed(1)}ms`;
    el.textContent = `色彩:${colorText}  运动:${motionText}  检测:${fmtTime(s.ts)}  耗时:${costText}`;
    el.dataset.brightness = s.lightBrightness ?? '';
    el.title = `融合:${s.person ? '有人' : '无人'} · 色彩:${colorText} · 运动:${motionText} · 来源:${s.source || 'simple'} · ${s.reason || '-'} · #${s.frameSeq || 0} · ${costText} · ${fmtTime(s.ts)}`;
  });
}

function updateLiveState(payload) {
  if (!payload || !payload.sourceId) return;
  const next = {
    person: !!payload.person,
    light: !!payload.light,
    lightState: payload.lightState || payload.light_state || (payload.light ? 'on' : 'off'),
    simplePerson: payload.simplePerson ?? payload.simple_person ?? payload.person,
    simplePersonConfidence: payload.simplePersonConfidence ?? payload.simple_person_confidence ?? payload.personConfidence ?? payload.person_confidence ?? 0,
    vlmPerson: payload.vlmPerson ?? payload.vlm_person ?? null,
    vlmPersonConfidence: payload.vlmPersonConfidence ?? payload.vlm_person_confidence ?? null,
    vlmStatus: payload.vlmStatus ?? payload.vlm_status ?? 'none',
    alarm: !!payload.alarm,
    alarmStatus: payload.alarmStatus || payload.alarm_status || 'normal',
    alarmProgress: payload.alarmProgress ?? payload.alarm_progress ?? 0,
    vlmProgress: payload.vlmProgress ?? payload.vlm_progress ?? 0,
    alarmCountdownProgress: payload.alarmCountdownProgress ?? payload.alarm_countdown_progress ?? 0,
    ts: payload.ts || Date.now(),
    personConfidence: payload.personConfidence ?? 0,
    lightConfidence: payload.lightConfidence ?? 0,
    source: payload.source ?? 'simple',
    modelLatencyMs: payload.modelLatencyMs ?? null,
    frameSeq: payload.frameSeq ?? 0,
    confidence: payload.confidence ?? 0,
    reason: payload.reason ?? null,
    lightBrightness: payload.lightBrightness ?? null,
    colorScore: payload.colorScore ?? null,
    motionScore: payload.motionScore ?? null,
    processMs: payload.processMs ?? null,
    yoloDetections: payload.yolo_detections ?? payload.yoloDetections ?? [],
  };
  sceneStates.set(payload.sourceId, next);
  const vlmState = vlmStateFromScene(next);
  if (vlmState) vlmStates.set(payload.sourceId, vlmState);
  applyStateIcons();
  updateAlarmBanner();
  renderDetectionBoxes(payload.sourceId, next.yoloDetections);
}

function renderDetectionBoxes(sourceId, detections) {
  const wrap = document.getElementById(`vw-${sourceId}`);
  if (!wrap) return;
  // 移除旧 overlay
  wrap.querySelectorAll('.yolo-overlay').forEach((el) => el.remove());
  if (!detections?.length) return;
  // YOLO 检测使用 1280x720 (16:9)，SVG viewBox 用同样比例
  // preserveAspectRatio='xMidYMid meet' 与 video 的 object-fit: contain 行为一致
  // 这样无论容器怎么缩放，检测框都能和视频实际显示区域对齐
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.classList.add('yolo-overlay');
  svg.setAttribute('viewBox', '0 0 1280 720');
  svg.setAttribute('preserveAspectRatio', 'xMidYMid meet');
  svg.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;pointer-events:none;z-index:5;';
  for (const det of detections) {
    const [cx, cy, w, h] = det.bbox;
    const x = (cx - w / 2) * 1280;
    const y = (cy - h / 2) * 720;
    const bw = w * 1280;
    const bh = h * 720;
    const r = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
    r.setAttribute('x', x);
    r.setAttribute('y', y);
    r.setAttribute('width', Math.max(bw, 2));
    r.setAttribute('height', Math.max(bh, 2));
    r.setAttribute('fill', 'none');
    const hue = Math.round(det.confidence * 120); // 0=红, 120=绿
    r.setAttribute('stroke', `hsl(${hue}, 90%, 55%)`);
    r.setAttribute('vector-effect', 'non-scaling-stroke');
    r.setAttribute('stroke-width', '2');
    r.setAttribute('rx', '3');
    svg.appendChild(r);
    // 置信度标签
    const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
    text.setAttribute('x', x);
    text.setAttribute('y', Math.max(y - 6, 16));
    text.setAttribute('fill', `hsl(${hue}, 90%, 55%)`);
    text.setAttribute('font-size', '14');
    text.setAttribute('font-family', 'system-ui, sans-serif');
    text.setAttribute('vector-effect', 'non-scaling-stroke');
    text.textContent = `${(det.confidence * 100).toFixed(0)}%`;
    svg.appendChild(text);
  }
  wrap.appendChild(svg);
}


function updateVlmStateFromSchedule(payload) {
  if (!payload?.sourceId) return;
  if (payload.action === 'vlm_error') {
    vlmStates.set(payload.sourceId, {
      status: 'error',
      label: '失败',
      confidence: null,
      latencyMs: payload.latencyMs ?? null,
      ts: payload.ts || Date.now(),
      reason: payload.reason || 'vlm_error',
    });
    const model = algorithmConfig?.vlmModel ?? algorithmConfig?.vlm_model ?? '';
    const modelInfo = model ? ` [${model}]` : '';
    addLog('warn', `VLM 检测失败 (${payload.sourceId})${modelInfo}: ${payload.reason || '未知错误'}`);
    applyStateIcons();
    return;
  }
  if (payload.action !== 'run_vlm') return;
  const hasPerson = payload.reason === 'vlm_person_detected';
  vlmStates.set(payload.sourceId, {
    status: hasPerson ? 'person' : 'no_person',
    label: hasPerson ? '有人' : '无人',
    confidence: null,
    latencyMs: payload.latencyMs ?? null,
    ts: payload.ts || Date.now(),
    reason: payload.reason || 'run_vlm',
  });
  applyStateIcons();
}

function updateRuntimeStatuses(payload) {
  runtimeStatuses = new Map((payload || []).map((item) => [item.sourceId || item.source_id, item]));
  applyStateIcons();
}

function updateAlarmBanner() {
  const banner = $('#alarm-banner');
  if (!banner) return;
  // 统计当前在报警的源
  const alarming = sources.filter((s) => {
    const st = sceneStates.get(s.id);
    return st && st.alarm;
  });
  // 永远占位、内容切换不改变高度（高度由 CSS 固定）
  if (alarming.length === 0) {
    banner.classList.add('ok');
    banner.classList.remove('alarm');
    banner.innerHTML = `
      <span class="icon">${icon('check')}</span>
      <div>所有通道状态正常</div>
    `;
  } else {
    banner.classList.add('alarm');
    banner.classList.remove('ok');
    // 只展示前 3 个通道名 + "等 N 路"
    const shown = alarming.slice(0, 3).map((s) => `<b>${escapeHtml(s.name)}</b>`).join('、');
    const more = alarming.length > 3 ? ` 等 <b>${alarming.length}</b> 路` : '';
    banner.innerHTML = `
      <span class="icon">${icon('alarm')}</span>
      <div>当前 <b>${alarming.length}</b> 路报警：${shown}${more}</div>
    `;
  }
}

const videoCardHtml = (s) => {
  const st = stats.find((x) => x.id === s.id) || {};
  const scene = sceneStates.get(s.id) || { person: false, light: false };
  const simplePerson = scene.simplePerson ?? scene.person;
  const alarm = !!scene.alarm;
  return `
    <div class="video-card" data-id="${s.id}" data-group-id="${s.groupId || 'grp-default'}">
      <div class="video-wrap" id="vw-${s.id}">
        <div class="placeholder"><div class="illu">${icon('satellite')}</div><div>正在加载视频…</div></div>
        <div class="live-tag ${st.online ? '' : 'off'}">
          <span class="pulse"></span>${st.online ? 'LIVE' : '离线'}
        </div>
      </div>
      <div class="card-info">
        <div class="card-row card-row-top">
          <span class="card-name" title="${escapeHtml(s.name)}">${escapeHtml(s.name)}</span>
          <span class="state-icons" data-state="${s.id}">
            <span class="state-icon vlm hidden" title="VLM：等待判断">${icon('user')}</span>
            <span class="state-icon person ${simplePerson ? 'is-active' : ''}" title="常规模型 ${simplePerson ? '有人' : '无人'}">${icon('user')}</span>
            <span class="state-icon light ${scene.light ? 'is-active' : ''}" title="灯 ${scene.light ? '亮' : '关'}">${icon('bulb')}</span>
            <span class="state-icon alarm ${alarm ? 'is-active' : ''}" title="${alarm ? '报警：无人但亮灯' : '正常'}">${icon('alarm')}</span>
          </span>
        </div>
        <div class="card-row card-row-bottom">
          <span class="card-loc" title="${escapeHtml(s.location || '')}">${escapeHtml(s.location || '—')}</span>
          <span class="card-actions">
            <button class="ico-btn btn-edit" data-id="${s.id}" title="编辑">${icon('edit')}</button>
            <button class="ico-btn btn-del" data-id="${s.id}" title="删除">${icon('trash')}</button>
          </span>
        </div>
        <div class="scene-readout ${developerMode ? '' : 'hidden'}" data-scene="${s.id}" title="等待检测结果">检测：等待结果</div>
      </div>
    </div>
  `;
};

const mountVideo = (src) => {
  const wrap = document.getElementById(`vw-${src.id}`);
  if (!wrap || wrap.dataset.mounted === '1') return;
  wrap.dataset.mounted = '1';
  const video = document.createElement('video');
  video.controls = true;
  video.muted = true;
  video.defaultMuted = true;
  video.playsInline = true;
  video.autoplay = true;
  video.preload = 'auto';
  const mountedAt = Date.now();
  let userInteracted = false;
  let playRetryTimer = null;
  const requestAutoPlay = (delay = 0) => {
    if (userInteracted || document.hidden) return;
    clearTimeout(playRetryTimer);
    playRetryTimer = setTimeout(() => {
      if (userInteracted || document.hidden || video.ended || !video.isConnected) return;
      video.play().catch(() => {});
    }, delay);
  };
  video.addEventListener('pointerdown', () => { userInteracted = true; });
  video.addEventListener('keydown', () => { userInteracted = true; });
  video.addEventListener('canplay', () => requestAutoPlay());
  video.addEventListener('loadedmetadata', () => requestAutoPlay());
  video.addEventListener('pause', () => {
    if (Date.now() - mountedAt < 12000 && !userInteracted) {
      requestAutoPlay(300);
    }
  });
  const onError = () => {
    const ph = wrap.querySelector('.placeholder');
    if (ph) ph.remove();
    const e = document.createElement('div');
    e.className = 'placeholder';
    e.innerHTML = `<div class="illu">${icon('alert')}</div><div>视频加载失败</div>`;
    wrap.appendChild(e);
  };
  video.addEventListener('error', onError);

  const ph = wrap.querySelector('.placeholder');
  if (ph && src.type !== 'webcam' && src.type !== 'rtsp') ph.remove();
  wrap.insertBefore(video, wrap.firstChild);

  try {
    if (src.type === 'hls') {
      if (Hls.isSupported()) {
        const hls = new Hls({
          enableWorker: true,
          liveSyncDurationCount: 3,
          liveMaxLatencyDurationCount: 10,
        });
        hls.loadSource(src.url);
        hls.attachMedia(video);
        hls.on(Hls.Events.MANIFEST_PARSED, () => requestAutoPlay());
        hls.on(Hls.Events.LEVEL_LOADED, () => requestAutoPlay());
        hls.on(Hls.Events.ERROR, (_event, data) => {
          if (!data.fatal) return;
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
            hls.startLoad();
            requestAutoPlay(500);
          } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
            hls.recoverMediaError();
            requestAutoPlay(500);
          } else {
            hls.destroy();
            onError();
          }
        });
      } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
        video.src = src.url;
        requestAutoPlay();
      } else {
        throw new Error('浏览器不支持 HLS');
      }
    } else if (src.type === 'mp4') {
      video.loop = true;
      video.src = playableVideoUrl(src);
      // 本地视频检测由后端 ffmpeg 抽帧完成，前端只负责播放
      requestAutoPlay();
    } else if (src.type === 'webcam') {
      navigator.mediaDevices.getUserMedia({ video: true, audio: false })
        .then((stream) => {
          video.srcObject = stream;
          requestAutoPlay();
        })
        .catch(() => onError());
    } else if (src.type === 'rtsp') {
      const ph = wrap.querySelector('.placeholder');
      if (ph) ph.remove();
      const e = document.createElement('div');
      e.className = 'placeholder';
      e.innerHTML = `<div class="illu">${icon('satellite')}</div><div>RTSP 需服务端转码</div>`;
      wrap.appendChild(e);
      return;
    }
  } catch (_) { /* swallow */ }
  video.addEventListener('waiting', () => {
    if (src.type !== 'hls' || Number.isNaN(video.duration)) return;
    const seekableEnd = video.seekable.length ? video.seekable.end(video.seekable.length - 1) : null;
    if (seekableEnd && seekableEnd - video.currentTime > 12) {
      video.currentTime = Math.max(0, seekableEnd - 3);
      video.play().catch(() => {});
    }
  });
  requestAutoPlay(200);
};

$('#btn-add-source-live')?.addEventListener('click', () => openModal(null));
$('#btn-add-source')?.addEventListener('click', () => openModal(null));
$('#btn-add-group')?.addEventListener('click', () => addNewGroup());
$('#btn-refresh-status').addEventListener('click', async () => {
  addLog('info', '手动刷新视频源…');
  await loadSources();
  await loadGroups();
  renderLive();
  renderSourcesTable();
});
$('#btn-probe')?.addEventListener('click', async () => {
  // 1. 写入诊断日志
  try { await openDevtools(); } catch (_) {}
  // 2. 后端探测 m3u8，确认本机网络和推流器是否可达
  const url = 'http://127.0.0.1:8080/cam-1/index.m3u8';
  addLog('info', `诊断: 后端探测 ${url} ...`);
  const r = await probeUrl(url);
  if (r && r.ok) {
    addLog('success', `m3u8 可达 status=${r.status} len=${r.content_length}`);
  } else {
    addLog('error', `m3u8 探测失败: ${JSON.stringify(r)}`);
  }
  // 3. 探测 ts 分片
  const tsUrl = url.replace(/index\.m3u8.*$/, 'seg_99999.ts');
  const r2 = await probeUrl(tsUrl);
  if (r2 && r2.ok) {
    addLog('success', `ts 分片可达 status=${r2.status} len=${r2.content_length}`);
  } else {
    addLog('warn', `ts 分片可能不存在 (status=${r2?.status ?? '?'})`);
  }
  addLog('info', '诊断完成：该结果只证明后端可达；播放问题仍以视频卡片和 WebView 网络行为为准');
});

/* -------------------- 视频管理 -------------------- */
const renderSourcesTable = () => {
  const tb = $('#src-tbody');
  const q = ($('#src-search')?.value || '').trim().toLowerCase();
  const list = sources.filter((s) => {
    if (!q) return true;
    return s.name.toLowerCase().includes(q) || (s.location || '').toLowerCase().includes(q);
  });
  if (list.length === 0) {
    tb.innerHTML = `<tr><td colspan="7" class="muted center">${sources.length === 0 ? '暂无数据' : '没有匹配的记录'}</td></tr>`;
    return;
  }
  tb.innerHTML = list.map((s) => `
    <tr>
      <td><strong>${escapeHtml(s.name)}</strong></td>
      <td><span class="tag tag-${s.type}">${s.type.toUpperCase()}</span></td>
      <td><code>${escapeHtml(s.url)}</code></td>
      <td>${escapeHtml(s.location || '—')}</td>
      <td>${s.enabled ? '<span class="status-pill online"><span class="dot"></span>已启用</span>' : '<span class="status-pill offline"><span class="dot"></span>已停用</span>'}</td>
      <td>${fmtDate(s.createdAt)}</td>
      <td>
        <button class="btn-ghost btn-edit-src" data-id="${s.id}">编辑</button>
        <button class="btn-danger btn-del-src" data-id="${s.id}">删除</button>
      </td>
    </tr>
  `).join('');
  $$('.btn-edit-src', tb).forEach((b) => b.addEventListener('click', () => openModal(b.dataset.id)));
  $$('.btn-del-src', tb).forEach((b) => b.addEventListener('click', () => removeSource(b.dataset.id)));
};
$('#src-search')?.addEventListener('input', renderSourcesTable);

/* -------------------- 监控总览 -------------------- */
const renderOverview = async () => {
  $('#ov-total').textContent = sources.length;
  const online = sources.filter((s) => {
    const st = stats.find((x) => x.id === s.id);
    return st && st.online;
  }).length;
  const totalBitrate = stats.reduce((s, x) => s + (x.bitrate || 0), 0);
  const totalViewers = stats.reduce((s, x) => s + (x.viewers || 0), 0);
  // 算法状态聚合
  const personCount = Array.from(sceneStates.values()).filter((x) => x.person).length;
  const alarmCount = Array.from(sceneStates.values()).filter((x) => x.alarm).length;
  $('#ov-online').textContent = online;
  $('#ov-online-rate').textContent = sources.length ? `${online} / ${sources.length} 路在线` : '—';
  $('#ov-bitrate').innerHTML = `${totalBitrate} <small>kbps</small>`;
  $('#ov-viewers').textContent = totalViewers;
  $('#ov-person').textContent = personCount;
  $('#ov-alarm').textContent = alarmCount;
  $('#ov-updated').textContent = stats.length ? `更新于 ${fmtTime(stats[0].ts)}` : '尚未刷新';

  // 报警 banner（永远占位，避免 UI 跳动）
  const banner = $('#ov-alarm-banner');
  const alarming = sources.filter((s) => {
    const st = sceneStates.get(s.id);
    return st && st.alarm;
  });
  if (alarming.length > 0) {
    banner.classList.add('alarm');
    banner.classList.remove('ok');
    const shown = alarming.slice(0, 5).map((s) => `<b>${escapeHtml(s.name)}</b>`).join('、');
    const more = alarming.length > 5 ? ` 等 <b>${alarming.length}</b> 路` : '';
    banner.innerHTML = `
      <span class="icon">${icon('alarm')}</span>
      <div>当前 <b>${alarming.length}</b> 路报警：${shown}${more}</div>
    `;
  } else {
    banner.classList.add('ok');
    banner.classList.remove('alarm');
    banner.innerHTML = `<span class="icon">${icon('check')}</span><div>所有通道状态正常</div>`;
  }

  const tb = $('#ov-tbody');
  if (sources.length === 0) {
    tb.innerHTML = `<tr><td colspan="10" class="muted center">暂无数据</td></tr>`;
  } else {
    tb.innerHTML = sources.map((s) => {
      const st = stats.find((x) => x.id === s.id) || {};
      const sc = sceneStates.get(s.id) || { person: false, light: false };
      const alarm = !!sc.alarm;
      const personIcon = stateMarker(sc.person, sc.person ? '在' : '不在');
      const lightIcon = stateMarker(sc.light, sc.light ? '亮' : '关');
      const alarmIcon = alarm
        ? `<span class="status-pill alarm">${icon('alarm')}报警</span>`
        : '<span class="muted">正常</span>';
      return `
        <tr>
          <td><strong>${escapeHtml(s.name)}</strong></td>
          <td>${escapeHtml(s.location || '—')}</td>
          <td>${st.online ? '<span class="status-pill online"><span class="dot"></span>在线</span>' : '<span class="status-pill offline"><span class="dot"></span>离线</span>'}</td>
          <td>${personIcon}</td>
          <td>${lightIcon}</td>
          <td>${alarmIcon}</td>
          <td>${st.bitrate || 0} kbps</td>
          <td>${st.fps || 0} fps</td>
          <td>${st.viewers || 0}</td>
          <td>${st.ts ? fmtTime(st.ts) : '—'}</td>
        </tr>`;
    }).join('');
  }

  await renderAlarmRecords();

  // 状态历史（最近 50 条）
  const histTb = $('#ov-history-tbody');
  try {
    const res = await getStateHistory(null, 50);
    const recs = (res && res.records) || [];
    if (recs.length === 0) {
      histTb.innerHTML = `<tr><td colspan="5" class="muted center">暂无记录</td></tr>`;
    } else {
      const byId = new Map(sources.map((s) => [s.id, s.name]));
      histTb.innerHTML = recs.map((r) => `
        <tr>
          <td>${fmtDate(r.ts)}</td>
          <td>${escapeHtml(byId.get(r.sourceId) || r.sourceId)}</td>
          <td>${stateMarker(r.person, r.person ? '在' : '不在')}</td>
          <td>${stateMarker(r.light, r.light ? '亮' : '关')}</td>
          <td>${r.alarm ? `<span class="status-pill alarm">${icon('alarm')}</span>` : '<span class="muted">—</span>'}</td>
        </tr>
      `).join('');
    }
  } catch (e) {
    histTb.innerHTML = `<tr><td colspan="5" class="muted center">加载失败: ${escapeHtml(e.message)}</td></tr>`;
  }
};

const alarmStatusText = (status) => ({
  suspected: '疑似',
  vlm_checking: 'VLM确认中',
  alarm_active: '报警中',
  recovering: '恢复中',
  acknowledged: '已确认',
  resolved: '已恢复',
}[status] || status || '-');

const renderAlarmRecords = async () => {
  const tb = $('#ov-alarm-tbody');
  try {
    alarms = await listAlarms({ limit: 50 });
  } catch (err) {
    tb.innerHTML = `<tr><td colspan="7" class="muted center">加载失败: ${escapeHtml(err.message || err)}</td></tr>`;
    return;
  }
  if (!alarms.length) {
    tb.innerHTML = '<tr><td colspan="7" class="muted center">暂无报警记录</td></tr>';
    return;
  }
  const byId = new Map(sources.map((s) => [s.id, s.name]));
  tb.innerHTML = alarms.map((alarm) => {
    const canAck = alarm.status === 'alarm_active' || alarm.status === 'suspected';
    const canResolve = alarm.status !== 'resolved';
    const statusClass = alarm.status === 'resolved' ? 'muted' : 'status-pill';
    const statusStyle = alarm.status === 'resolved' ? '' : 'style="background:#fee2e2;color:#991b1b;"';
    return `
      <tr>
        <td>${fmtDate(alarm.triggeredAt || alarm.firstSeenAt)}</td>
        <td>${escapeHtml(byId.get(alarm.sourceId) || alarm.sourceId)}</td>
        <td><span class="${statusClass}" ${statusStyle}>${escapeHtml(alarmStatusText(alarm.status))}</span></td>
        <td>${alarm.acknowledgedAt ? fmtDate(alarm.acknowledgedAt) : '-'}</td>
        <td>${alarm.resolvedAt ? fmtDate(alarm.resolvedAt) : '-'}</td>
        <td>${escapeHtml(alarm.note || '')}</td>
        <td>
          ${canAck ? `<button class="btn-ghost" data-ack-alarm="${alarm.id}">确认</button>` : ''}
          ${canResolve ? `<button class="btn-ghost" data-resolve-alarm="${alarm.id}">恢复</button>` : '-'}
        </td>
      </tr>
    `;
  }).join('');
  $$('[data-ack-alarm]', tb).forEach((btn) => {
    btn.addEventListener('click', () => acknowledgeAlarmRecord(btn.dataset.ackAlarm));
  });
  $$('[data-resolve-alarm]', tb).forEach((btn) => {
    btn.addEventListener('click', () => resolveAlarmRecord(btn.dataset.resolveAlarm));
  });
};

const acknowledgeAlarmRecord = async (id) => {
  try {
    await ackAlarm(id, '前端确认');
    addLog('info', '报警已确认');
    await renderOverview();
  } catch (err) {
    alert(err.message || '确认报警失败');
  }
};

const resolveAlarmRecord = async (id) => {
  try {
    await resolveAlarm(id, '前端手动恢复');
    addLog('info', '报警已恢复');
    await renderOverview();
  } catch (err) {
    alert(err.message || '恢复报警失败');
  }
};

$('#btn-refresh-alarms')?.addEventListener('click', renderAlarmRecords);

/* -------------------- 开发者检测历史 -------------------- */
let detectionHistoryRecords = [];

const fmtNum = (value, digits = 3) => {
  const n = Number(value);
  return Number.isFinite(n) ? n.toFixed(digits) : '-';
};

const populateDetectionHistorySources = () => {
  const sel = $('#det-history-source');
  if (!sel) return;
  const current = sel.value || sources[0]?.id || '';
  sel.innerHTML = sources.length
    ? sources.map((s) => `<option value="${s.id}">${escapeHtml(s.name)} · ${escapeHtml(s.location || '')}</option>`).join('')
    : '<option value="">暂无视频源</option>';
  sel.value = sources.some((s) => s.id === current) ? current : (sources[0]?.id || '');
};

const drawDetectionHistoryChart = (records) => {
  const canvas = $('#det-history-chart');
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  const cssWidth = canvas.clientWidth || 900;
  const cssHeight = canvas.clientHeight || 520;
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.max(1, Math.floor(cssWidth * dpr));
  canvas.height = Math.max(1, Math.floor(cssHeight * dpr));
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssWidth, cssHeight);

  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, cssWidth, cssHeight);
  ctx.font = '12px system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';

  const outer = { left: 58, right: 18, top: 16, bottom: 30 };
  const gap = 18;
  const plotCount = 4;
  const plotW = cssWidth - outer.left - outer.right;
  const plotH = (cssHeight - outer.top - outer.bottom - gap * (plotCount - 1)) / plotCount;
  const plots = [
    { title: '灯光检测', top: outer.top, min: 0, max: 1, ticks: ['1.00', '0.50', '0.00'] },
    { title: '人员/运动', top: outer.top + (plotH + gap), min: 0, max: 1, ticks: ['1.00', '0.50', '0.00'] },
    { title: '亮度', top: outer.top + (plotH + gap) * 2, min: 0, max: 255, ticks: ['255', '128', '0'] },
    { title: '报警/耗时', top: outer.top + (plotH + gap) * 3, min: 0, max: 1, ticks: ['1.00', '0.50', '0.00'] },
  ];

  if (!records.length) {
    ctx.fillStyle = '#94a3b8';
    ctx.fillText('暂无检测采样', outer.left + 12, outer.top + 28);
    return;
  }

  const tsMin = records[0].ts || 0;
  const tsMax = records[records.length - 1].ts || tsMin + 1;
  const span = Math.max(1, tsMax - tsMin);
  const xOf = (r) => outer.left + (((r.ts || tsMin) - tsMin) / span) * plotW;
  const processMax = Math.max(1, ...records.map((r) => Number(r.processMs) || 0));

  const drawPlotFrame = (plot) => {
    ctx.strokeStyle = '#e5e7eb';
    ctx.lineWidth = 1;
    [0, 0.5, 1].forEach((ratio, idx) => {
      const y = plot.top + plotH * ratio;
      ctx.beginPath();
      ctx.moveTo(outer.left, y);
      ctx.lineTo(outer.left + plotW, y);
      ctx.stroke();
      ctx.fillStyle = '#64748b';
      ctx.fillText(plot.ticks[idx], 8, y + 4);
    });
    ctx.strokeStyle = '#94a3b8';
    ctx.strokeRect(outer.left, plot.top, plotW, plotH);
    ctx.fillStyle = '#334155';
    ctx.font = '600 12px system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
    ctx.fillText(plot.title, outer.left + 8, plot.top + 16);
    ctx.font = '12px system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
  };

  const yOf = (plot, value) => {
    const v = Number(value) || 0;
    const norm = plot.max === plot.min ? 0 : (v - plot.min) / (plot.max - plot.min);
    return plot.top + (1 - Math.max(0, Math.min(1, norm))) * plotH;
  };

  const drawBands = (plot, predicate, color) => {
    ctx.fillStyle = color;
    let start = null;
    records.forEach((r, i) => {
      if (predicate(r) && start === null) start = i;
      const ending = start !== null && (!predicate(r) || i === records.length - 1);
      if (ending) {
        const end = predicate(r) && i === records.length - 1 ? i : i - 1;
        const x1 = xOf(records[start]);
        const x2 = xOf(records[end]);
        ctx.fillRect(x1, plot.top, Math.max(2, x2 - x1), plotH);
        start = null;
      }
    });
  };

  const drawLine = (plot, getter, color) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    records.forEach((r, i) => {
      const x = xOf(r);
      const y = yOf(plot, getter(r));
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();
  };

  plots.forEach(drawPlotFrame);
  drawBands(plots[0], (r) => r.light, 'rgba(245, 158, 11, 0.12)');
  drawBands(plots[1], (r) => r.person, 'rgba(34, 197, 94, 0.10)');
  drawBands(plots[3], (r) => r.alarm || r.alarmStatus === 'alarm_active', 'rgba(239, 68, 68, 0.14)');

  drawLine(plots[0], (r) => r.colorScore, '#2563eb');
  drawLine(plots[0], (r) => r.lightConfidence, '#d97706');
  drawLine(plots[1], (r) => r.motionScore, '#16a34a');
  drawLine(plots[1], (r) => r.personConfidence, '#7c3aed');
  drawLine(plots[2], (r) => r.lightBrightness, '#0891b2');
  drawLine(plots[3], (r) => (Number(r.processMs) || 0) / processMax, '#475569');

  ctx.fillStyle = '#64748b';
  ctx.fillText(fmtTime(tsMin), outer.left, cssHeight - 10);
  ctx.fillText(fmtTime(tsMax), Math.max(outer.left, outer.left + plotW - 58), cssHeight - 10);
  ctx.fillText(`耗时归一化，max=${processMax.toFixed(1)}ms`, outer.left + plotW - 178, plots[3].top + 16);
};

const renderDetectionHistorySummary = (records) => {
  const last = records[records.length - 1];
  $('#det-history-count').textContent = `${records.length} 条`;
  $('#det-history-range').textContent = records.length
    ? `${fmtDate(records[0].ts)} - ${fmtDate(last.ts)}`
    : '—';
  $('#det-last-light').textContent = last ? (last.light ? '开灯' : '关灯') : '—';
  $('#det-last-person').textContent = last ? (last.person ? '有人' : '无人') : '—';
  $('#det-last-alarm').textContent = last ? alarmStatusText(last.alarmStatus) : '—';
  $('#det-last-color').textContent = last ? fmtNum(last.colorScore) : '—';
  $('#det-last-motion').textContent = last ? fmtNum(last.motionScore) : '—';
  $('#det-last-process').textContent = last ? `${fmtNum(last.processMs, 1)} ms` : '—';
};

const renderDetectionHistoryTable = (records) => {
  const tb = $('#det-history-tbody');
  if (!tb) return;
  if (!records.length) {
    tb.innerHTML = '<tr><td colspan="11" class="muted center">暂无检测采样</td></tr>';
    return;
  }
  const byId = new Map(sources.map((s) => [s.id, s.name]));
  tb.innerHTML = [...records].reverse().map((r) => `
    <tr>
      <td>${fmtDate(r.ts)}</td>
      <td>${escapeHtml(byId.get(r.sourceId) || r.sourceId || '-')}</td>
      <td>${stateMarker(r.light, r.light ? '亮' : '关')}</td>
      <td>${stateMarker(r.person, r.person ? '在' : '不在')}</td>
      <td>${r.alarm || r.alarmStatus === 'alarm_active' ? `<span class="status-pill alarm">${icon('alarm')}报警</span>` : escapeHtml(alarmStatusText(r.alarmStatus))}</td>
      <td>${fmtNum(r.colorScore)}</td>
      <td>${fmtNum(r.motionScore)}</td>
      <td>${fmtNum(r.lightBrightness, 1)}</td>
      <td>灯 ${fmtNum(r.lightConfidence, 2)} / 人 ${fmtNum(r.personConfidence, 2)}</td>
      <td>${fmtNum(r.processMs, 1)}ms</td>
      <td class="mono-cell">${escapeHtml(r.reason || '')}</td>
    </tr>
  `).join('');
};

const renderDetectionHistory = async () => {
  if (!developerMode) return;
  populateDetectionHistorySources();
  const sourceId = $('#det-history-source')?.value || sources[0]?.id || null;
  const limit = Number($('#det-history-limit')?.value || 500);
  if (!sourceId) {
    detectionHistoryRecords = [];
    renderDetectionHistorySummary([]);
    renderDetectionHistoryTable([]);
    drawDetectionHistoryChart([]);
    return;
  }
  try {
    detectionHistoryRecords = await listDetectionHistory(sourceId, limit);
    renderDetectionHistorySummary(detectionHistoryRecords);
    renderDetectionHistoryTable(detectionHistoryRecords);
    drawDetectionHistoryChart(detectionHistoryRecords);
  } catch (err) {
    $('#det-history-tbody').innerHTML = `<tr><td colspan="11" class="muted center">加载失败: ${escapeHtml(err.message || err)}</td></tr>`;
    addLog('warn', `检测历史加载失败: ${err.message || err}`);
  }
};

$('#btn-refresh-det-history')?.addEventListener('click', renderDetectionHistory);
$('#det-history-source')?.addEventListener('change', renderDetectionHistory);
$('#det-history-limit')?.addEventListener('change', renderDetectionHistory);
window.addEventListener('resize', () => {
  if (!$('#view-detection-history')?.classList.contains('hidden')) {
    drawDetectionHistoryChart(detectionHistoryRecords);
  }
});

/* -------------------- 模态框 / 增删改 -------------------- */
const modal = $('#modal-mask');
const openModal = (id) => {
  const editing = id ? sources.find((s) => s.id === id) : null;
  $('#modal-title').textContent = editing ? '编辑视频源' : '新增视频源';
  $('#src-id').value = editing?.id || '';
  $('#src-name').value = editing?.name || '';
  $('#src-type').value = editing?.type || 'hls';
  $('#src-url').value = editing?.url || '';
  $('#src-location').value = editing?.location || '';
  $('#src-enabled').checked = editing ? !!editing.enabled : true;
  // 渲染分组下拉
  const sel = $('#src-group');
  sel.innerHTML = groups
    .sort((a, b) => a.order - b.order)
    .map((g) => `<option value="${g.id}">${escapeHtml(g.name)}</option>`)
    .join('');
  sel.value = editing?.groupId || 'grp-default';
  modal.classList.remove('hidden');
  setTimeout(() => $('#src-name').focus(), 50);
};
const closeModal = () => modal.classList.add('hidden');
$('#modal-close').addEventListener('click', closeModal);
$('#modal-cancel').addEventListener('click', closeModal);
modal.addEventListener('click', (e) => { if (e.target === modal) closeModal(); });

$('#btn-pick-video-file')?.addEventListener('click', async () => {
  if (!isTauriEnv) {
    alert('选择本地视频文件需要在 Tauri 应用中使用');
    return;
  }
  try {
    const selected = await openDialog({
      multiple: false,
      directory: false,
      filters: VIDEO_DIALOG_FILTERS,
    });
    if (!selected || Array.isArray(selected)) return;
    $('#src-type').value = 'mp4';
    $('#src-url').value = selected;
    $('#src-location').value = selected;
    if (!$('#src-name').value.trim()) {
      $('#src-name').value = fileNameFromPath(selected);
    }
  } catch (err) {
    alert(err.message || '选择视频文件失败');
  }
});

$('#source-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const id = $('#src-id').value;
  const payload = {
    name: $('#src-name').value.trim(),
    type: $('#src-type').value,
    url: $('#src-url').value.trim(),
    location: $('#src-location').value.trim(),
    enabled: $('#src-enabled').checked,
    groupId: $('#src-group').value || 'grp-default',
    order: 0,
  };
  try {
    if (id) {
      await updateSource(id, payload);
      addLog('success', `已更新视频源: ${payload.name}`);
    } else {
      await createSource(payload);
      addLog('success', `已新增视频源: ${payload.name}`);
    }
    await loadSources();
    renderLive();
    renderSourcesTable();
    closeModal();
  } catch (err) {
    alert(err.message || '保存失败');
  }
});

const removeSource = async (id) => {
  const s = sources.find((x) => x.id === id);
  if (!s) return;
  if (!confirm(`确定要删除视频源「${s.name}」吗？`)) return;
  try {
    await deleteSource(id);
    addLog('warn', `已删除视频源: ${s.name}`);
    await loadSources();
    renderLive();
    renderSourcesTable();
  } catch (err) {
    alert(err.message || '删除失败');
  }
};

/* -------------------- 基础设置 / 改密码 -------------------- */
$('#pw-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const oldPw = $('#pw-old').value;
  const newPw = $('#pw-new').value;
  const newPw2 = $('#pw-new2').value;
  const errEl = $('#pw-error');
  const okEl = $('#pw-ok');
  errEl.textContent = '';
  okEl.textContent = '';
  if (newPw !== newPw2) { errEl.textContent = '两次输入的新密码不一致'; return; }
  try {
    await changePassword(oldPw, newPw);
    okEl.textContent = '密码修改成功';
    $('#pw-old').value = '';
    $('#pw-new').value = '';
    $('#pw-new2').value = '';
    addLog('success', '登录密码已修改');
  } catch (err) {
    errEl.textContent = err.message || '修改失败';
  }
});

/* -------------------- 算法配置 -------------------- */
const GLOBAL_CONFIG_ID = '__global__';
let algorithmConfiguredSourceIds = [];
const DEFAULT_VLM_PROMPT = `你是一个专业的人体目标检测系统。请仔细分析这张图片，检测其中是否包含人体（完整或局部均可，包括背影、侧身、被部分遮挡的人）。

你必须严格按照以下 JSON 格式输出，不要包含任何额外文字、解释或说明：

当检测到人时：
{"has_person": true, "detections": [{"label": "person", "confidence": 0.95, "bbox": [x1, y1, x2, y2]}]}

当未检测到人时：
{"has_person": false, "detections": []}

要求：
1. bbox 坐标采用千分制归一化值（范围 0-1000），[x1, y1] 为边界框左上角，[x2, y2] 为右下角
2. confidence 为 0-1 之间的浮点数，表示检测置信度
3. 检测到的每一个人都必须单独列出一条记录
4. 仅输出 JSON，不要包含 markdown 标记、代码块或其他任何文字`;

const toInt = (value, fallback, min = 0) => {
  const n = Number.parseInt(value, 10);
  if (!Number.isFinite(n)) return fallback;
  return Math.max(min, n);
};

const toFloat = (value, fallback, min = 0, max = 1) => {
  const n = Number.parseFloat(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, n));
};

const normalizePersonMotionThreshold = (value) => {
  const n = Number(value);
  if (!Number.isFinite(n)) return 0.003;
  if (n > 0.2) return Math.max(0.001, Math.min(1, n) * 0.03);
  if (Math.abs(n - 0.020) < 0.0005) return 0.003;
  return Math.min(0.20, Math.max(0.001, n));
};

const parseWeekdays = (value) => {
  const items = String(value)
    .split(',')
    .map((item) => Number.parseInt(item.trim(), 10))
    .filter((n) => Number.isInteger(n) && n >= 1 && n <= 7);
  return [...new Set(items)].sort((a, b) => a - b);
};

const normalizeTimeText = (value, fallback) => {
  const text = String(value || '').trim();
  return /^\d{2}:\d{2}$/.test(text) ? text : fallback;
};

const vlmChatCompletionsUrl = (apiBase) => {
  const base = String(apiBase || '').trim().replace(/\/+$/, '');
  if (!base) return '-';
  return base.endsWith('/chat/completions') ? base : `${base}/chat/completions`;
};

const validateVlmApiBase = (apiBase) => {
  const base = String(apiBase || '').trim();
  if (!base) return '请先填写 API 地址';
  try {
    new URL(base);
  } catch (_) {
    return 'API 地址格式不正确';
  }
  return '';
};

const updateVlmRequestUrlPreview = () => {
  const el = $('#algo-vlm-request-url');
  if (!el) return;
  el.textContent = vlmChatCompletionsUrl($('#algo-vlm-api-base')?.value);
};

const updateVlmCostControls = () => {
  const enabled = !!$('#algo-vlm-cost-enabled')?.checked;
  [
    '#algo-vlm-price-input',
    '#algo-vlm-price-input-cache',
    '#algo-vlm-price-output',
    '#algo-vlm-price-output-cache',
  ].forEach((selector) => {
    const input = $(selector);
    if (input) input.disabled = !enabled;
  });
};

const updateYoloVisibility = () => {
  const yoloEnabled = !!$('#algo-yolo-enabled')?.checked;
  const yoloFields = $('#yolo-config-fields');
  const thresholdRow = $('#person-threshold-row');
  if (yoloFields) yoloFields.style.display = yoloEnabled ? '' : 'none';
  if (thresholdRow) thresholdRow.style.display = yoloEnabled ? 'none' : '';
};

const fillAlgorithmForm = (cfg) => {
  const win = (cfg.activeWindows || [])[0] || { weekdays: [1, 2, 3, 4, 5], start: '18:30', end: '08:30', timezone: 'Local' };
  $('#algo-enabled').checked = !!cfg.enabled;
  $('#algo-weekdays').value = (win.weekdays || [1, 2, 3, 4, 5]).join(',');
  $('#algo-start').value = win.start || '18:30';
  $('#algo-end').value = win.end || '08:30';
  $('#algo-simple-interval').value = cfg.simpleIntervalSec ?? 1;
  $('#algo-person-threshold').value = normalizePersonMotionThreshold(cfg.personThreshold ?? 0.003).toFixed(3);
  $('#algo-hold-sec').value = cfg.alarmHoldSec ?? 300;
  $('#algo-recover-sec').value = cfg.alarmRecoverSec ?? 60;
  $('#algo-recover-policy').value = cfg.recoverPolicy || 'either';
  $('#algo-vlm-limit').value = cfg.vlmHourlyLimit ?? 12;
  $('#algo-vlm-enabled').checked = !!cfg.vlmEnabled;
  $('#algo-vlm-api-base').value = cfg.vlmApiBase ?? cfg.vlm_api_base ?? '';
  $('#algo-vlm-api-key').value = cfg.vlmApiKey ?? cfg.vlm_api_key ?? '';
  $('#algo-vlm-model').value = cfg.vlmModel ?? cfg.vlm_model ?? '';
  $('#algo-vlm-temperature').value = cfg.vlmTemperature ?? cfg.vlm_temperature ?? 0.1;
  $('#algo-vlm-max-tokens').value = cfg.vlmMaxTokens ?? cfg.vlm_max_tokens ?? 2048;
  $('#algo-vlm-prompt').value = cfg.vlmPrompt ?? cfg.vlm_prompt ?? DEFAULT_VLM_PROMPT;
  $('#algo-vlm-cost-enabled').checked = !!(cfg.vlmCostEnabled ?? cfg.vlm_cost_enabled);
  $('#algo-vlm-price-input').value = cfg.vlmPriceInput ?? cfg.vlm_price_input ?? 0;
  $('#algo-vlm-price-input-cache').value = cfg.vlmPriceInputCache ?? cfg.vlm_price_input_cache ?? 0;
  $('#algo-vlm-price-output').value = cfg.vlmPriceOutput ?? cfg.vlm_price_output ?? 0;
  $('#algo-vlm-price-output-cache').value = cfg.vlmPriceOutputCache ?? cfg.vlm_price_output_cache ?? 0;
  // YOLO 配置
  $('#algo-yolo-enabled').checked = !!(cfg.yoloEnabled ?? cfg.yolo_enabled);
  $('#algo-yolo-api-base').value = cfg.yoloApiBase ?? cfg.yolo_api_base ?? 'ws://localhost:8090';
  $('#algo-yolo-confidence').value = cfg.yoloConfidence ?? cfg.yolo_confidence ?? 0.45;
  updateYoloVisibility();
  const selectedSourceId = getSelectedAlgorithmSourceId();
  const selectedSource = selectedSourceId ? sources.find((s) => s.id === selectedSourceId) : null;
  $('#algo-scope').textContent = selectedSource
    ? `通道配置 · ${selectedSource.name}`
    : '全局默认设置';
  const vlmScope = $('#vlm-scope');
  if (vlmScope) {
    vlmScope.textContent = selectedSource ? `通道配置 · ${selectedSource.name}` : '全局默认设置';
  }
  const resetBtn = $('#btn-reset-algorithm-source');
  if (resetBtn) resetBtn.style.display = selectedSourceId ? '' : 'none';
  updateVlmRequestUrlPreview();
  updateVlmCostControls();
  // 同步 chip 选中态 + 刷新 slider 数字显示
  syncChipsFromHidden();
  $('#algo-person-threshold')?.dispatchEvent(new Event('input'));
};

const getSelectedAlgorithmSourceId = () => {
  const value = $('#algo-source')?.value || GLOBAL_CONFIG_ID;
  return value === GLOBAL_CONFIG_ID ? null : value;
};

const populateAlgorithmSourceOptions = () => {
  const sel = $('#algo-source');
  if (!sel) return;
  const current = sel.value || GLOBAL_CONFIG_ID;
  const configuredSources = sources.filter((source) => algorithmConfiguredSourceIds.includes(source.id));
  sel.innerHTML = [
    '<option value="__global__">全局默认设置</option>',
    ...configuredSources.map((source) => `<option value="${source.id}">${escapeHtml(source.name)} · ${escapeHtml(source.location || '')}</option>`),
  ].join('');
  sel.value = [...sel.options].some((option) => option.value === current) ? current : GLOBAL_CONFIG_ID;
};

const populateAlgorithmAddOptions = () => {
  const sel = $('#algo-add-source');
  if (!sel) return;
  const candidates = sources.filter((source) => !algorithmConfiguredSourceIds.includes(source.id));
  sel.innerHTML = candidates.length
    ? candidates.map((source) => `<option value="${source.id}">${escapeHtml(source.name)} · ${escapeHtml(source.location || '')}</option>`).join('')
    : '<option value="">没有可添加的视频源</option>';
  $('#btn-confirm-add-algorithm-source')?.toggleAttribute('disabled', !candidates.length);
};

const algorithmPayloadFromForm = () => {
  const weekdays = parseWeekdays($('#algo-weekdays').value);
  const sourceId = getSelectedAlgorithmSourceId();
  return {
    ...(algorithmConfig || {}),
    enabled: $('#algo-enabled').checked,
    scope: sourceId ? 'source' : 'global',
    scopeId: sourceId,
    activeWindows: [{
      weekdays: weekdays.length ? weekdays : [1, 2, 3, 4, 5],
      start: normalizeTimeText($('#algo-start').value, '18:30'),
      end: normalizeTimeText($('#algo-end').value, '08:30'),
      timezone: 'Local',
    }],
    exceptionWindows: algorithmConfig?.exceptionWindows || [],
    simpleIntervalSec: toInt($('#algo-simple-interval').value, 1, 1),
    vlmEnabled: $('#algo-vlm-enabled').checked,
    vlmApiBase: $('#algo-vlm-api-base').value.trim(),
    vlmApiKey: $('#algo-vlm-api-key').value.trim(),
    vlmModel: $('#algo-vlm-model').value.trim(),
    vlmPrompt: $('#algo-vlm-prompt').value.trim() || DEFAULT_VLM_PROMPT,
    vlmTemperature: toFloat($('#algo-vlm-temperature').value, 0.1, 0, 2),
    vlmMaxTokens: toInt($('#algo-vlm-max-tokens').value, 2048, 16),
    vlmCostEnabled: $('#algo-vlm-cost-enabled').checked,
    vlmPriceInput: toFloat($('#algo-vlm-price-input').value, 0, 0, Number.MAX_SAFE_INTEGER),
    vlmPriceInputCache: toFloat($('#algo-vlm-price-input-cache').value, 0, 0, Number.MAX_SAFE_INTEGER),
    vlmPriceOutput: toFloat($('#algo-vlm-price-output').value, 0, 0, Number.MAX_SAFE_INTEGER),
    vlmPriceOutputCache: toFloat($('#algo-vlm-price-output-cache').value, 0, 0, Number.MAX_SAFE_INTEGER),
    personThreshold: toFloat($('#algo-person-threshold').value, 0.003, 0.001, 0.20),
    alarmHoldSec: toInt($('#algo-hold-sec').value, 300, 0),
    alarmRecoverSec: toInt($('#algo-recover-sec').value, 60, 0),
    recoverPolicy: $('#algo-recover-policy').value,
    vlmHourlyLimit: toInt($('#algo-vlm-limit').value, 12, 0),
    roiVersion: algorithmConfig?.roiVersion ?? null,
    yoloEnabled: $('#algo-yolo-enabled').checked,
    yoloApiBase: ($('#algo-yolo-api-base')?.value || 'ws://localhost:8090').trim(),
    yoloConfidence: toFloat($('#algo-yolo-confidence')?.value, 0.45, 0.1, 0.95),
  };
};

const renderAlgorithmSettings = async () => {
  try {
    algorithmConfiguredSourceIds = await listAlgorithmConfigSources();
    populateAlgorithmSourceOptions();
    populateAlgorithmAddOptions();
    const sourceId = getSelectedAlgorithmSourceId();
    algorithmConfig = await getAlgorithmConfig(sourceId);
    fillAlgorithmForm(algorithmConfig);
  } catch (err) {
    addLog('warn', `算法配置加载失败: ${err.message || err}`);
  }
};

const renderVlmTestResult = (result, title = '连接成功，模型可用。') => {
  const el = $('#vlm-test-result');
  if (!el) return;
  const usage = result.usage || {};
  const promptTokens = usage.promptTokens ?? usage.prompt_tokens ?? 0;
  const completionTokens = usage.completionTokens ?? usage.completion_tokens ?? 0;
  const totalTokens = usage.totalTokens ?? usage.total_tokens ?? 0;
  const promptCached = usage.promptCachedTokens ?? usage.prompt_cached_tokens ?? 0;
  const completionCached = usage.completionCachedTokens ?? usage.completion_cached_tokens ?? 0;
  const costEnabled = !!(result.costEnabled ?? result.cost_enabled);
  const cost = result.cost == null ? null : Number(result.cost || 0);
  el.classList.add('success');
  el.innerHTML = [
    `<div>${title}</div>`,
    `<div>Tokens：输入 ${promptTokens}，输出 ${completionTokens}，合计 ${totalTokens}</div>`,
    (promptCached || completionCached)
      ? `<div>缓存命中：输入 ${promptCached}，输出 ${completionCached}</div>`
      : '',
    costEnabled && cost != null
      ? `<div>估算费用：¥${cost.toFixed(6)}</div>`
      : '<div>费用估算：未启用</div>',
    result.requestUrl ? `<div>请求地址：${escapeHtml(result.requestUrl)}</div>` : '',
    result.requestBody ? `<pre class="inline-pre">${escapeHtml(JSON.stringify(result.requestBody, null, 2))}</pre>` : '',
    result.reply ? `<div>模型回复：</div><pre class="inline-pre">${escapeHtml(result.reply)}</pre>` : '',
  ].filter(Boolean).join('');
};

const renderSettings = async () => {
  await renderBasicSettings();
  await renderDetectionSettings();
  await renderNotificationSettings();
};

const renderBasicSettings = async () => {
  try {
    const dataDir = await getDataDir();
    const dataDirEl = $('#data-dir');
    if (dataDirEl) dataDirEl.textContent = dataDir;
  } catch (e) {
    console.warn('获取数据目录失败:', e);
  }
  await renderDeveloperSettings();
};

const renderDetectionSettings = async () => {
  await renderAlgorithmSettings();
  await renderRoiSettings();
};

const renderDeveloperSettings = async () => {
  try {
    const cfg = await getAlgorithmConfig(null);
    setDeveloperMode(!!(cfg.developerMode ?? cfg.developer_mode));
    const input = $('#developer-mode');
    if (input) input.checked = developerMode;
    const testIds = new Set(TEST_SOURCE_IDS);
    const hasTestSources = sources.some((s) =>
      testIds.has(s.id) || s.groupId === TEST_VIDEO_GROUP_ID || s.group_id === TEST_VIDEO_GROUP_ID
    );
    const testToggle = $('#test-sources-toggle');
    if (testToggle) testToggle.checked = hasTestSources;
    await refreshFfmpegStatus();
    applyStateIcons();
  } catch (err) {
    addLog('warn', `开发设置加载失败: ${err.message || err}`);
  }
};

const renderToolStatus = (status) => {
  const el = $('#ffmpeg-status-text');
  const row = $('#ffmpeg-check-row');
  if (!el) return;
  const ffmpeg = status?.ffmpeg || {};
  const ffprobe = status?.ffprobe || {};
  const ok = !!status?.ok;
  row?.classList.toggle('tool-ok', ok);
  row?.classList.toggle('tool-error', !ok);
  if (ok) {
    el.textContent = `可用 · ${ffmpeg.version || ffmpeg.path} · ${ffprobe.version || ffprobe.path}`;
    el.title = `ffmpeg: ${ffmpeg.path}\n${ffmpeg.version || ''}\nffprobe: ${ffprobe.path}\n${ffprobe.version || ''}`;
  } else {
    const errors = [ffmpeg.error, ffprobe.error].filter(Boolean).join('；');
    el.textContent = errors || '未找到 ffmpeg / ffprobe';
    el.title = '请将 ffmpeg.exe 和 ffprobe.exe 放到程序目录，或把 ffmpeg 安装目录加入 PATH';
  }
};

const refreshFfmpegStatus = async () => {
  const el = $('#ffmpeg-status-text');
  if (el) el.textContent = '检测中...';
  try {
    const status = await checkFfmpegStatus();
    renderToolStatus(status);
    if (!status.ok) addLog('warn', '未检测到完整 ffmpeg / ffprobe，后端抽帧检测不可用');
    return status;
  } catch (err) {
    renderToolStatus({ ok: false, ffmpeg: { error: err.message || String(err) }, ffprobe: {} });
    addLog('warn', `ffmpeg 检测失败: ${err.message || err}`);
    return null;
  }
};

$('#btn-check-ffmpeg')?.addEventListener('click', async () => {
  const status = await refreshFfmpegStatus();
  if (status?.ok) addLog('success', 'ffmpeg / ffprobe 检测通过');
});

const ensureDeveloperNotificationTarget = async () => {
  try {
    const targets = await listNotificationTargets();
    const exists = (targets || []).some((target) => {
      const name = target.name || '';
      const url = target.url || '';
      return name === DEFAULT_DEV_NOTIFY_TARGET.name || url === DEFAULT_DEV_NOTIFY_TARGET.url;
    });
    if (exists) return;
    await createNotificationTarget(DEFAULT_DEV_NOTIFY_TARGET);
    if (!$('#view-notification-settings')?.classList.contains('hidden')) {
      await renderNotificationSettings();
    }
    addLog('info', '开发者模式已添加默认企业内部通知渠道');
  } catch (err) {
    addLog('warn', `默认通知渠道添加失败: ${err.message || err}`);
  }
};

$('#developer-mode')?.addEventListener('change', async (e) => {
  try {
    const cfg = await getAlgorithmConfig(null);
    const payload = {
      ...cfg,
      developerMode: !!e.target.checked,
      scope: 'global',
      scopeId: null,
    };
    const saved = await updateAlgorithmConfig(null, payload);
    setDeveloperMode(!!(saved.developerMode ?? saved.developer_mode));
    e.target.checked = developerMode;
    const devMsg = $('#developer-ok');
    if (devMsg) devMsg.textContent = '开发者模式已保存';
    if (developerMode) await ensureDeveloperNotificationTarget();
    renderLive();
    addLog('info', developerMode ? '开发者模式已开启' : '开发者模式已关闭');
  } catch (err) {
    e.target.checked = developerMode;
    alert(err.message || '保存开发设置失败');
  }
});

const importTestSourcesWithDialog = async () => {
  if (!isTauriEnv) {
    alert('导入测试视频源需要在 Tauri 应用中使用');
    return false;
  }
  try {
    addLog('info', '正在打开测试视频文件夹选择窗口...');
    const folder = await openDialog({
      multiple: false,
      directory: true,
    });
    if (!folder || Array.isArray(folder)) return false;
    const result = await importTestSourcesFromFolder(folder);
    await loadSources();
    await loadGroups();
    const msg = $('#test-sources-ok');
    const imported = result?.imported ?? 0;
    const skipped = result?.skipped ?? 0;
    if (msg) msg.textContent = `已导入 ${imported} 个测试视频源，跳过 ${skipped} 个`;
    addLog('info', `已导入测试视频源: ${imported} 个，跳过 ${skipped} 个`);
    renderLive();
    renderSourcesTable();
    await renderDeveloperSettings();
    return true;
  } catch (err) {
    addLog('error', `导入测试视频源失败: ${err.message || err}`);
    alert(err.message || '导入测试视频源失败');
    return false;
  }
};

$('#btn-import-test-sources')?.addEventListener('click', async () => {
  await importTestSourcesWithDialog();
});

$('#test-sources-toggle')?.addEventListener('change', async (e) => {
  const enabled = !!e.target.checked;
  if (enabled) {
    const ok = await importTestSourcesWithDialog();
    e.target.checked = ok;
    return;
  }
  try {
    await setTestSourcesEnabled(false);
    await loadSources();
    await loadGroups();
    const msg = $('#test-sources-ok');
    if (msg) msg.textContent = '测试视频源已移除';
    addLog('info', '测试视频源已移除');
    renderLive();
    renderSourcesTable();
  } catch (err) {
    e.target.checked = true;
    addLog('error', `移除测试视频源失败: ${err.message || err}`);
    alert(err.message || '移除测试视频源失败');
  }
});

$('#btn-reset-all-data')?.addEventListener('click', async () => {
  const first = confirm('确定要初始化并清空所有业务配置和状态吗？登录密码会保留，但视频源、分组、算法、ROI、通知、报警和历史都会被清空。');
  if (!first) return;
  const typed = prompt('请输入“初始化”确认执行。');
  if (typed !== '初始化') return;
  try {
    await resetAllAppData();
    sources = [];
    groups = [];
    stats = [];
    alarms = [];
    algorithmConfig = null;
    roiConfig = null;
    notificationTargets = [];
    notificationHistory = [];
    sceneStates = new Map();
    runtimeStatuses = new Map();
    vlmStates = new Map();
    developerMode = false;
    await loadSources();
    await loadGroups();
    await renderBasicSettings();
    renderLive();
    renderSourcesTable();
    renderOverview();
    addLog('warn', '已初始化全部业务配置和运行状态');
  } catch (err) {
    addLog('error', `初始化失败: ${err.message || err}`);
    alert(err.message || '初始化失败');
  }
});

$('#algorithm-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  try {
    const sourceId = getSelectedAlgorithmSourceId();
    const payload = algorithmPayloadFromForm();
    algorithmConfig = await updateAlgorithmConfig(sourceId, payload);
    fillAlgorithmForm(algorithmConfig);
    addLog('info', sourceId ? '通道算法配置已保存' : '全局默认设置已保存');
  } catch (err) {
    alert(err.message || '保存算法配置失败');
  }
});

$('#vlm-form')?.addEventListener('submit', async (e) => {
  e.preventDefault();
  try {
    const sourceId = getSelectedAlgorithmSourceId();
    const payload = algorithmPayloadFromForm();
    algorithmConfig = await updateAlgorithmConfig(sourceId, payload);
    fillAlgorithmForm(algorithmConfig);
    addLog('info', sourceId ? '通道 VLM 配置已保存' : '全局 VLM 配置已保存');
  } catch (err) {
    alert(err.message || '保存 VLM 配置失败');
  }
});

$('#btn-reload-algorithm').addEventListener('click', renderAlgorithmSettings);
$('#btn-reload-vlm')?.addEventListener('click', renderAlgorithmSettings);
$('#algo-vlm-api-base')?.addEventListener('input', updateVlmRequestUrlPreview);
$('#algo-vlm-cost-enabled')?.addEventListener('change', updateVlmCostControls);
$('#algo-yolo-enabled')?.addEventListener('change', updateYoloVisibility);
$('#btn-test-yolo')?.addEventListener('click', async () => {
  const btn = $('#btn-test-yolo');
  const el = $('#yolo-test-result');
  const apiBase = ($('#algo-yolo-api-base')?.value || 'ws://localhost:8090').trim();
  if (!apiBase) {
    if (el) {
      el.classList.remove('success');
      el.textContent = '请填写 YOLO 服务器地址';
    }
    return;
  }
  if (el) {
    el.classList.remove('success');
    el.textContent = `正在测试连接 ${apiBase} ...`;
  }
  if (btn) btn.disabled = true;
  try {
    const result = await testYoloConnection(apiBase);
    if (el) {
      el.classList.add('success');
      const count = result.count ?? 0;
      const processMs = result.processMs != null ? Number(result.processMs).toFixed(1) : '-';
      el.innerHTML = `连接成功：识别到 ${count} 个目标，服务器处理耗时 ${processMs}ms<br><span class="field-hint">${escapeHtml(result.url || apiBase)}</span>`;
    }
  } catch (err) {
    if (el) {
      el.classList.remove('success');
      el.textContent = `连接失败：${err.message || err || '未知错误'}`;
    }
  } finally {
    if (btn) btn.disabled = false;
  }
});
$('#btn-test-vlm')?.addEventListener('click', async () => {
  const btn = $('#btn-test-vlm');
  const el = $('#vlm-test-result');
  const validationError = validateVlmApiBase($('#algo-vlm-api-base')?.value);
  if (validationError) {
    if (el) {
      el.classList.remove('success');
      el.innerHTML = `模型测试失败：${escapeHtml(validationError)}<br>请求地址：${escapeHtml(vlmChatCompletionsUrl($('#algo-vlm-api-base')?.value))}`;
    }
    return;
  }
  if (el) {
    el.classList.remove('success');
    el.innerHTML = `正在测试模型连接...<br>请求地址：${escapeHtml(vlmChatCompletionsUrl($('#algo-vlm-api-base')?.value))}`;
  }
  if (btn) btn.disabled = true;
  try {
    const result = await testVlmConfig(algorithmPayloadFromForm());
    renderVlmTestResult(result);
  } catch (err) {
    if (el) {
      el.classList.remove('success');
      el.innerHTML = `模型测试失败：<pre class="inline-pre">${escapeHtml(err.message || err || '未知错误')}</pre>`;
    }
  } finally {
    if (btn) btn.disabled = false;
  }
});
$('#btn-test-vlm-vision')?.addEventListener('click', async () => {
  const btn = $('#btn-test-vlm-vision');
  const el = $('#vlm-test-result');
  const validationError = validateVlmApiBase($('#algo-vlm-api-base')?.value);
  if (validationError) {
    if (el) {
      el.classList.remove('success');
      el.innerHTML = `图片识别测试失败：${escapeHtml(validationError)}`;
    }
    return;
  }
  // 用当前选择的算法源，或第一个启用的源
  const sourceId = getSelectedAlgorithmSourceId()
    || sources.filter((s) => s.enabled)[0]?.id;
  if (!sourceId) {
    if (el) {
      el.classList.remove('success');
      el.innerHTML = '图片识别测试失败：没有可用的视频源';
    }
    return;
  }
  const sourceName = sources.find((s) => s.id === sourceId)?.name || sourceId;
  if (el) {
    el.classList.remove('success');
    el.innerHTML = `正在对「${escapeHtml(sourceName)}」抽帧并调用模型识别...<br>请求地址：${escapeHtml(vlmChatCompletionsUrl($('#algo-vlm-api-base')?.value))}`;
  }
  if (btn) btn.disabled = true;
  try {
    const payload = { ...algorithmPayloadFromForm(), sourceId };
    const result = await testVlmVision(payload);
    renderVlmTestResult(result, `图片识别测试（${escapeHtml(sourceName)}）`);
  } catch (err) {
    if (el) {
      el.classList.remove('success');
      el.innerHTML = `图片识别测试失败：<pre class="inline-pre">${escapeHtml(err.message || err || '未知错误')}</pre>`;
    }
  } finally {
    if (btn) btn.disabled = false;
  }
});
$('#btn-add-algorithm-source')?.addEventListener('click', () => {
  populateAlgorithmAddOptions();
  $('#algo-add-row')?.classList.toggle('hidden');
});
$('#btn-confirm-add-algorithm-source')?.addEventListener('click', async () => {
  const sourceId = $('#algo-add-source')?.value;
  if (!sourceId) return;
  try {
    const globalConfig = await getAlgorithmConfig(null);
    const payload = {
      ...globalConfig,
      scope: 'source',
      scopeId: sourceId,
    };
    await updateAlgorithmConfig(sourceId, payload);
    algorithmConfiguredSourceIds = await listAlgorithmConfigSources();
    populateAlgorithmSourceOptions();
    populateAlgorithmAddOptions();
    $('#algo-source').value = sourceId;
    $('#algo-add-row')?.classList.add('hidden');
    algorithmConfig = await getAlgorithmConfig(sourceId);
    fillAlgorithmForm(algorithmConfig);
    addLog('info', '通道算法配置已添加');
  } catch (err) {
    alert(err.message || '添加通道算法配置失败');
  }
});
$('#btn-reset-algorithm-source')?.addEventListener('click', async () => {
  const sourceId = getSelectedAlgorithmSourceId();
  if (!sourceId) return;
  const sourceName = sources.find((item) => item.id === sourceId)?.name || sourceId;
  if (!confirm(`确定删除「${sourceName}」的算法自定义配置吗？删除后该通道会继承全局默认设置。`)) return;
  try {
    await deleteAlgorithmConfig(sourceId);
    algorithmConfiguredSourceIds = await listAlgorithmConfigSources();
    populateAlgorithmSourceOptions();
    populateAlgorithmAddOptions();
    $('#algo-source').value = GLOBAL_CONFIG_ID;
    algorithmConfig = await getAlgorithmConfig(null);
    fillAlgorithmForm(algorithmConfig);
    addLog('info', `已删除算法自定义配置: ${sourceName}`);
  } catch (err) {
    alert(err.message || '删除算法自定义配置失败');
  }
});

/* -------------------- ROI 标定 -------------------- */
const clamp01 = (value, fallback = 0) => {
  const n = Number.parseFloat(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(1, Math.max(0, n));
};

const DEFAULT_COLOR_THRESHOLD = 0.015;
const GLOBAL_ROI_SOURCE_ID = '__global__';
let roiConfiguredSourceIds = [];

const normalizeColorThreshold = (value, fallback) => {
  const n = Number.parseFloat(value);
  if (!Number.isFinite(n) || n > 0.2) return fallback;
  return Math.min(0.2, Math.max(0, n));
};

/* ---- ROI 预览区视频播放 ---- */
let roiHls = null;

const destroyRoiVideo = () => {
  if (roiHls) { roiHls.destroy(); roiHls = null; }
  const wrap = $('#roi-video-wrap');
  const preview = $('#roi-preview');
  if (!wrap) return;
  const video = wrap.querySelector('video');
  if (video) { video.pause(); video.removeAttribute('src'); video.remove(); }
  preview?.classList.remove('has-video');
};

const mountRoiVideo = (src) => {
  destroyRoiVideo();
  const wrap = $('#roi-video-wrap');
  const preview = $('#roi-preview');
  if (!wrap || !src) return;

  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  video.autoplay = true;

  const mountedAt = Date.now();
  const requestAutoPlay = (delay = 0) => {
    clearTimeout(video._roiPlayTimer);
    video._roiPlayTimer = setTimeout(() => {
      if (video.ended || !video.isConnected) return;
      video.play().catch(() => {});
    }, delay);
  };
  const showPlaceholder = () => {
    preview?.classList.remove('has-video');
  };

  // 先把 video 插进去（CSS 透明背景，loading 阶段不会突兀），并尽早隐藏占位符
  wrap.insertBefore(video, wrap.firstChild);
  video.addEventListener('playing', () => {
    preview?.classList.add('has-video');
  }, { once: true });
  video.addEventListener('canplay', () => {
    preview?.classList.add('has-video');
    requestAutoPlay();
  });
  video.addEventListener('loadedmetadata', () => requestAutoPlay());
  video.addEventListener('pause', () => {
    if (Date.now() - mountedAt < 12000) requestAutoPlay(300);
  });
  video.addEventListener('error', () => showPlaceholder());

  try {
    if (src.type === 'hls') {
      if (Hls.isSupported()) {
        const hls = new Hls({
          enableWorker: true,
          liveSyncDurationCount: 3,
          liveMaxLatencyDurationCount: 10,
        });
        hls.loadSource(src.url);
        hls.attachMedia(video);
        hls.on(Hls.Events.MANIFEST_PARSED, () => requestAutoPlay());
        hls.on(Hls.Events.LEVEL_LOADED, () => requestAutoPlay());
        hls.on(Hls.Events.ERROR, (_event, data) => {
          if (!data.fatal) return;
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
            hls.startLoad();
            requestAutoPlay(500);
          } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
            hls.recoverMediaError();
            requestAutoPlay(500);
          } else {
            hls.destroy();
            roiHls = null;
            showPlaceholder();
          }
        });
        roiHls = hls;
      } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
        video.src = src.url;
        requestAutoPlay();
      } else {
        showPlaceholder();
        return;
      }
    } else if (src.type === 'mp4') {
      video.loop = true;
      video.src = playableVideoUrl(src);
      requestAutoPlay();
    } else if (src.type === 'webcam') {
      navigator.mediaDevices.getUserMedia({ video: true, audio: false })
        .then((stream) => { video.srcObject = stream; requestAutoPlay(); })
        .catch(() => showPlaceholder());
    } else {
      showPlaceholder();
      return;
    }
  } catch (_) {
    showPlaceholder();
    return;
  }
};

const populateRoiSourceOptions = () => {
  const sel = $('#roi-source');
  if (!sel) return;
  const current = sel.value || GLOBAL_ROI_SOURCE_ID;
  const configuredSources = sources.filter((s) => roiConfiguredSourceIds.includes(s.id));
  sel.innerHTML = [
    '<option value="__global__">全局默认设置</option>',
    ...configuredSources.map((s) => `<option value="${s.id}">${escapeHtml(s.name)} · ${escapeHtml(s.location || '未填写位置')}</option>`),
  ].join('');
  sel.value = [...sel.options].some((option) => option.value === current)
    ? current
    : GLOBAL_ROI_SOURCE_ID;
};

const populateRoiAddOptions = () => {
  const sel = $('#roi-add-source');
  if (!sel) return;
  const candidates = sources.filter((s) => !roiConfiguredSourceIds.includes(s.id));
  sel.innerHTML = candidates.length
    ? candidates.map((s) => `<option value="${s.id}">${escapeHtml(s.name)} · ${escapeHtml(s.location || '未填写位置')}</option>`).join('')
    : '<option value="">没有可添加的视频源</option>';
  $('#btn-confirm-add-roi-source')?.toggleAttribute('disabled', !candidates.length);
};

const getSelectedRoiSourceId = () => {
  const value = $('#roi-source')?.value || GLOBAL_ROI_SOURCE_ID;
  return value === GLOBAL_ROI_SOURCE_ID ? null : value;
};

const getRoiTestSourceId = () => getSelectedRoiSourceId() || sources[0]?.id || null;

const updateRoiPreview = () => {
  const box = $('#roi-preview-box');
  if (!box) return;
  const x = clamp01($('#roi-x').value, 0);
  const y = clamp01($('#roi-y').value, 0);
  const w = Math.max(0.01, Math.min(1 - x, clamp01($('#roi-w').value, 1)));
  const h = Math.max(0.01, Math.min(1 - y, clamp01($('#roi-h').value, 1)));
  box.style.left = `${x * 100}%`;
  box.style.top = `${y * 100}%`;
  box.style.width = `${w * 100}%`;
  box.style.height = `${h * 100}%`;
};

const fillRoiForm = (cfg) => {
  const roi = (cfg.personRois || cfg.person_rois || [])[0]
    || (cfg.lightRois || cfg.light_rois || [])[0]
    || { x: 0, y: 0, w: 1, h: 1 };
  $('#roi-x').value = roi.x ?? 0;
  $('#roi-y').value = roi.y ?? 0;
  $('#roi-w').value = roi.w ?? 1;
  $('#roi-h').value = roi.h ?? 1;
  $('#roi-light').value = normalizeColorThreshold(
    cfg.lightThreshold ?? cfg.light_threshold
      ?? cfg.lightOnThreshold ?? cfg.light_on_threshold,
    DEFAULT_COLOR_THRESHOLD,
  ).toFixed(3);
  updateRoiPreview();
};

const roiPayloadFromForm = () => {
  const sourceId = getSelectedRoiSourceId();
  const x = clamp01($('#roi-x').value, 0);
  const y = clamp01($('#roi-y').value, 0);
  const w = Math.max(0.01, Math.min(1 - x, clamp01($('#roi-w').value, 1)));
  const h = Math.max(0.01, Math.min(1 - y, clamp01($('#roi-h').value, 1)));
  const lightThreshold = normalizeColorThreshold($('#roi-light').value, DEFAULT_COLOR_THRESHOLD);
  return {
    ...(roiConfig || {}),
    sourceId: sourceId || GLOBAL_ROI_SOURCE_ID,
    version: roiConfig?.version || `roi-${Date.now()}`,
    lightRois: [{ id: 'light-main', label: '主灯光区域', x, y, w, h }],
    excludeRois: roiConfig?.excludeRois || [],
    personRois: [{ id: 'person-main', label: '人员运动检测区域', x, y, w, h }],
    lightThreshold,
    updatedAt: Date.now(),
  };
};

const loadSelectedRoi = async () => {
  const sourceId = getSelectedRoiSourceId();
  // 全局模式：使用第一个可用视频源作为预览；单独配置模式：使用被选中的视频源
  const previewSource = sourceId
    ? sources.find((s) => s.id === sourceId)
    : sources[0];
  const resetBtn = $('#btn-reset-roi-source');
  if (resetBtn) resetBtn.style.display = sourceId ? '' : 'none';
  if (previewSource) mountRoiVideo(previewSource);
  else destroyRoiVideo();
  try {
    roiConfig = await getRoiConfig(sourceId);
    fillRoiForm(roiConfig);
  } catch (err) {
    addLog('warn', `ROI 配置加载失败: ${err.message || err}`);
  }
};

const renderRoiSettings = async () => {
  roiConfiguredSourceIds = await listRoiConfigSources();
  populateRoiSourceOptions();
  populateRoiAddOptions();
  await loadSelectedRoi();
};

$('#roi-source')?.addEventListener('change', loadSelectedRoi);
['#roi-x', '#roi-y', '#roi-w', '#roi-h'].forEach((id) => {
  $(id)?.addEventListener('input', updateRoiPreview);
});
$('#btn-add-roi-source')?.addEventListener('click', () => {
  populateRoiAddOptions();
  $('#roi-add-row')?.classList.toggle('hidden');
});
$('#btn-confirm-add-roi-source')?.addEventListener('click', async () => {
  const sourceId = $('#roi-add-source')?.value;
  if (!sourceId) return;
  try {
    const globalConfig = await getRoiConfig(null);
    const payload = {
      ...globalConfig,
      sourceId,
      version: `roi-${Date.now()}`,
      updatedAt: Date.now(),
    };
    await updateRoiConfig(sourceId, payload);
    roiConfiguredSourceIds = await listRoiConfigSources();
    populateRoiSourceOptions();
    populateRoiAddOptions();
    $('#roi-source').value = sourceId;
    $('#roi-add-row')?.classList.add('hidden');
    await loadSelectedRoi();
    addLog('info', '通道 ROI 配置已添加');
  } catch (err) {
    alert(err.message || '添加通道 ROI 配置失败');
  }
});
$('#btn-reload-roi')?.addEventListener('click', loadSelectedRoi);
$('#btn-reset-roi-source')?.addEventListener('click', async () => {
  const sourceId = getSelectedRoiSourceId();
  if (!sourceId) return;
  const sourceName = sources.find((item) => item.id === sourceId)?.name || sourceId;
  if (!confirm(`确定删除「${sourceName}」的 ROI 自定义配置吗？删除后该通道会继承全局默认设置。`)) return;
  try {
    await deleteRoiConfig(sourceId);
    roiConfiguredSourceIds = await listRoiConfigSources();
    populateRoiSourceOptions();
    populateRoiAddOptions();
    $('#roi-source').value = GLOBAL_ROI_SOURCE_ID;
    roiConfig = await getRoiConfig(null);
    fillRoiForm(roiConfig);
    await loadSelectedRoi();
    addLog('info', `已删除 ROI 自定义配置: ${sourceName}`);
  } catch (err) {
    alert(err.message || '删除 ROI 自定义配置失败');
  }
});
$('#btn-test-roi')?.addEventListener('click', async () => {
  const sourceId = getRoiTestSourceId();
  if (!sourceId) {
    alert('请先添加视频源，全局默认设置测试需要用一个视频源抽帧');
    return;
  }
  try {
    const result = await testRoiConfig(sourceId, roiPayloadFromForm());
    const el = $('#roi-test-result');
    if (el) {
      el.classList.toggle('success', !!result.light);
      el.textContent = `测试结果：灯光${result.light ? '开灯' : '关灯'}，色彩分数 ${Number(result.colorScore || result.color_score || 0).toFixed(3)}，亮度 ${Number(result.brightness || 0).toFixed(1)}，置信度 ${Number(result.confidence || 0).toFixed(2)}，耗时 ${Number(result.processMs || result.process_ms || 0).toFixed(2)}ms`;
    }
    addLog('info', 'ROI 测试完成');
  } catch (err) {
    alert(err.message || 'ROI 测试失败');
  }
});
$('#roi-form')?.addEventListener('submit', async (e) => {
  e.preventDefault();
  const sourceId = getSelectedRoiSourceId();
  try {
    const payload = roiPayloadFromForm();
    roiConfig = await updateRoiConfig(sourceId, payload);
    fillRoiForm(roiConfig);
    addLog('info', sourceId ? '通道 ROI 配置已保存' : '全局默认设置已保存');
  } catch (err) {
    alert(err.message || '保存 ROI 失败');
  }
});

/* -------------------- 通知配置 -------------------- */
const CHANNEL_LABELS = {
  webhook: 'Webhook',
  feishu: '飞书',
  wechat_work: '企业微信',
  qqbot: 'QQ',
};

const CHANNEL_URL_HINTS = {
  webhook: '支持 mock://local 走本地测试通道',
  feishu: '飞书群机器人 Webhook 地址，例如 https://open.feishu.cn/open-apis/bot/v2/hook/xxx',
  wechat_work: '企业微信群机器人 Webhook 地址，例如 https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=xxx',
  qqbot: 'QQ 群机器人 Webhook 地址',
};

const CHANNEL_URL_PLACEHOLDERS = {
  webhook: 'https://example.com/webhook',
  feishu: 'https://open.feishu.cn/open-apis/bot/v2/hook/xxxx',
  wechat_work: 'https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=xxxx',
  qqbot: 'https://api.sgroup.qq.com/...',
};

const DEFAULT_NOTIFY_BODY_TEMPLATE = JSON.stringify({
  title: '[EcoAlert] {{event}}',
  video_source: '{{source_name}}',
  area: '{{location}}',
  source_url: '{{source_url}}',
  alarm: '{{alarm}}',
  person: '{{person}}',
  light: '{{light}}',
  time: '{{ts}}',
}, null, 2);

const CHANNEL_DEFAULT_BODY_TEMPLATES = {
  webhook: DEFAULT_NOTIFY_BODY_TEMPLATE,
  feishu: JSON.stringify({
    msg_type: 'text',
    content: {
      text: '[EcoAlert] {{event}}\n视频源: {{source_name}}\n区域: {{location}}\n有人: {{person}}\n亮灯: {{light}}\n时间: {{ts}}',
    },
  }, null, 2),
  wechat_work: JSON.stringify({
    msgtype: 'text',
    text: {
      content: '[EcoAlert] {{event}}\n视频源: {{source_name}}\n区域: {{location}}\n有人: {{person}}\n亮灯: {{light}}\n时间: {{ts}}',
    },
  }, null, 2),
  qqbot: JSON.stringify({
    msg_type: 0,
    content: '[EcoAlert] {{event}}\n视频源: {{source_name}}\n区域: {{location}}\n有人: {{person}}\n亮灯: {{light}}\n时间: {{ts}}',
  }, null, 2),
};

const getDefaultNotifyBodyTemplate = (channelType) =>
  CHANNEL_DEFAULT_BODY_TEMPLATES[channelType] || DEFAULT_NOTIFY_BODY_TEMPLATE;

const isDefaultNotifyBodyTemplate = (value) =>
  Object.values(CHANNEL_DEFAULT_BODY_TEMPLATES).includes(String(value || '').trim());

const DEFAULT_DEV_NOTIFY_TARGET = {
  name: '企业内部通知（开发调试）',
  enabled: true,
  channelType: 'webhook',
  url: 'https://biz.hirain.com/synergy/notice/545B6B5FEF17',
  method: 'POST',
  headers: [
    { name: 'Content-Type', value: 'application/json' },
    { name: 'Cookie', value: 'JSESSIONID=1D6DEE38ECAC44223748CE0B062F8CC0' },
  ],
  bodyTemplate: JSON.stringify({
    touser: 'jinsheng.liu1',
    msgtype: 'text',
    agentcode: 'ai_challenge',
    text: { content: '[EcoAlert] {{event}}\n视频源: {{source_name}}\n区域: {{location}}\n有人: {{person}}\n亮灯: {{light}}\n时间: {{ts_formatted}}' },
  }, null, 2),
  eventTypes: ['alarm_triggered', 'alarm_resolved'],
  cooldownSec: 1800,
  timeoutSec: 10,
  retryCount: 2,
  appId: '',
  appSecret: '',
  agentId: '',
  chatId: '',
};

// API 模式各字段的 label 和 placeholder
const CHANNEL_API_CONFIG = {
  feishu: {
    appIdLabel: 'App ID *',
    appIdPlaceholder: '飞书开发者后台的 App ID（cli_ 开头）',
    secretLabel: 'App Secret *',
    secretPlaceholder: '飞书 App Secret',
    chatIdLabel: '接收群 Chat ID *',
    chatIdPlaceholder: 'oc_ 开头，可通过扫码绑定自动获取',
    chatIdHint: 'App 需有群消息权限；点右侧「扫码绑定」可获取机器人所在群 chat_id',
    showAgent: false,
    showOAuth: true,
  },
  wechat_work: {
    appIdLabel: 'Corp ID *',
    appIdPlaceholder: '企业微信管理后台的 CorpID',
    secretLabel: 'Secret *',
    secretPlaceholder: '应用 Secret',
    chatIdLabel: '接收人 UserID *',
    chatIdPlaceholder: '@all 或 user1|user2',
    chatIdHint: '企业微信应用消息使用 touser；多人用 | 分隔，@all 表示全员',
    showAgent: true,
    showOAuth: false,
  },
  qqbot: {
    appIdLabel: 'App ID *',
    appIdPlaceholder: 'QQ 开放平台的 AppID',
    secretLabel: 'Client Secret *',
    secretPlaceholder: 'QQ Bot ClientSecret',
    chatIdLabel: '接收目标',
    chatIdPlaceholder: '',
    chatIdHint: '',
    showAgent: false,
    showOAuth: false,
    showChatTarget: false,
  },
};

const getApiMode = () => {
  const radios = $$('input[name="ntf-mode"]');
  const checked = radios.find((r) => r.checked);
  return checked ? checked.value : 'webhook';
};

const toggleNotifyChannelFields = () => {
  const type = $('#ntf-channel').value;
  const isWebhookType = type === 'webhook';
  const modeSwitch = $('#ntf-mode-switch');
  const apiMode = getApiMode();
  const isApiMode = !isWebhookType && apiMode === 'api';

  // 模式切换栏：通用 Webhook 渠道不显示
  if (modeSwitch) modeSwitch.style.display = isWebhookType ? 'none' : '';

  // Webhook URL：通用 Webhook 或简单模式下显示
  const urlSection = $('#ntf-webhook-url-section');
  if (urlSection) urlSection.style.display = (isWebhookType || !isApiMode) ? '' : 'none';

  // API 凭证字段
  const apiFields = $('#ntf-api-fields');
  if (apiFields) apiFields.style.display = isApiMode ? '' : 'none';

  // 简单模式 / 通用 Webhook 字段（Method、Body 模板）
  const webhookFields = $('#ntf-webhook-fields');
  if (webhookFields) webhookFields.style.display = isApiMode ? 'none' : '';

  // URL hint / placeholder
  const hint = $('#ntf-url-hint');
  if (hint) hint.textContent = CHANNEL_URL_HINTS[type] || CHANNEL_URL_HINTS.webhook;
  const urlInput = $('#ntf-url');
  if (urlInput) {
    urlInput.placeholder = CHANNEL_URL_PLACEHOLDERS[type] || CHANNEL_URL_PLACEHOLDERS.webhook;
    urlInput.required = !isApiMode;
    urlInput.disabled = isApiMode;
  }
  const appIdInput = $('#ntf-app-id');
  const secretInput = $('#ntf-app-secret');
  const agentInput = $('#ntf-agent-id');
  const chatInput = $('#ntf-chat-id');
  if (appIdInput) {
    appIdInput.required = isApiMode;
    appIdInput.disabled = !isApiMode;
  }
  if (secretInput) {
    secretInput.required = isApiMode;
    secretInput.disabled = !isApiMode;
  }
  if (agentInput) {
    agentInput.required = isApiMode && type === 'wechat_work';
    agentInput.disabled = !isApiMode;
  }
  if (chatInput) {
    chatInput.required = isApiMode && type !== 'qqbot';
    chatInput.disabled = !isApiMode;
  }
  const bodyInput = $('#ntf-body');
  if (bodyInput && !isApiMode && (!bodyInput.value.trim() || isDefaultNotifyBodyTemplate(bodyInput.value))) {
    bodyInput.value = getDefaultNotifyBodyTemplate(type);
  }

  // API 字段配置
  if (isApiMode) {
    const cfg = CHANNEL_API_CONFIG[type];
    if (cfg) {
      const appIdLabel = $('#ntf-appid-label');
      if (appIdLabel) appIdLabel.textContent = cfg.appIdLabel;
      const appIdInput = $('#ntf-app-id');
      if (appIdInput) appIdInput.placeholder = cfg.appIdPlaceholder;
      const secretLabel = $('#ntf-secret-label');
      if (secretLabel) secretLabel.textContent = cfg.secretLabel;
      const secretInput = $('#ntf-app-secret');
      if (secretInput) secretInput.placeholder = cfg.secretPlaceholder;
      const agentRow = $('#ntf-agent-row');
      if (agentRow) agentRow.style.display = cfg.showAgent ? '' : 'none';
      const chatRow = $('#ntf-chat-row');
      if (chatRow) chatRow.style.display = cfg.showChatTarget === false ? 'none' : '';
      const chatIdLabel = $('#ntf-chatid-label');
      if (chatIdLabel) chatIdLabel.textContent = cfg.chatIdLabel;
      const chatIdInput = $('#ntf-chat-id');
      if (chatIdInput) chatIdInput.placeholder = cfg.chatIdPlaceholder;
      const chatIdHint = $('#ntf-chatid-hint');
      if (chatIdHint) chatIdHint.textContent = cfg.chatIdHint;
      const oauthButton = $('#btn-oauth-bind');
      if (oauthButton) oauthButton.style.display = cfg.showOAuth ? '' : 'none';
    }
  } else {
    const oauthButton = $('#btn-oauth-bind');
    if (oauthButton) oauthButton.style.display = 'none';
  }
};

const notifyPayloadFromForm = () => {
  const eventType = $('#ntf-event').value;
  const cooldown = Number.parseInt($('#ntf-cooldown').value, 10);
  const channelType = $('#ntf-channel').value;
  const isWebhookType = channelType === 'webhook';
  const apiMode = getApiMode();
  const isApi = !isWebhookType && apiMode === 'api';
  return {
    name: $('#ntf-name').value.trim(),
    enabled: $('#ntf-enabled').checked,
    channelType,
    url: isApi ? '' : $('#ntf-url').value.trim(),
    method: isApi ? 'POST' : ($('#ntf-method').value || 'POST'),
    headers: [{ name: 'Content-Type', value: 'application/json' }],
    bodyTemplate: isApi || (channelType !== 'webhook' && isDefaultNotifyBodyTemplate($('#ntf-body').value))
      ? ''
      : ($('#ntf-body').value.trim() || getDefaultNotifyBodyTemplate(channelType)),
    timeoutSec: 10,
    retryCount: 2,
    eventTypes: eventType ? [eventType] : [],
    cooldownSec: Number.isFinite(cooldown) && cooldown > 0 ? cooldown : 1800,
    // API 凭证
    appId: isApi ? ($('#ntf-app-id').value.trim()) : '',
    appSecret: isApi ? ($('#ntf-app-secret').value.trim()) : '',
    agentId: isApi ? ($('#ntf-agent-id').value.trim()) : '',
    chatId: isApi ? ($('#ntf-chat-id').value.trim()) : '',
  };
};

const clearNotifyForm = () => {
  $('#ntf-edit-id').value = '';
  $('#ntf-channel').value = 'webhook';
  $('#ntf-name').value = '';
  $('#ntf-url').value = '';
  $('#ntf-method').value = 'POST';
  $('#ntf-event').value = 'alarm_triggered';
  $('#ntf-cooldown').value = '1800';
  $('#ntf-body').value = getDefaultNotifyBodyTemplate('webhook');
  $('#ntf-enabled').checked = true;
  $('#ntf-app-id').value = '';
  $('#ntf-app-secret').value = '';
  $('#ntf-agent-id').value = '';
  $('#ntf-chat-id').value = '';
  // 重置模式为 webhook
  const webhookRadio = $('input[name="ntf-mode"][value="webhook"]');
  if (webhookRadio) webhookRadio.checked = true;
  toggleNotifyChannelFields();
};

const showNotifyForm = (mode = 'create') => {
  const form = $('#notify-form');
  if (form) form.classList.remove('hidden');
  $('#btn-add-notify')?.classList.add('hidden');
  const submitBtn = $('#notify-form button[type="submit"]');
  if (submitBtn) submitBtn.textContent = mode === 'edit' ? '保存修改' : '保存通知目标';
  setTimeout(() => $('#ntf-name')?.focus(), 0);
};

const hideNotifyForm = () => {
  const form = $('#notify-form');
  if (form) form.classList.add('hidden');
  $('#btn-add-notify')?.classList.remove('hidden');
  $('#verify-result').textContent = '';
};

const fillNotifyForm = (target) => {
  showNotifyForm('edit');
  $('#ntf-edit-id').value = target.id || '';
  const channelType = target.channelType || target.channel_type || 'webhook';
  $('#ntf-channel').value = channelType;
  $('#ntf-name').value = target.name || '';
  $('#ntf-url').value = target.url || '';
  $('#ntf-method').value = target.method || 'POST';
  const evts = target.eventTypes || target.event_types || [];
  $('#ntf-event').value = evts.length === 1 ? evts[0] : '';
  $('#ntf-cooldown').value = target.cooldownSec || target.cooldown_sec || 1800;
  $('#ntf-body').value = target.bodyTemplate || target.body_template || getDefaultNotifyBodyTemplate(channelType);
  $('#ntf-enabled').checked = target.enabled !== false;
  // API 凭证
  $('#ntf-app-id').value = target.appId || target.app_id || '';
  $('#ntf-app-secret').value = target.appSecret || target.app_secret || '';
  $('#ntf-agent-id').value = target.agentId || target.agent_id || '';
  $('#ntf-chat-id').value = target.chatId || target.chat_id || '';
  // 根据是否有凭证自动切模式
  const hasCredentials = !!(target.appId || target.app_id);
  const isWebhookType = channelType === 'webhook';
  const mode = (!isWebhookType && hasCredentials) ? 'api' : 'webhook';
  const modeRadio = $(`input[name="ntf-mode"][value="${mode}"]`);
  if (modeRadio) modeRadio.checked = true;
  toggleNotifyChannelFields();
};

const renderNotificationSettings = async () => {
  try {
    notificationTargets = await listNotificationTargets();
    notificationHistory = await listNotificationHistory({ limit: 50 });
  } catch (err) {
    addLog('warn', `通知配置加载失败: ${err.message || err}`);
  }
  renderNotificationTargets();
  renderNotificationHistory();
};

const renderNotificationTargets = () => {
  const tbody = $('#ntf-target-tbody');
  if (!notificationTargets.length) {
    tbody.innerHTML = '<tr><td colspan="7" class="muted center">暂无通知目标</td></tr>';
    return;
  }
  tbody.innerHTML = notificationTargets.map((target) => {
    const channelType = target.channelType || target.channel_type || 'webhook';
    const channelLabel = CHANNEL_LABELS[channelType] || channelType;
    const events = (target.eventTypes || []).length ? target.eventTypes.join(', ') : '全部';
    const endpoint = target.url || target.chatId || target.chat_id || target.appId || target.app_id || '-';
    return `
      <tr>
        <td><span class="badge badge-channel">${escapeHtml(channelLabel)}</span></td>
        <td>${escapeHtml(target.name)}</td>
        <td>${escapeHtml(events)}</td>
        <td>${target.cooldownSec || target.cooldown_sec || 0}s</td>
        <td>${target.enabled ? '是' : '否'}</td>
        <td title="${escapeHtml(endpoint)}">${escapeHtml(endpoint).slice(0, 48)}</td>
        <td>
          <button class="btn-ghost" data-edit-ntf="${target.id}">编辑</button>
          <button class="btn-ghost" data-test-ntf="${target.id}">测试</button>
          <button class="btn-ghost btn-danger" data-del-ntf="${target.id}">删除</button>
        </td>
      </tr>
    `;
  }).join('');
  $$('[data-edit-ntf]', tbody).forEach((btn) => {
    btn.addEventListener('click', () => {
      const t = notificationTargets.find((x) => x.id === btn.dataset.editNtf);
      if (t) {
        fillNotifyForm(t);
        $('#notify-form')?.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
    });
  });
  $$('[data-test-ntf]', tbody).forEach((btn) => {
    btn.addEventListener('click', () => testSavedNotification(btn.dataset.testNtf));
  });
  $$('[data-del-ntf]', tbody).forEach((btn) => {
    btn.addEventListener('click', () => removeNotificationTarget(btn.dataset.delNtf));
  });
};

const renderNotificationHistory = () => {
  const tbody = $('#ntf-history-tbody');
  $('#ntf-history-count').textContent = `${notificationHistory.length} 条`;
  if (!notificationHistory.length) {
    tbody.innerHTML = '<tr><td colspan="7" class="muted center">暂无通知历史</td></tr>';
    return;
  }
  tbody.innerHTML = notificationHistory.map((record) => {
    const source = record.sourceId ? (sources.find((item) => item.id === record.sourceId)?.name || record.sourceId) : '-';
    const result = record.ok ? `成功 ${record.statusCode || ''}` : `失败 ${escapeHtml(record.error || '')}`;
    return `
      <tr>
        <td>${fmtDate(record.requestAt)}</td>
        <td>${escapeHtml(record.targetName || record.targetId || '-')}</td>
        <td>${escapeHtml(record.event)}</td>
        <td>${escapeHtml(source)}</td>
        <td>${result}</td>
        <td>${record.latencyMs ?? '-'}ms</td>
        <td>${record.ok ? '-' : `<button class="btn-ghost" data-resend-ntf="${record.id}">重发</button>`}</td>
      </tr>
    `;
  }).join('');
  $$('[data-resend-ntf]', tbody).forEach((btn) => {
    btn.addEventListener('click', () => resendNotificationRecord(btn.dataset.resendNtf));
  });
};

$('#notify-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  try {
    const payload = notifyPayloadFromForm();
    const isApi = payload.channelType !== 'webhook' && getApiMode() === 'api';
    if (!payload.name) {
      alert('通知名称不能为空');
      return;
    }
    if (!isApi && !payload.url) {
      alert('Webhook URL 不能为空（或切换到 API 凭证模式填写凭证）');
      return;
    }
    if (isApi && (!payload.appId || !payload.appSecret)) {
      alert('API 模式下 App ID 和 Secret 不能为空');
      return;
    }
    if (isApi && payload.channelType !== 'qqbot' && !payload.chatId) {
      alert('API 模式下接收目标不能为空');
      return;
    }
    if (isApi && payload.channelType === 'wechat_work' && !payload.agentId) {
      alert('企业微信 API 模式下 Agent ID 不能为空');
      return;
    }
    const editId = $('#ntf-edit-id').value.trim();
    if (editId) {
      await updateNotificationTarget(editId, payload);
    } else {
      await createNotificationTarget(payload);
    }
    clearNotifyForm();
    hideNotifyForm();
    await renderNotificationSettings();
    addLog('info', editId ? '通知目标已更新' : '通知目标已创建');
  } catch (err) {
    alert(err.message || '保存通知目标失败');
  }
});

$('#btn-add-notify')?.addEventListener('click', () => {
  clearNotifyForm();
  showNotifyForm('create');
});
$('#btn-cancel-notify-form')?.addEventListener('click', () => {
  clearNotifyForm();
  hideNotifyForm();
});
$('#btn-refresh-notify').addEventListener('click', renderNotificationSettings);
$('#btn-refresh-ntf-history')?.addEventListener('click', renderNotificationHistory);
$('#ntf-channel').addEventListener('change', toggleNotifyChannelFields);

// 模式切换
$$('input[name="ntf-mode"]').forEach((radio) => {
  radio.addEventListener('change', toggleNotifyChannelFields);
});
toggleNotifyChannelFields();

// 验证凭证
$('#btn-verify-credentials')?.addEventListener('click', async () => {
  const result = $('#verify-result');
  const channelType = $('#ntf-channel').value;
  const appId = $('#ntf-app-id').value.trim();
  const appSecret = $('#ntf-app-secret').value.trim();
  if (!appId || !appSecret) {
    if (result) { result.textContent = '请填写 App ID 和 Secret'; result.className = 'verify-result err'; }
    return;
  }
  if (result) { result.textContent = '验证中…'; result.className = 'verify-result'; }
  try {
    const res = await verifyChannelCredentials(channelType, appId, appSecret);
    if (result) { result.textContent = res.message || '✓ 凭证有效'; result.className = 'verify-result ok'; }
  } catch (err) {
    if (result) { result.textContent = err.message || '凭证无效'; result.className = 'verify-result err'; }
  }
});

// OAuth 扫码绑定（飞书）
let oauthPollTimer = null;
$('#btn-oauth-bind')?.addEventListener('click', async () => {
  const appId = $('#ntf-app-id').value.trim();
  const appSecret = $('#ntf-app-secret').value.trim();
  if (!appId || !appSecret) {
    alert('请先填写 App ID 和 App Secret');
    return;
  }
  const modal = $('#oauth-modal');
  const qrContainer = $('#qr-container');
  const statusEl = $('#oauth-status');
  const chatSelectDiv = $('#oauth-chat-select');
  const confirmBtn = $('#btn-confirm-oauth');
  modal.classList.remove('hidden');
  chatSelectDiv.style.display = 'none';
  confirmBtn.disabled = true;
  statusEl.textContent = '正在启动授权服务…';
  statusEl.className = 'oauth-status';
  qrContainer.innerHTML = '<div class="qr-loading">正在生成二维码…</div>';

  try {
    const binding = await startOAuthBinding('feishu', appId, appSecret);
    const authUrl = binding.authUrl || binding.qrData;
    // 用 QR Server API 生成二维码图片（无需本地库）
    const qrImgUrl = `https://api.qrserver.com/v1/create-qr-code/?size=200x200&data=${encodeURIComponent(authUrl)}`;
    qrContainer.innerHTML = `<img src="${qrImgUrl}" alt="飞书授权二维码" />`;
    statusEl.textContent = '请用飞书 App 扫描二维码';

    // 轮询授权状态
    let oauthSessionId = binding.sessionId;
    let oauthDone = false;
    const poll = async () => {
      if (oauthDone) return;
      try {
        const status = await checkOAuthStatus(oauthSessionId, appId, appSecret);
        if (status.status === 'success') {
          oauthDone = true;
          statusEl.textContent = '✓ 授权成功！';
          statusEl.className = 'oauth-status success';
          // 显示群列表
          if (status.chats && status.chats.length > 0) {
            chatSelectDiv.style.display = '';
            const select = $('#oauth-chat-id');
            select.innerHTML = status.chats.map((c) =>
              `<option value="${escapeHtml(c.chat_id)}">${escapeHtml(c.name)}</option>`
            ).join('');
            confirmBtn.disabled = false;
          } else {
            statusEl.textContent = '✓ 授权成功，但未找到可发送的群聊（机器人需在群内）';
          }
        }
      } catch (e) { /* ignore poll errors */ }
      if (!oauthDone) oauthPollTimer = setTimeout(poll, 2000);
    };
    oauthPollTimer = setTimeout(poll, 2000);
  } catch (err) {
    statusEl.textContent = err.message || '启动授权失败';
    statusEl.className = 'oauth-status';
  }
});

// 确认 OAuth 绑定
$('#btn-confirm-oauth')?.addEventListener('click', () => {
  const chatId = $('#oauth-chat-id').value;
  if (chatId) {
    $('#ntf-chat-id').value = chatId;
  }
  $('#oauth-modal').classList.add('hidden');
  if (oauthPollTimer) clearTimeout(oauthPollTimer);
});

// 关闭 OAuth 弹窗
const closeOAuthModal = () => {
  $('#oauth-modal').classList.add('hidden');
  if (oauthPollTimer) clearTimeout(oauthPollTimer);
};
$('#btn-close-oauth')?.addEventListener('click', closeOAuthModal);
$('#btn-cancel-oauth')?.addEventListener('click', closeOAuthModal);

$('#btn-test-notify-form').addEventListener('click', async () => {
  try {
    const payload = notifyPayloadFromForm();
    const isApi = payload.channelType !== 'webhook' && getApiMode() === 'api';
    if (!payload.name) {
      alert('通知名称不能为空');
      return;
    }
    if (!isApi && !payload.url) {
      alert('Webhook URL 不能为空（或切换到 API 凭证模式）');
      return;
    }
    if (isApi && (!payload.appId || !payload.appSecret)) {
      alert('API 模式下 App ID 和 Secret 不能为空');
      return;
    }
    if (isApi && payload.channelType !== 'qqbot' && !payload.chatId) {
      alert('API 模式下接收目标不能为空');
      return;
    }
    if (isApi && payload.channelType === 'wechat_work' && !payload.agentId) {
      alert('企业微信 API 模式下 Agent ID 不能为空');
      return;
    }
    const record = await testNotificationTarget({ payload });
    await renderNotificationSettings();
    addLog(record.ok ? 'info' : 'warn', `通知测试${record.ok ? '成功' : '失败'}: ${record.targetName || payload.name}`);
  } catch (err) {
    alert(err.message || '测试通知失败');
  }
});

const testSavedNotification = async (id) => {
  try {
    const record = await testNotificationTarget({ id });
    await renderNotificationSettings();
    addLog(record.ok ? 'info' : 'warn', `通知测试${record.ok ? '成功' : '失败'}: ${record.targetName || id}`);
  } catch (err) {
    alert(err.message || '测试通知失败');
  }
};

const removeNotificationTarget = async (id) => {
  const target = notificationTargets.find((item) => item.id === id);
  if (!target) return;
  if (!confirm(`确定要删除通知目标「${target.name}」吗？`)) return;
  try {
    await deleteNotificationTarget(id);
    await renderNotificationSettings();
    addLog('warn', `已删除通知目标: ${target.name}`);
  } catch (err) {
    alert(err.message || '删除通知目标失败');
  }
};

const resendNotificationRecord = async (id) => {
  try {
    const record = await resendNotification(id);
    await renderNotificationSettings();
    addLog(record.ok ? 'info' : 'warn', `通知重发${record.ok ? '成功' : '失败'}: ${record.targetName || id}`);
  } catch (err) {
    alert(err.message || '通知重发失败');
  }
};

/* -------------------- 系统日志 -------------------- */
const logs = [];
const addLog = (level, text) => {
  const line = { level, text, ts: Date.now() };
  logs.push(line);
  if (logs.length > 1000) logs.shift();
  appendLogLine(line);
  $('#log-count').textContent = `${logs.length} 条`;
};
const appendLogLine = (line) => {
  const box = $('#console-box');
  const div = document.createElement('div');
  div.className = `log-line ${line.level}`;
  div.innerHTML =
    `<span class="ts">[${fmtTime(line.ts)}]</span>` +
    `<span class="lv">${line.level.toUpperCase()}</span>` +
    `<span class="tx">${escapeHtml(line.text)}</span>`;
  box.appendChild(div);
  if ($('#auto-scroll').checked) box.scrollTop = box.scrollHeight;
};
$('#btn-clear-log').addEventListener('click', () => {
  logs.length = 0;
  $('#console-box').innerHTML = '';
  $('#log-count').textContent = '0 条';
});

/* -------------------- 事件订阅 -------------------- */
const subscribeEvents = async () => {
  await onEvent((payload) => addLog(payload.level || 'info', payload.text));
  await onStatus((payload) => {
    stats = payload || [];
    updateLiveStats();
    if (!$('#view-overview').classList.contains('hidden')) renderOverview();
  });
  await onRuntimeStatus((payload) => {
    updateRuntimeStatuses(payload);
  });
  await onAlgorithmSchedule((payload) => {
    updateVlmStateFromSchedule(payload);
  });
  await onSceneState((payload) => {
    updateLiveState(payload);
    if (!$('#view-overview').classList.contains('hidden')) renderOverview();
    if (!$('#view-detection-history')?.classList.contains('hidden')) renderDetectionHistory();
  });
  await onAlarm(async (payload) => {
    addLog('warn', `报警事件: ${payload.event || payload.status || '-'}`);
    if (!$('#view-overview').classList.contains('hidden')) {
      await renderOverview();
    }
  });
  await onNotification(async (payload) => {
    addLog(payload.ok ? 'info' : 'warn', `通知${payload.ok ? '成功' : '失败'}: ${payload.event || '-'}`);
    if (!$('#view-notification-settings').classList.contains('hidden')) {
      await renderNotificationSettings();
    }
  });
  await onSources(async () => {
    await loadSources();
    await loadGroups();
    renderLive();
    renderSourcesTable();
    if (!$('#view-detection-history')?.classList.contains('hidden')) renderDetectionHistory();
  });
};

const updateLiveStats = () => {
  $$('.video-card').forEach((card) => {
    const id = card.dataset.id;
    const s = stats.find((x) => x.id === id);
    if (!s) return;
    const tag = card.querySelector('.live-tag');
    if (tag) {
      if (s.online) {
        tag.classList.remove('off');
        tag.innerHTML = '<span class="pulse"></span>LIVE';
      } else {
        tag.classList.add('off');
        tag.innerHTML = '<span class="pulse"></span>离线';
      }
    }
  });
};

/* -------------------- 启动 -------------------- */
(async () => {
  hydrateIcons();
  $('#ws-text').textContent = isTauriEnv ? 'Tauri IPC 就绪' : '浏览器预览模式';
  const dot = $('#ws-dot');
  dot.className = isTauriEnv ? 'dot ok' : 'dot';
  try {
    await checkAuth();
  } catch (_) {
    showLogin();
    return;
  }
  await enterApp();
})();
