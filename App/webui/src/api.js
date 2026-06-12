/* ===========================================================
   EcoAlert · Tauri API 封装
   - 在 Tauri 环境下：调用 Rust 暴露的 command
   - 浏览器环境：降级到 mock（开发时可用 Vite 直接预览 UI）
   =========================================================== */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const isTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__;

const MOCK_PASSWORD = 'admin123';
const MOCK_KEY = 'ecoalert_mock_sources';
const MOCK_GROUPS_KEY = 'ecoalert_mock_groups';
const MOCK_HISTORY_KEY = 'ecoalert_mock_history';
const MOCK_PW_KEY = 'ecoalert_mock_pw';

function mockLoad() {
  // 浏览器预览模式：永远返回最新默认数据（4 组 12 路）。
  // 用户的修改不会被持久化（mock 模式本身就是为了看 UI）。
  return mockDefaultSources();
}

function mockDefaultSources() {
  return [
    // —— A 栋办公 ——
    { id: 'cam-a1', name: '大堂入口',     url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: 'A 栋 1F',     enabled: true, groupId: 'grp-a',  order: 0, createdAt: Date.now() - 80000000 },
    { id: 'cam-a2', name: '前台接待区',   url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: 'A 栋 1F',     enabled: true, groupId: 'grp-a',  order: 1, createdAt: Date.now() - 80000000 },
    { id: 'cam-a3', name: '电梯厅',       url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: 'A 栋 1F',     enabled: true, groupId: 'grp-a',  order: 2, createdAt: Date.now() - 80000000 },
    { id: 'cam-a4', name: '茶水间',       url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: 'A 栋 2F',     enabled: true, groupId: 'grp-a',  order: 3, createdAt: Date.now() - 80000000 },
    // —— B 栋车间 ——
    { id: 'cam-b1', name: '生产线 1',     url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: 'B 栋 车间',   enabled: true, groupId: 'grp-b',  order: 0, createdAt: Date.now() - 70000000 },
    { id: 'cam-b2', name: '生产线 2',     url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: 'B 栋 车间',   enabled: true, groupId: 'grp-b',  order: 1, createdAt: Date.now() - 70000000 },
    { id: 'cam-b3', name: '原材料仓库',   url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: 'B 栋 仓库',   enabled: true, groupId: 'grp-b',  order: 2, createdAt: Date.now() - 70000000 },
    // —— 园区周界 ——
    { id: 'cam-c1', name: '园区东门',     url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: '园区 周界',   enabled: true, groupId: 'grp-c',  order: 0, createdAt: Date.now() - 60000000 },
    { id: 'cam-c2', name: '园区西门',     url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: '园区 周界',   enabled: true, groupId: 'grp-c',  order: 1, createdAt: Date.now() - 60000000 },
    { id: 'cam-c3', name: '停车场',       url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: '园区 停车',   enabled: true, groupId: 'grp-c',  order: 2, createdAt: Date.now() - 60000000 },
    // —— 重点机房（默认分组放 1 路作为对照）——
    { id: 'cam-d1', name: '核心机房',     url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: '核心 机房',   enabled: true, groupId: 'grp-default', order: 0, createdAt: Date.now() - 50000000 },
    { id: 'cam-d2', name: 'UPS 配电室',   url: 'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8', type: 'hls', location: '核心 机房',   enabled: false, groupId: 'grp-default', order: 1, createdAt: Date.now() - 50000000 },
  ];
}
function mockSave(arr) {
  localStorage.setItem(MOCK_KEY, JSON.stringify(arr));
}
function mockLoadGroups() {
  return [
    { id: 'grp-default', name: '默认分组',   order: 0, collapsed: false, createdAt: Date.now() },
    { id: 'grp-a',       name: 'A 栋办公',   order: 1, collapsed: false, createdAt: Date.now() },
    { id: 'grp-b',       name: 'B 栋车间',   order: 2, collapsed: false, createdAt: Date.now() },
    { id: 'grp-c',       name: '园区周界',   order: 3, collapsed: false, createdAt: Date.now() },
  ];
}
function mockSaveGroups(arr) {
  localStorage.setItem(MOCK_GROUPS_KEY, JSON.stringify(arr));
}
function mockLoadHistory(sourceId, limit) {
  try {
    const raw = localStorage.getItem(MOCK_HISTORY_KEY);
    const all = raw ? JSON.parse(raw) : [];
    let out = sourceId ? all.filter((r) => r.sourceId === sourceId) : all;
    out = out.slice(-limit).reverse();
    const bySource = {};
    for (const r of out) {
      (bySource[r.sourceId] = bySource[r.sourceId] || []).push(r);
    }
    return { ok: true, records: out, bySource };
  } catch (_) {
    return { ok: true, records: [], bySource: {} };
  }
}
function mockGetPw() {
  return localStorage.getItem(MOCK_PW_KEY) || MOCK_PASSWORD;
}
function mockSetPw(p) {
  localStorage.setItem(MOCK_PW_KEY, p);
}

/* ---------- 命令封装 ---------- */
export async function login(password) {
  if (isTauri) return invoke('login', { password });
  // 浏览器 mock
  return new Promise((resolve, reject) => {
    setTimeout(() => {
      if (password === mockGetPw()) {
        resolve({ ok: true, token: 'mock-token' });
      } else {
        reject(new Error('密码错误'));
      }
    }, 200);
  });
}

export async function logout() {
  if (isTauri) return invoke('logout');
  return { ok: true };
}

export async function checkAuth() {
  if (isTauri) return invoke('check_auth');
  return { ok: true };
}

export async function listSources() {
  if (isTauri) return invoke('list_sources');
  return mockLoad();
}

export async function listGroups() {
  if (isTauri) return invoke('list_groups');
  return mockLoadGroups();
}

export async function createGroup(payload) {
  if (isTauri) return invoke('create_group', { payload });
  const all = mockLoadGroups();
  const item = {
    id: 'grp-' + Math.random().toString(36).slice(2, 10),
    ...payload,
    createdAt: Date.now(),
  };
  all.push(item);
  mockSaveGroups(all);
  return item;
}

export async function updateGroup(id, payload) {
  if (isTauri) return invoke('update_group', { id, payload });
  const all = mockLoadGroups();
  const idx = all.findIndex((g) => g.id === id);
  if (idx < 0) throw new Error('分组不存在');
  all[idx] = { ...all[idx], ...payload };
  mockSaveGroups(all);
  return all[idx];
}

export async function deleteGroup(id) {
  if (isTauri) return invoke('delete_group', { id });
  let all = mockLoadGroups().filter((g) => g.id !== id);
  if (all.length === 0) all = [{ id: 'grp-default', name: '默认分组', order: 0, collapsed: false, createdAt: Date.now() }];
  mockSaveGroups(all);
  // 把该组下的源移到默认组
  const sources = mockLoad().map((s) => s.groupId === id ? { ...s, groupId: 'grp-default' } : s);
  mockSave(sources);
  return { ok: true };
}

export async function reorder(items) {
  if (isTauri) return invoke('reorder', { items });
  const all = mockLoad();
  for (const it of items) {
    const s = all.find((x) => x.id === it.id);
    if (s) {
      s.order = it.order;
      if (it.groupId) s.groupId = it.groupId;
    }
  }
  mockSave(all);
  return { ok: true };
}

export async function createSource(payload) {
  if (isTauri) return invoke('create_source', { payload });
  const all = mockLoad();
  const item = {
    id: 'src-' + Math.random().toString(36).slice(2, 10),
    order: 0,
    ...payload,
    createdAt: Date.now(),
  };
  all.push(item);
  mockSave(all);
  return item;
}

export async function updateSource(id, payload) {
  if (isTauri) return invoke('update_source', { id, payload });
  const all = mockLoad();
  const idx = all.findIndex((s) => s.id === id);
  if (idx < 0) throw new Error('视频源不存在');
  all[idx] = { ...all[idx], ...payload };
  mockSave(all);
  return all[idx];
}

export async function deleteSource(id) {
  if (isTauri) return invoke('delete_source', { id });
  const all = mockLoad().filter((s) => s.id !== id);
  mockSave(all);
  return { ok: true };
}

export async function reportSceneState(sourceId, person, light) {
  if (isTauri) return invoke('report_scene_state', { sourceId, person, light });
  // mock 模式下不做任何事
  return { ok: true };
}

export async function getStateHistory(sourceId, limit) {
  if (isTauri) return invoke('get_state_history', { sourceId: sourceId ?? null, limit: limit ?? 100 });
  // mock 模式从 localStorage 读
  return mockLoadHistory(sourceId, limit);
}

export async function changePassword(oldPw, newPw) {
  if (isTauri) return invoke('change_password', { oldPassword: oldPw, newPassword: newPw });
  if (oldPw !== mockGetPw()) throw new Error('当前密码错误');
  if (newPw.length < 6) throw new Error('新密码至少 6 位');
  mockSetPw(newPw);
  return { ok: true };
}

export async function getDataDir() {
  if (isTauri) return invoke('get_data_dir');
  return '(浏览器预览模式 - 数据保存在 localStorage)';
}

/* ---------- 事件订阅 ---------- */
// 后端会推送事件：event(日志) / status(码率) / sources(源变更) / scene_state(算法)
export async function onEvent(handler) {
  if (isTauri) return listen('ecoalert://event', (e) => handler(e.payload));
  // 浏览器 mock: 推一些假状态
  const timer = setInterval(() => {
    handler({
      type: 'event',
      level: 'info',
      text: '演示事件：状态模拟推送（仅浏览器预览）',
    });
  }, 8000);
  return () => clearInterval(timer);
}
export async function onStatus(handler) {
  if (isTauri) return listen('ecoalert://status', (e) => handler(e.payload));
  const timer = setInterval(() => {
    const all = mockLoad();
    handler(
      all.map((s, i) => ({
        id: s.id,
        name: s.name,
        online: s.enabled,
        bitrate: s.enabled ? 800 + Math.floor(Math.random() * 2000) : 0,
        fps: s.enabled ? 20 + Math.floor(Math.random() * 10) : 0,
        viewers: s.enabled ? Math.floor(Math.random() * 6) : 0,
        location: s.location,
        ts: Date.now(),
      }))
    );
  }, 3000);
  return () => clearInterval(timer);
}
export async function onSources(handler) {
  if (isTauri) return listen('ecoalert://sources', (e) => handler(e.payload));
  // 浏览器 mock 不再单独推 sources（手动 reload）
  return () => {};
}
export async function onSceneState(handler) {
  if (isTauri) return listen('ecoalert://scene_state', (e) => handler(e.payload));
  // 浏览器 mock：每 4s 给每个源推一次状态（与 Rust 端节奏一致）
  const seedMap = new Map();
  const timer = setInterval(() => {
    const all = mockLoad();
    for (const s of all) {
      if (!s.enabled) continue;
      const cur = seedMap.get(s.id) || { seq: 0, person: false, light: false };
      const t = Date.now() / 4000;
      const idSeed = s.id.charCodeAt(0) || 0;
      const n = idSeed * Math.floor(t);
      const person = (n % 5) < 2;
      const light = (n * 3 % 7) < 3;
      cur.seq++;
      // 写历史（仅变化时）
      if (person !== cur.person || light !== cur.light) {
        const alarm = !person && light;
        const rec = {
          id: 'rec-' + Math.random().toString(36).slice(2, 10),
          sourceId: s.id,
          person, light, alarm,
          ts: Date.now(),
        };
        try {
          const raw = localStorage.getItem(MOCK_HISTORY_KEY);
          const arr = raw ? JSON.parse(raw) : [];
          arr.push(rec);
          if (arr.length > 5000) arr.splice(0, arr.length - 5000);
          localStorage.setItem(MOCK_HISTORY_KEY, JSON.stringify(arr));
        } catch (_) {}
        cur.person = person;
        cur.light = light;
        seedMap.set(s.id, cur);
        handler({ sourceId: s.id, person, light, ts: Date.now() });
      } else if (cur.seq % 3 === 0) {
        handler({ sourceId: s.id, person, light, ts: Date.now() });
      }
    }
  }, 4000);
  return () => clearInterval(timer);
}

export const isTauriEnv = isTauri;
