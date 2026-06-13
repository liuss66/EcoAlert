/* ===========================================================
   EcoAlert 前端主逻辑（Tauri 版）
   - 登录 / 鉴权
   - 视图切换：实时监控 / 监控总览 / 视频管理 / 系统设置 / 日志
   - 视频播放（HLS / MP4 / 摄像头）
   - 事件订阅（替代 WebSocket）
   =========================================================== */
import Hls from 'hls.js';
import {
  login, logout, checkAuth,
  listSources, listGroups, createGroup, updateGroup, deleteGroup, reorder,
  createSource, updateSource, deleteSource,
  reportSceneState, getStateHistory,
  listAlarms, ackAlarm, resolveAlarm,
  getAlgorithmConfig, updateAlgorithmConfig, deleteAlgorithmConfig,
  getRoiConfig, updateRoiConfig, testRoiConfig,
  listNotificationTargets, createNotificationTarget, updateNotificationTarget, deleteNotificationTarget,
  listNotificationHistory, testNotificationTarget, resendNotification,
  changePassword, getDataDir,
  startOAuthBinding, checkOAuthStatus, verifyChannelCredentials,
  onEvent, onStatus, onRuntimeStatus, onSources, onSceneState, onAlarm, onNotification, isTauriEnv,
  openDevtools, probeUrl,
} from './api.js';

const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => Array.from(el.querySelectorAll(sel));

/* -------------------- 工具 -------------------- */
const escapeHtml = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[c]));

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
      developerMode = !!(globalAlgorithmConfig.developerMode ?? globalAlgorithmConfig.developer_mode);
    } catch (e) {
      console.warn('开发者模式配置读取失败:', e);
      developerMode = false;
    }
    renderLive();
    renderSourcesTable();
    switchView('live');
    await subscribeEvents();
    // 设置页数据
    try { $('#data-dir').textContent = await getDataDir(); } catch (e) { console.warn('获取数据目录失败:', e); }
    await renderSettings();
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
    const fmt = (v) => (Math.round(Number(v) * 100) / 100).toFixed(2);
    const update = () => (l.textContent = fmt(s.value));
    s.addEventListener('input', update);
    update();
  };
  syncSlider('#algo-person-threshold', '#algo-person-threshold-val');
  syncSlider('#algo-light-threshold', '#algo-light-threshold-val');

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
  const tabs = $$('#settings-tabs .settings-tab');
  if (tabs.length === 0) return;
  const panels = tabs
    .map((t) => document.getElementById(t.dataset.target))
    .filter(Boolean);
  const show = (targetId) => {
    tabs.forEach((t) => t.classList.toggle('active', t.dataset.target === targetId));
    panels.forEach((p) => p.classList.toggle('hidden', p.id !== targetId));
    // 切 tab 时把页面滚到顶，避免停留在上一个 panel 的中间
    const main = document.querySelector('.main');
    if (main) main.scrollTo?.({ top: 0, behavior: 'smooth' });
    window.scrollTo?.({ top: 0, behavior: 'smooth' });
  };
  tabs.forEach((tab) => {
    tab.addEventListener('click', (e) => {
      e.preventDefault();
      show(tab.dataset.target);
    });
  });
  // 初始化：默认显示第一个 tab 对应 panel
  const initial = tabs.find((t) => t.classList.contains('active')) || tabs[0];
  show(initial.dataset.target);
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
  settings: { title: '系统设置', sub: '修改登录密码 / 查看数据目录' },
  'notify-history': { title: '通知历史', sub: '每次通知发送的记录与结果' },
  console: { title: '系统日志', sub: '服务端与客户端事件流' },
};
const switchView = (name) => {
  if (!VIEW_META[name]) return;
  $$('.nav-item').forEach((b) => b.classList.toggle('active', b.dataset.view === name));
  $$('.view').forEach((v) => v.classList.toggle('hidden', v.id !== `view-${name}`));
  $('#view-title').textContent = VIEW_META[name].title;
  $('#view-sub').textContent = VIEW_META[name].sub;
  if (name === 'overview') renderOverview();
  if (name === 'settings') renderSettings();
  if (name === 'notify-history') renderNotificationHistory();
  if (name !== 'settings') destroyRoiVideo();
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
    <section class="group-section" data-group-id="${g.id}">
      <header class="group-header">
        <span class="caret ${g.collapsed ? 'collapsed' : ''}" data-toggle="${g.id}">▾</span>
        <div class="group-name">
          <span class="grp-label">${escapeHtml(g.name)}</span>
          <input class="grp-input hidden" data-grp-input="${g.id}" value="${escapeHtml(g.name)}" />
        </div>
        <span class="group-count">${list.length} 路</span>
        <div class="group-actions">
          ${g.id === 'grp-default' ? '' : `<button data-rename="${g.id}" title="重命名">✏️</button>`}
          ${g.id === 'grp-default' ? '' : `<button data-delgrp="${g.id}" title="删除分组">🗑</button>`}
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
      { id: '__nogroup', name: '其他', collapsed: false, order: 9999 },
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
let dragSourceId = null;
function bindDragAndDrop() {
  // 视频卡：可拖
  $$('.video-card[draggable="true"]').forEach((card) => {
    // 阻止 video 元素自己被拖
    card.querySelectorAll('video').forEach((v) => {
      v.setAttribute('draggable', 'false');
    });
    card.addEventListener('dragstart', (e) => {
      // 排除从按钮等交互控件触发的拖拽（让 click 正常生效）
      if (e.target.closest('button, input, select, textarea, a[href]')) {
        e.preventDefault();
        return;
      }
      dragSourceId = card.dataset.id;
      card.classList.add('dragging');
      // 标记来源分组
      const srcSection = card.closest('.group-section');
      if (srcSection) srcSection.classList.add('drag-source');
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', card.dataset.id);
    });
    card.addEventListener('dragend', () => {
      card.classList.remove('dragging');
      dragSourceId = null;
      $$('.group-section').forEach((g) => {
        g.classList.remove('drag-over', 'drag-source');
        const body = g.querySelector('.group-body');
        if (body) body.classList.remove('drag-active');
      });
    });
  });
  // 分组容器：接收
  $$('[data-dropzone]').forEach((zone) => {
    zone.addEventListener('dragover', (e) => {
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      zone.classList.add('drag-active');
      const section = zone.closest('.group-section');
      if (section) section.classList.add('drag-over');
    });
    zone.addEventListener('dragleave', () => {
      zone.classList.remove('drag-active');
      const section = zone.closest('.group-section');
      if (section) section.classList.remove('drag-over');
    });
    zone.addEventListener('drop', async (e) => {
      e.preventDefault();
      zone.classList.remove('drag-active');
      const section = zone.closest('.group-section');
      if (section) section.classList.remove('drag-over');
      const id = e.dataTransfer.getData('text/plain') || dragSourceId;
      const targetGroupId = zone.dataset.dropzone;
      if (!id) return;
      await moveSourceToGroup(id, targetGroupId);
    });
  });
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
    const updated = await updateGroup(groupId, { name: g.name, order: g.order, collapsed: !g.collapsed });
    g.collapsed = updated.collapsed;
    renderLive();
  } catch (err) {
    addLog('error', `折叠状态切换失败: ${err.message}`);
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
    const updated = await updateGroup(groupId, { name: newName, order: g.order, collapsed: g.collapsed });
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
    const grp = await createGroup({ name, order, collapsed: false });
    groups.push(grp);
    renderLive();
    addLog('info', `新增分组: ${grp.name}`);
  } catch (err) {
    addLog('error', `新增分组失败: ${err.message}`);
  }
}

/* -------------------- 状态图标（人/灯/报警）实时更新 -------------------- */
function applyStateIcons() {
  $$('.state-icons').forEach((el) => {
    const id = el.dataset.state;
    const s = sceneStates.get(id) || { person: false, light: false };
    const person = el.querySelector('.person');
    const light = el.querySelector('.light');
    const alarm = el.querySelector('.alarm');
    if (person) {
      person.style.display = s.person ? '' : 'none';
      person.title = s.person
        ? `人：在场 (置信度 ${(s.personConfidence * 100).toFixed(0)}%)`
        : '人：不在';
    }
    if (light) {
      light.style.display = s.light ? '' : 'none';
      light.title = s.light
        ? `灯：亮 (置信度 ${(s.lightConfidence * 100).toFixed(0)}%)`
        : '灯：关';
    }
    if (alarm) {
      const isAlarm = !!s.alarm;
      alarm.style.display = isAlarm ? '' : 'none';
      alarm.title = isAlarm ? '⚠️ 报警：无人 + 亮灯' : (s.alarmStatus === 'suspected' ? '疑似：等待保持时间' : '正常');
    }
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
          el.textContent = `检测：未运行 (${err || 'disabled'})`;
          el.title = '算法被关闭、通道停用或当前不在启用时段';
        } else if (status === 'error') {
          el.textContent = `检测：抽帧失败`;
          el.title = err || '请检查 ffmpeg、HLS URL 和推流器';
        } else if (last) {
          el.textContent = `检测：等待下一次结果`;
          el.title = `上次算法时间 ${fmtTime(last)}`;
        } else {
          el.textContent = `检测：${status === 'running' ? '抽帧中' : '等待首次结果'}`;
          el.title = err || 'Tauri 后端完成首次抽帧检测后显示';
        }
      } else {
        el.textContent = '检测：等待后端状态';
        el.title = '等待 runtime_status 或 scene_state 事件';
      }
      return;
    }
    const personText = s.person ? `有人 ${(s.personConfidence * 100).toFixed(0)}%` : `无人 ${(s.personConfidence * 100).toFixed(0)}%`;
    const lightText = s.light ? `亮灯 ${(s.lightConfidence * 100).toFixed(0)}%` : `关灯 ${(s.lightConfidence * 100).toFixed(0)}%`;
    const brightness = s.lightBrightness == null ? '-' : Number(s.lightBrightness).toFixed(0);
    const color = s.colorScore == null ? '-' : Number(s.colorScore).toFixed(3);
    const motion = s.motionScore == null ? '-' : Number(s.motionScore).toFixed(3);
    const cost = s.processMs ?? s.modelLatencyMs;
    const costText = cost == null ? '-' : `${Number(cost).toFixed(1)}ms`;
    el.textContent = `${personText} · ${lightText} · 色彩 ${color} · 运动 ${motion}`;
    el.dataset.brightness = brightness;
    el.title = `来源 ${s.source || 'simple'} / ${s.reason || '-'} / #${s.frameSeq || 0} / ${costText} / ${fmtTime(s.ts)}`;
  });
}

function updateLiveState(payload) {
  if (!payload || !payload.sourceId) return;
  sceneStates.set(payload.sourceId, {
    person: !!payload.person,
    light: !!payload.light,
    alarm: !!payload.alarm,
    alarmStatus: payload.alarmStatus || payload.alarm_status || 'normal',
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
  });
  applyStateIcons();
  updateAlarmBanner();
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
      <span class="icon">✅</span>
      <div>所有通道状态正常</div>
    `;
  } else {
    banner.classList.add('alarm');
    banner.classList.remove('ok');
    // 只展示前 3 个通道名 + "等 N 路"
    const shown = alarming.slice(0, 3).map((s) => `<b>${escapeHtml(s.name)}</b>`).join('、');
    const more = alarming.length > 3 ? ` 等 <b>${alarming.length}</b> 路` : '';
    banner.innerHTML = `
      <span class="icon">🚨</span>
      <div>当前 <b>${alarming.length}</b> 路报警：${shown}${more}</div>
    `;
  }
}

const videoCardHtml = (s) => {
  const st = stats.find((x) => x.id === s.id) || {};
  const scene = sceneStates.get(s.id) || { person: false, light: false };
  const alarm = !!scene.alarm;
  return `
    <div class="video-card" draggable="true" data-id="${s.id}" data-group-id="${s.groupId || 'grp-default'}">
      <div class="video-wrap" id="vw-${s.id}">
        <div class="placeholder"><div class="illu">📡</div><div>正在加载视频…</div></div>
        <div class="live-tag ${st.online ? '' : 'off'}">
          <span class="pulse"></span>${st.online ? 'LIVE' : '离线'}
        </div>
      </div>
      <div class="card-info">
        <div class="card-row card-row-top">
          <span class="card-name" title="${escapeHtml(s.name)}">${escapeHtml(s.name)}</span>
          <span class="state-icons" data-state="${s.id}">
            <span class="state-icon person" title="人 ${scene.person ? '在场' : '不在'}" style="${scene.person ? '' : 'display:none'}">🧍</span>
            <span class="state-icon light" title="灯 ${scene.light ? '亮' : '关'}" style="${scene.light ? '' : 'display:none'}">💡</span>
            <span class="state-icon alarm" title="${alarm ? '⚠️ 报警：无人但亮灯' : '正常'}" style="${alarm ? '' : 'display:none'}">🚨</span>
          </span>
        </div>
        <div class="card-row card-row-bottom">
          <span class="card-loc" title="${escapeHtml(s.location || '')}">${escapeHtml(s.location || '—')}</span>
          <span class="card-actions">
            <button class="ico-btn btn-edit" data-id="${s.id}" title="编辑">✏️</button>
            <button class="ico-btn btn-del" data-id="${s.id}" title="删除">🗑</button>
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
  video.playsInline = true;
  video.autoplay = true;
  const onError = () => {
    const ph = wrap.querySelector('.placeholder');
    if (ph) ph.remove();
    const e = document.createElement('div');
    e.className = 'placeholder';
    e.innerHTML = '<div class="illu">⚠️</div><div>视频加载失败</div>';
    wrap.appendChild(e);
  };
  video.addEventListener('error', onError);

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
        hls.on(Hls.Events.ERROR, (_event, data) => {
          if (!data.fatal) return;
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
            hls.startLoad();
          } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
            hls.recoverMediaError();
          } else {
            hls.destroy();
            onError();
          }
        });
      } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
        video.src = src.url;
      } else {
        throw new Error('浏览器不支持 HLS');
      }
    } else if (src.type === 'mp4') {
      video.src = src.url;
    } else if (src.type === 'webcam') {
      navigator.mediaDevices.getUserMedia({ video: true, audio: false })
        .then((stream) => { video.srcObject = stream; })
        .catch(() => onError());
    } else if (src.type === 'rtsp') {
      const ph = wrap.querySelector('.placeholder');
      if (ph) ph.remove();
      const e = document.createElement('div');
      e.className = 'placeholder';
      e.innerHTML = '<div class="illu">📡</div><div>RTSP 需服务端转码</div>';
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
  const ph = wrap.querySelector('.placeholder');
  if (ph && src.type !== 'webcam' && src.type !== 'rtsp') ph.remove();
  wrap.insertBefore(video, wrap.firstChild);
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
      <span class="icon">🚨</span>
      <div>当前 <b>${alarming.length}</b> 路报警：${shown}${more}</div>
    `;
  } else {
    banner.classList.add('ok');
    banner.classList.remove('alarm');
    banner.innerHTML = `<span class="icon">✅</span><div>所有通道状态正常</div>`;
  }

  const tb = $('#ov-tbody');
  if (sources.length === 0) {
    tb.innerHTML = `<tr><td colspan="10" class="muted center">暂无数据</td></tr>`;
  } else {
    tb.innerHTML = sources.map((s) => {
      const st = stats.find((x) => x.id === s.id) || {};
      const sc = sceneStates.get(s.id) || { person: false, light: false };
      const alarm = !!sc.alarm;
      const personIcon = `<span style="color:${sc.person ? '#10b981' : '#94a3b8'};">${sc.person ? '🟢' : '⚪'}</span>`;
      const lightIcon = `<span style="color:${sc.light ? '#10b981' : '#94a3b8'};">${sc.light ? '🟢' : '⚪'}</span>`;
      const alarmIcon = alarm
        ? '<span class="status-pill" style="background:#fee2e2;color:#991b1b;">🚨 报警</span>'
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
          <td>${r.person ? '🟢 在' : '⚪ 不在'}</td>
          <td>${r.light ? '🟢 亮' : '⚪ 关'}</td>
          <td>${r.alarm ? '<span class="status-pill" style="background:#fee2e2;color:#991b1b;">🚨</span>' : '<span class="muted">—</span>'}</td>
        </tr>
      `).join('');
    }
  } catch (e) {
    histTb.innerHTML = `<tr><td colspan="5" class="muted center">加载失败: ${escapeHtml(e.message)}</td></tr>`;
  }
};

const alarmStatusText = (status) => ({
  suspected: '疑似',
  alarm_active: '报警中',
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

/* -------------------- 系统设置 / 改密码 -------------------- */
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

const fillAlgorithmForm = (cfg) => {
  const win = (cfg.activeWindows || [])[0] || { weekdays: [1, 2, 3, 4, 5], start: '18:30', end: '08:30', timezone: 'Local' };
  $('#algo-enabled').checked = !!cfg.enabled;
  $('#algo-developer-mode').checked = !!(cfg.developerMode ?? cfg.developer_mode);
  $('#algo-weekdays').value = (win.weekdays || [1, 2, 3, 4, 5]).join(',');
  $('#algo-start').value = win.start || '18:30';
  $('#algo-end').value = win.end || '08:30';
  $('#algo-simple-interval').value = cfg.simpleIntervalSec ?? 10;
  $('#algo-vlm-interval').value = cfg.vlmIntervalSec ?? 300;
  $('#algo-person-threshold').value = cfg.personThreshold ?? 0.65;
  $('#algo-light-threshold').value = cfg.lightThreshold ?? 0.70;
  $('#algo-hold-sec').value = cfg.alarmHoldSec ?? 300;
  $('#algo-recover-sec').value = cfg.alarmRecoverSec ?? 60;
  $('#algo-recover-policy').value = cfg.recoverPolicy || 'either';
  $('#algo-vlm-limit').value = cfg.vlmHourlyLimit ?? 12;
  $('#algo-vlm-enabled').checked = !!cfg.vlmEnabled;
  $('#algo-vlm-skip-person').checked = cfg.vlmSkipWhenPerson !== false;
  const selectedSourceId = getSelectedAlgorithmSourceId();
  const selectedSource = selectedSourceId ? sources.find((s) => s.id === selectedSourceId) : null;
  $('#algo-scope').textContent = selectedSource
    ? (cfg.scope === 'source' ? `通道配置 · ${selectedSource.name}` : `继承全局 · ${selectedSource.name}`)
    : '全局配置';
  const resetBtn = $('#btn-reset-algorithm-source');
  if (resetBtn) resetBtn.style.display = selectedSourceId ? '' : 'none';
  // 同步 chip 选中态 + 刷新 slider 数字显示
  syncChipsFromHidden();
  $('#algo-person-threshold')?.dispatchEvent(new Event('input'));
  $('#algo-light-threshold')?.dispatchEvent(new Event('input'));
};

const getSelectedAlgorithmSourceId = () => {
  const value = $('#algo-source')?.value || '';
  return value === '__global__' ? null : value;
};

const populateAlgorithmSourceOptions = () => {
  const sel = $('#algo-source');
  if (!sel) return;
  const current = sel.value || '__global__';
  sel.innerHTML = [
    '<option value="__global__">全局默认配置</option>',
    ...sources.map((source) => `<option value="${source.id}">${escapeHtml(source.name)} · ${escapeHtml(source.location || '')}</option>`),
  ].join('');
  sel.value = [...sel.options].some((option) => option.value === current) ? current : '__global__';
};

const algorithmPayloadFromForm = () => {
  const weekdays = parseWeekdays($('#algo-weekdays').value);
  const sourceId = getSelectedAlgorithmSourceId();
  return {
    ...(algorithmConfig || {}),
    enabled: $('#algo-enabled').checked,
    developerMode: $('#algo-developer-mode').checked,
    scope: sourceId ? 'source' : 'global',
    scopeId: sourceId,
    activeWindows: [{
      weekdays: weekdays.length ? weekdays : [1, 2, 3, 4, 5],
      start: normalizeTimeText($('#algo-start').value, '18:30'),
      end: normalizeTimeText($('#algo-end').value, '08:30'),
      timezone: 'Local',
    }],
    exceptionWindows: algorithmConfig?.exceptionWindows || [],
    simpleIntervalSec: toInt($('#algo-simple-interval').value, 10, 1),
    vlmIntervalSec: toInt($('#algo-vlm-interval').value, 300, 30),
    vlmEnabled: $('#algo-vlm-enabled').checked,
    vlmSkipWhenPerson: $('#algo-vlm-skip-person').checked,
    personThreshold: toFloat($('#algo-person-threshold').value, 0.65),
    lightThreshold: toFloat($('#algo-light-threshold').value, 0.70),
    alarmHoldSec: toInt($('#algo-hold-sec').value, 300, 0),
    alarmRecoverSec: toInt($('#algo-recover-sec').value, 60, 0),
    recoverPolicy: $('#algo-recover-policy').value,
    vlmHourlyLimit: toInt($('#algo-vlm-limit').value, 12, 0),
    roiVersion: algorithmConfig?.roiVersion ?? null,
  };
};

const renderAlgorithmSettings = async () => {
  try {
    populateAlgorithmSourceOptions();
    const sourceId = getSelectedAlgorithmSourceId();
    algorithmConfig = await getAlgorithmConfig(sourceId);
    if (!sourceId) {
      developerMode = !!(algorithmConfig.developerMode ?? algorithmConfig.developer_mode);
      applyStateIcons();
    }
    fillAlgorithmForm(algorithmConfig);
  } catch (err) {
    addLog('warn', `算法配置加载失败: ${err.message || err}`);
  }
};

const renderSettings = async () => {
  await renderAlgorithmSettings();
  await renderRoiSettings();
  await renderNotificationSettings();
};

$('#algorithm-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  try {
    const sourceId = getSelectedAlgorithmSourceId();
    const payload = algorithmPayloadFromForm();
    algorithmConfig = await updateAlgorithmConfig(sourceId, payload);
    if (!sourceId) {
      developerMode = !!(algorithmConfig.developerMode ?? algorithmConfig.developer_mode);
      renderLive();
    }
    fillAlgorithmForm(algorithmConfig);
    addLog('info', sourceId ? '通道算法配置已保存' : '全局算法配置已保存');
  } catch (err) {
    alert(err.message || '保存算法配置失败');
  }
});

$('#btn-reload-algorithm').addEventListener('click', renderAlgorithmSettings);
$('#algo-source')?.addEventListener('change', renderAlgorithmSettings);
$('#btn-reset-algorithm-source')?.addEventListener('click', async () => {
  const sourceId = getSelectedAlgorithmSourceId();
  if (!sourceId) return;
  try {
    await deleteAlgorithmConfig(sourceId);
    algorithmConfig = await getAlgorithmConfig(sourceId);
    fillAlgorithmForm(algorithmConfig);
    addLog('info', '已恢复为全局算法配置');
  } catch (err) {
    alert(err.message || '恢复全局继承失败');
  }
});

/* -------------------- ROI 标定 -------------------- */
const clamp01 = (value, fallback = 0) => {
  const n = Number.parseFloat(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(1, Math.max(0, n));
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

  const showPlaceholder = () => {
    preview?.classList.remove('has-video');
  };

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
        hls.on(Hls.Events.ERROR, (_event, data) => {
          if (!data.fatal) return;
          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
            hls.startLoad();
          } else if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
            hls.recoverMediaError();
          } else {
            hls.destroy();
            roiHls = null;
            showPlaceholder();
          }
        });
        roiHls = hls;
      } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
        video.src = src.url;
      } else {
        showPlaceholder();
        return;
      }
    } else if (src.type === 'mp4') {
      video.src = src.url;
    } else if (src.type === 'webcam') {
      navigator.mediaDevices.getUserMedia({ video: true, audio: false })
        .then((stream) => { video.srcObject = stream; })
        .catch(() => showPlaceholder());
    } else {
      showPlaceholder();
      return;
    }
  } catch (_) {
    showPlaceholder();
    return;
  }

  video.addEventListener('playing', () => {
    preview?.classList.add('has-video');
  }, { once: true });

  wrap.insertBefore(video, wrap.firstChild);
};

const populateRoiSourceOptions = () => {
  const sel = $('#roi-source');
  if (!sel) return;
  const current = sel.value;
  sel.innerHTML = sources.length
    ? sources.map((s) => `<option value="${s.id}">${escapeHtml(s.name)} · ${escapeHtml(s.location || '未填写位置')}</option>`).join('')
    : '<option value="">暂无视频源</option>';
  if (current && sources.some((s) => s.id === current)) sel.value = current;
};

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
  const roi = (cfg.lightRois || cfg.light_rois || [])[0] || { x: 0, y: 0, w: 1, h: 1 };
  $('#roi-x').value = roi.x ?? 0;
  $('#roi-y').value = roi.y ?? 0;
  $('#roi-w').value = roi.w ?? 1;
  $('#roi-h').value = roi.h ?? 1;
  $('#roi-light-on').value = cfg.lightOnThreshold ?? cfg.light_on_threshold ?? 0.70;
  $('#roi-light-off').value = cfg.lightOffThreshold ?? cfg.light_off_threshold ?? 0.45;
  updateRoiPreview();
};

const roiPayloadFromForm = () => {
  const sourceId = $('#roi-source').value;
  const x = clamp01($('#roi-x').value, 0);
  const y = clamp01($('#roi-y').value, 0);
  const w = Math.max(0.01, Math.min(1 - x, clamp01($('#roi-w').value, 1)));
  const h = Math.max(0.01, Math.min(1 - y, clamp01($('#roi-h').value, 1)));
  const on = clamp01($('#roi-light-on').value, 0.70);
  const off = Math.min(on, clamp01($('#roi-light-off').value, 0.45));
  return {
    ...(roiConfig || {}),
    sourceId,
    version: roiConfig?.version || `roi-${Date.now()}`,
    lightRois: [{ id: 'light-main', label: '主灯光区域', x, y, w, h }],
    excludeRois: roiConfig?.excludeRois || [],
    personRois: roiConfig?.personRois || [],
    lightOnThreshold: on,
    lightOffThreshold: off,
    updatedAt: Date.now(),
  };
};

const loadSelectedRoi = async () => {
  const sourceId = $('#roi-source')?.value;
  if (!sourceId) return;
  const src = sources.find((s) => s.id === sourceId);
  if (src) mountRoiVideo(src);
  try {
    roiConfig = await getRoiConfig(sourceId);
    fillRoiForm(roiConfig);
  } catch (err) {
    addLog('warn', `ROI 配置加载失败: ${err.message || err}`);
  }
};

const renderRoiSettings = async () => {
  populateRoiSourceOptions();
  await loadSelectedRoi();
};

$('#roi-source')?.addEventListener('change', loadSelectedRoi);
['#roi-x', '#roi-y', '#roi-w', '#roi-h'].forEach((id) => {
  $(id)?.addEventListener('input', updateRoiPreview);
});
$('#btn-reload-roi')?.addEventListener('click', loadSelectedRoi);
$('#btn-test-roi')?.addEventListener('click', async () => {
  const sourceId = $('#roi-source').value;
  if (!sourceId) {
    alert('请先选择视频源');
    return;
  }
  try {
    const result = await testRoiConfig(sourceId, roiPayloadFromForm());
    const el = $('#roi-test-result');
    if (el) {
      el.classList.toggle('success', !!result.light);
      el.textContent = `测试结果：${result.light ? '灯亮' : '灯灭'}，色彩 ${Number(result.colorScore || result.color_score || 0).toFixed(3)}，亮度 ${Number(result.brightness || 0).toFixed(1)}，置信度 ${Number(result.confidence || 0).toFixed(2)}，耗时 ${Number(result.processMs || result.process_ms || 0).toFixed(2)}ms`;
    }
    addLog('info', 'ROI 测试完成');
  } catch (err) {
    alert(err.message || 'ROI 测试失败');
  }
});
$('#roi-form')?.addEventListener('submit', async (e) => {
  e.preventDefault();
  const sourceId = $('#roi-source').value;
  if (!sourceId) {
    alert('请先选择视频源');
    return;
  }
  try {
    const payload = roiPayloadFromForm();
    roiConfig = await updateRoiConfig(sourceId, payload);
    fillRoiForm(roiConfig);
    addLog('info', 'ROI 配置已保存');
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

// API 模式各字段的 label 和 placeholder
const CHANNEL_API_CONFIG = {
  feishu: {
    appIdLabel: 'App ID *',
    appIdPlaceholder: '飞书开发者后台的 App ID（cli_ 开头）',
    secretLabel: 'App Secret *',
    secretPlaceholder: '飞书 App Secret',
    chatIdLabel: '接收群 Chat ID *',
    chatIdPlaceholder: 'oc_ 开头，可通过扫码绑定自动获取',
    chatIdHint: '点右侧「扫码绑定」自动获取群列表',
    showAgent: false,
    showOAuth: true,
  },
  wechat_work: {
    appIdLabel: 'Corp ID *',
    appIdPlaceholder: '企业微信管理后台的 CorpID',
    secretLabel: 'Secret *',
    secretPlaceholder: '应用 Secret',
    chatIdLabel: '接收人 / 部门 *',
    chatIdPlaceholder: 'user1|user2 或 @all',
    chatIdHint: '多人用 | 分隔，@all 表示全员',
    showAgent: true,
    showOAuth: false,
  },
  qqbot: {
    appIdLabel: 'App ID *',
    appIdPlaceholder: 'QQ 开放平台的 AppID',
    secretLabel: 'Client Secret *',
    secretPlaceholder: 'QQ Bot ClientSecret',
    chatIdLabel: '群 OpenID *',
    chatIdPlaceholder: '群机器人的 group_openid',
    chatIdHint: '需手动填写',
    showAgent: false,
    showOAuth: false,
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

  // Webhook 专属字段（Method、Body 模板）
  const webhookFields = $('#ntf-webhook-fields');
  if (webhookFields) webhookFields.style.display = isWebhookType ? '' : 'none';

  // URL hint / placeholder
  const hint = $('#ntf-url-hint');
  if (hint) hint.textContent = CHANNEL_URL_HINTS[type] || CHANNEL_URL_HINTS.webhook;
  const urlInput = $('#ntf-url');
  if (urlInput) urlInput.placeholder = CHANNEL_URL_PLACEHOLDERS[type] || CHANNEL_URL_PLACEHOLDERS.webhook;

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
      const chatIdLabel = $('#ntf-chatid-label');
      if (chatIdLabel) chatIdLabel.textContent = cfg.chatIdLabel;
      const chatIdInput = $('#ntf-chat-id');
      if (chatIdInput) chatIdInput.placeholder = cfg.chatIdPlaceholder;
      const chatIdHint = $('#ntf-chatid-hint');
      if (chatIdHint) chatIdHint.textContent = cfg.chatIdHint;
      const oauthSection = $('#ntf-oauth-section');
      if (oauthSection) oauthSection.style.display = cfg.showOAuth ? '' : 'none';
    }
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
    method: isWebhookType ? ($('#ntf-method').value || 'POST') : 'POST',
    headers: [{ name: 'Content-Type', value: 'application/json' }],
    bodyTemplate: isWebhookType ? ($('#ntf-body').value.trim()) : '',
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
  $('#ntf-body').value = '';
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

const fillNotifyForm = (target) => {
  $('#ntf-edit-id').value = target.id || '';
  const channelType = target.channelType || target.channel_type || 'webhook';
  $('#ntf-channel').value = channelType;
  $('#ntf-name').value = target.name || '';
  $('#ntf-url').value = target.url || '';
  $('#ntf-method').value = target.method || 'POST';
  const evts = target.eventTypes || target.event_types || [];
  $('#ntf-event').value = evts.length === 1 ? evts[0] : '';
  $('#ntf-cooldown').value = target.cooldownSec || target.cooldown_sec || 1800;
  $('#ntf-body').value = target.bodyTemplate || target.body_template || '';
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
    return `
      <tr>
        <td><span class="badge badge-channel">${escapeHtml(channelLabel)}</span></td>
        <td>${escapeHtml(target.name)}</td>
        <td>${escapeHtml(events)}</td>
        <td>${target.cooldownSec || target.cooldown_sec || 0}s</td>
        <td>${target.enabled ? '是' : '否'}</td>
        <td title="${escapeHtml(target.url)}">${escapeHtml(target.url).slice(0, 48)}</td>
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
      if (t) fillNotifyForm(t);
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
    const isApi = !!(payload.appId && payload.appSecret);
    if (!payload.name) {
      alert('通知名称不能为空');
      return;
    }
    if (!isApi && !payload.url) {
      alert('Webhook URL 不能为空（或切换到 API 凭证模式填写凭证）');
      return;
    }
    if (isApi && !payload.chatId) {
      alert('API 模式下接收目标不能为空');
      return;
    }
    const editId = $('#ntf-edit-id').value.trim();
    if (editId) {
      await updateNotificationTarget(editId, payload);
    } else {
      await createNotificationTarget(payload);
    }
    clearNotifyForm();
    await renderNotificationSettings();
    addLog('info', editId ? '通知目标已更新' : '通知目标已创建');
  } catch (err) {
    alert(err.message || '保存通知目标失败');
  }
});

$('#btn-refresh-notify').addEventListener('click', renderNotificationSettings);
$('#btn-refresh-ntf-history')?.addEventListener('click', renderNotificationHistory);
$('#ntf-channel').addEventListener('change', toggleNotifyChannelFields);

// 模式切换
$$('input[name="ntf-mode"]').forEach((radio) => {
  radio.addEventListener('change', toggleNotifyChannelFields);
});

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
    const isApi = !!(payload.appId && payload.appSecret);
    if (!payload.name) {
      alert('通知名称不能为空');
      return;
    }
    if (!isApi && !payload.url) {
      alert('Webhook URL 不能为空（或切换到 API 凭证模式）');
      return;
    }
    if (isApi && !payload.chatId) {
      alert('API 模式下接收目标不能为空');
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
  await onSceneState((payload) => {
    updateLiveState(payload);
    if (!$('#view-overview').classList.contains('hidden')) renderOverview();
  });
  await onAlarm(async (payload) => {
    addLog('warn', `报警事件: ${payload.event || payload.status || '-'}`);
    if (!$('#view-overview').classList.contains('hidden')) {
      await renderOverview();
    }
  });
  await onNotification(async (payload) => {
    addLog(payload.ok ? 'info' : 'warn', `通知${payload.ok ? '成功' : '失败'}: ${payload.event || '-'}`);
    if (!$('#view-settings').classList.contains('hidden')) {
      await renderNotificationSettings();
    }
  });
  await onSources(async () => {
    await loadSources();
    await loadGroups();
    renderLive();
    renderSourcesTable();
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
