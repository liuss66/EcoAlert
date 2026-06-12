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
  getAlgorithmConfig, updateAlgorithmConfig,
  listNotificationTargets, createNotificationTarget, deleteNotificationTarget,
  listNotificationHistory, testNotificationTarget, resendNotification,
  changePassword, getDataDir,
  onEvent, onStatus, onSources, onSceneState, onAlarm, onNotification, isTauriEnv,
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
  try { await logout(); } catch (_) {}
  showLogin();
});

const enterApp = async () => {
  showApp();
  startClock();
  await loadSources();
  await loadGroups();
  renderLive();
  renderSourcesTable();
  switchView('live');
  await subscribeEvents();
  // 设置页数据
  try { $('#data-dir').textContent = await getDataDir(); } catch (_) {}
  await renderSettings();
  setupSettingsEnhancements();
  setupSettingsTabs();
  if (!isTauriEnv) addLog('warn', '当前在浏览器预览模式（未连接 Tauri 后端）');
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
}

let syncChipsFromHidden = () => {};

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
};
$$('.nav-item').forEach((b) => b.addEventListener('click', () => switchView(b.dataset.view)));

/* -------------------- 数据 -------------------- */
let sources = [];
let groups = [];
let stats = [];
let alarms = [];
let algorithmConfig = null;
let notificationTargets = [];
let notificationHistory = [];
/** 实时状态：sourceId -> { person, light, alarm, ts } */
let sceneStates = new Map();

const loadSources = async () => {
  try { sources = await listSources(); }
  catch (e) { sources = []; }
};
const loadGroups = async () => {
  try { groups = await listGroups(); }
  catch (e) { groups = []; }
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
      person.title = s.person ? '人：在场' : '人：不在';
    }
    if (light) {
      light.style.display = s.light ? '' : 'none';
      light.title = s.light ? '灯：亮' : '灯：关';
    }
    if (alarm) {
      const isAlarm = !s.person && s.light;
      alarm.style.display = isAlarm ? '' : 'none';
      alarm.title = isAlarm ? '⚠️ 报警：无人 + 亮灯' : '正常';
    }
  });
}

function updateLiveState(payload) {
  if (!payload || !payload.sourceId) return;
  sceneStates.set(payload.sourceId, {
    person: !!payload.person,
    light: !!payload.light,
    ts: payload.ts || Date.now(),
  });
  applyStateIcons();
  updateAlarmBanner();
}

function updateAlarmBanner() {
  const banner = $('#alarm-banner');
  if (!banner) return;
  // 统计当前在报警的源
  const alarming = sources.filter((s) => {
    const st = sceneStates.get(s.id);
    return st && !st.person && st.light;
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
  const alarm = !scene.person && scene.light;
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
        const hls = new Hls({ enableWorker: true });
        hls.loadSource(src.url);
        hls.attachMedia(video);
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
  const alarmCount = Array.from(sceneStates.values()).filter((x) => !x.person && x.light).length;
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
    return st && !st.person && st.light;
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
      const alarm = !sc.person && sc.light;
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
  $('#algo-scope').textContent = cfg.scope === 'source' ? '通道配置' : '全局配置';
  // 同步 chip 选中态 + 刷新 slider 数字显示
  syncChipsFromHidden();
  $('#algo-person-threshold')?.dispatchEvent(new Event('input'));
  $('#algo-light-threshold')?.dispatchEvent(new Event('input'));
};

const algorithmPayloadFromForm = () => {
  const weekdays = parseWeekdays($('#algo-weekdays').value);
  return {
    ...(algorithmConfig || {}),
    enabled: $('#algo-enabled').checked,
    scope: 'global',
    scopeId: null,
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
    algorithmConfig = await getAlgorithmConfig();
    fillAlgorithmForm(algorithmConfig);
  } catch (err) {
    addLog('warn', `算法配置加载失败: ${err.message || err}`);
  }
};

const renderSettings = async () => {
  await renderAlgorithmSettings();
  await renderNotificationSettings();
};

$('#algorithm-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  try {
    const payload = algorithmPayloadFromForm();
    algorithmConfig = await updateAlgorithmConfig(null, payload);
    fillAlgorithmForm(algorithmConfig);
    addLog('info', '算法配置已保存');
  } catch (err) {
    alert(err.message || '保存算法配置失败');
  }
});

$('#btn-reload-algorithm').addEventListener('click', renderAlgorithmSettings);

/* -------------------- 通知配置 -------------------- */
const notifyPayloadFromForm = () => {
  const eventType = $('#ntf-event').value;
  const cooldown = Number.parseInt($('#ntf-cooldown').value, 10);
  return {
    name: $('#ntf-name').value.trim(),
    enabled: $('#ntf-enabled').checked,
    url: $('#ntf-url').value.trim(),
    method: $('#ntf-method').value,
    headers: [{ name: 'Content-Type', value: 'application/json' }],
    bodyTemplate: $('#ntf-body').value.trim() || '{"event":"{{event}}","source":"{{source_name}}","location":"{{location}}"}',
    timeoutSec: 10,
    retryCount: 2,
    eventTypes: eventType ? [eventType] : [],
    cooldownSec: Number.isFinite(cooldown) && cooldown > 0 ? cooldown : 1800,
  };
};

const clearNotifyForm = () => {
  $('#ntf-name').value = '';
  $('#ntf-url').value = '';
  $('#ntf-method').value = 'POST';
  $('#ntf-event').value = 'alarm_triggered';
  $('#ntf-cooldown').value = '1800';
  $('#ntf-body').value = '';
  $('#ntf-enabled').checked = true;
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
    tbody.innerHTML = '<tr><td colspan="6" class="muted center">暂无通知目标</td></tr>';
    return;
  }
  tbody.innerHTML = notificationTargets.map((target) => {
    const events = (target.eventTypes || []).length ? target.eventTypes.join(', ') : '全部';
    return `
      <tr>
        <td>${escapeHtml(target.name)}</td>
        <td>${escapeHtml(events)}</td>
        <td>${target.cooldownSec || target.cooldown_sec || 0}s</td>
        <td>${target.enabled ? '是' : '否'}</td>
        <td title="${escapeHtml(target.url)}">${escapeHtml(target.url).slice(0, 64)}</td>
        <td>
          <button class="btn-ghost" data-test-ntf="${target.id}">测试</button>
          <button class="btn-ghost btn-danger" data-del-ntf="${target.id}">删除</button>
        </td>
      </tr>
    `;
  }).join('');
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
    if (!payload.name || !payload.url) {
      alert('通知名称和 URL 不能为空');
      return;
    }
    await createNotificationTarget(payload);
    clearNotifyForm();
    await renderNotificationSettings();
    addLog('info', '通知目标已保存');
  } catch (err) {
    alert(err.message || '保存通知目标失败');
  }
});

$('#btn-refresh-notify').addEventListener('click', renderNotificationSettings);
$('#btn-refresh-ntf-history')?.addEventListener('click', renderNotificationHistory);

$('#btn-test-notify-form').addEventListener('click', async () => {
  try {
    const payload = notifyPayloadFromForm();
    if (!payload.name || !payload.url) {
      alert('通知名称和 URL 不能为空');
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
    await enterApp();
  } catch (_) {
    showLogin();
  }
})();
